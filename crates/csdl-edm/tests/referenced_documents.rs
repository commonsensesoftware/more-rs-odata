#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{
    edm::{ResolvedType, SchemaElement},
    parser,
    resolver::{DocumentRegistry, Resolver},
};
use std::sync::Arc;

fn doc(json: &str) -> csdl_edm::CsdlDocument {
    parser::from_json_reader(json.as_bytes()).unwrap()
}

#[test]
fn relative_references_cycles_and_reused_aliases_preserve_identity() {
    let root = doc(
        r#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"Types","$Alias":"T"}]}},"Root":{"$Alias":"self","Item":{"$Kind":"ComplexType","Value":{"$Type":"T.Value"}}}}"#,
    );
    let external = doc(
        r#"{"$Version":"4.01","$Reference":{"root.json":{"$Include":[{"$Namespace":"Root","$Alias":"R"}]}},"Types":{"$Alias":"self","Value":{"$Kind":"ComplexType","Owner":{"$Type":"R.Item"}}}}"#,
    );
    let mut calls = 0;
    let mut loader = |uri: &str| {
        calls += 1;
        assert_eq!(uri, "https://example.test/models/types.json");
        Ok(external.clone())
    };
    let model =
        Resolver::resolve_with_loader(root, "https://example.test/models/root.json", &mut loader)
            .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(model.schemas.len(), 1);
    assert_eq!(model.referenced_schemas.len(), 1);
    let SchemaElement::ComplexType(item) = model.schemas[0].elements[0].as_ref() else {
        panic!()
    };
    let SchemaElement::ComplexType(value) = model.referenced_schemas[0].elements[0].as_ref() else {
        panic!()
    };
    let ResolvedType::Complex(actual) = &item.properties()[0].ty else {
        panic!()
    };
    assert!(Arc::ptr_eq(actual, value));
}

#[test]
fn nonincluded_schemas_do_not_leak_into_the_referencing_scope() {
    let root = doc(
        r#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"Public"}]}},"Root":{"Item":{"$Kind":"ComplexType","Value":{"$Type":"Hidden.Value"}}}}"#,
    );
    let external = doc(
        r#"{"$Version":"4.01","Public":{"Value":{"$Kind":"ComplexType"}},"Hidden":{"Value":{"$Kind":"ComplexType"}}}"#,
    );
    let result =
        Resolver::resolve_with_loader(root, "https://example.test/root.json", &mut |_: &str| {
            Ok(external.clone())
        });
    assert!(format!("{:?}", result.unwrap_err()).contains("not in scope"));
}

#[test]
fn included_annotations_filter_terms_qualifiers_and_target_namespaces() {
    let root = doc(
        r#"{"$Version":"4.01","$Reference":{"annotations.json":{"$IncludeAnnotations":[{"$TermNamespace":"V","$Qualifier":"Phone","$TargetNamespace":"Root"}]}},"Root":{"Item":{"$Kind":"ComplexType"}}}"#,
    );
    let external = doc(
        r#"{"$Version":"4.01","$Reference":{"root.json":{"$Include":[{"$Namespace":"Root","$Alias":"R"}]}},"V":{"$Alias":"self","Label":{"$Kind":"Term"},"$Annotations":{"R.Item":{"@self.Label#Phone":"yes","@self.Label#Tablet":"no"}}}}"#,
    );
    let model =
        Resolver::resolve_with_loader(root, "https://example.test/root.json", &mut |_: &str| {
            Ok(external.clone())
        })
        .unwrap();
    let annotations = model
        .all_schemas()
        .flat_map(|s| s.annotations.get().into_iter().flatten())
        .collect::<Vec<_>>();
    // The vocabulary namespace is loaded to resolve terms. Its external groups
    // must not bypass the importing document's filter.
    let on_root = annotations
        .iter()
        .filter(|a| a.target_path == "Root.Item")
        .collect::<Vec<_>>();
    assert_eq!(on_root.len(), 1);
    assert_eq!(on_root[0].qualifier.as_deref(), Some("Phone"));
    assert!(on_root[0].term.is_some());
}

#[test]
fn includes_only_documents_and_missing_includes_are_handled() {
    let root = doc(
        r#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"Types"}]}}}"#,
    );
    let mut registry = DocumentRegistry::default();
    registry
        .insert(
            "https://example.test/types.json",
            doc(r#"{"$Version":"4.01","Types":{"Value":{"$Kind":"ComplexType"}}}"#),
        )
        .unwrap();
    let model = Resolver::resolve_with_loader(
        root.clone(),
        "https://example.test/root.json",
        &mut registry,
    )
    .unwrap();
    assert!(model.schemas.is_empty());
    assert_eq!(model.referenced_schemas.len(), 1);
    assert!(
        Resolver::resolve_with_loader(
            root,
            "https://example.test/root.json",
            &mut DocumentRegistry::default()
        )
        .is_err()
    );
}

#[test]
fn xml_base_is_applied_before_resolving_reference_uri() {
    let input = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01" xml:base="../vocab/"><edmx:Reference xml:base="v1/" Uri="types.json"><edmx:Include Namespace="Types"/></edmx:Reference><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Root"/></edmx:DataServices></edmx:Edmx>"#;
    let root = parser::from_xml_reader_with_profile(
        input.as_bytes(),
        csdl_edm::conformance::ValidationProfile::Strict,
    )
    .unwrap();
    let external = doc(r#"{"$Version":"4.01","Types":{"Value":{"$Kind":"ComplexType"}}}"#);
    let model = Resolver::resolve_with_loader(
        root,
        "https://example.test/service/model.xml",
        &mut |uri: &str| {
            assert_eq!(uri, "https://example.test/vocab/v1/types.json");
            Ok(external.clone())
        },
    )
    .unwrap();
    assert_eq!(model.referenced_schemas.len(), 1);
}

#[test]
fn annotations_cannot_bind_to_transitively_loaded_but_unincluded_terms() {
    let root = doc(r#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"Public"}]}},"Root":{"@Hidden.Label":"test","Item":{"$Kind":"ComplexType"}}}"#);
    let external = doc(r#"{"$Version":"4.01","Public":{"Value":{"$Kind":"ComplexType","Internal":{"$Type":"Hidden.Value"}}},"Hidden":{"Value":{"$Kind":"ComplexType"},"Label":{"$Kind":"Term"}}}"#);
    let model = Resolver::resolve_with_loader(root, "https://example.test/root.json", &mut |_: &str| Ok(external.clone())).unwrap();
    assert_eq!(model.referenced_schemas.len(), 2);
    let issues = csdl_edm::validator::validate_annotations(&model);
    assert!(issues.iter().any(|d| d.issue == csdl_edm::validator::AnnotationIssue::UnknownTerm));
}
