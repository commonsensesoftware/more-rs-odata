#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{
    CsdlDocument, CsdlFormat,
    conformance::ValidationProfile,
    parser,
    resolver::{DocumentLoader, ReferenceRequest, Resolver},
    validator,
};

fn doc(source: &str) -> CsdlDocument {
    parser::from_json_reader(source.as_bytes()).unwrap()
}

fn root() -> CsdlDocument {
    doc(
        r#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"Public","@Core.Description":"included"}],"@Core.SchemaVersion":"2026-09","@Core.SchemaVersion@Core.Description":"version hint"}},"Org.OData.Core.V1":{"$Alias":"Core","SchemaVersion":{"$Kind":"Term","$Type":"Edm.String","$AppliesTo":["Reference"]},"Description":{"$Kind":"Term","$Type":"Edm.String","$AppliesTo":["Include","Annotation"]}},"Root":{}}"#,
    )
}

#[test]
fn reference_and_include_annotations_roundtrip_and_resolve_in_both_formats() {
    for format in [CsdlFormat::Xml, CsdlFormat::Json] {
        let original = root();
        let mut output = Vec::new();
        original
            .write_as_with_profile(&mut output, format, ValidationProfile::Strict)
            .unwrap();
        let parsed = match format {
            CsdlFormat::Xml => {
                parser::from_xml_reader_with_profile(output.as_slice(), ValidationProfile::Strict)
            }
            CsdlFormat::Json => {
                parser::from_json_reader_with_profile(output.as_slice(), ValidationProfile::Strict)
            }
        }
        .unwrap();
        assert_eq!(
            original.edmx.as_ref().unwrap().references,
            parsed.edmx.as_ref().unwrap().references
        );
        let model = Resolver::resolve_document(parsed.edmx.unwrap()).unwrap();
        assert_eq!(model.references[0].annotations.len(), 2);
        assert_eq!(model.references[0].includes[0].annotations.len(), 1);
        assert!(
            model.references[0]
                .annotations
                .iter()
                .all(|a| a.term.is_some())
        );
        assert!(
            validator::validate_annotations(&model).is_empty(),
            "{:?}",
            validator::validate_annotations(&model)
        );
    }
}

#[test]
fn loader_observes_alias_resolved_schema_version_before_retrieval() {
    struct Loader {
        inspected: bool,
    }
    impl DocumentLoader for Loader {
        fn inspect_reference(&mut self, request: ReferenceRequest<'_>) -> Result<(), String> {
            assert_eq!(request.absolute_uri, "https://example.test/types.json");
            assert_eq!(request.schema_version, Some("2026-09"));
            assert_eq!(request.reference.includes[0].annotations.len(), 1);
            self.inspected = true;
            Ok(())
        }
        fn load(&mut self, uri: &str) -> Result<CsdlDocument, String> {
            assert!(self.inspected);
            assert_eq!(uri, "https://example.test/types.json");
            Ok(doc(r#"{"$Version":"4.01","Public":{}}"#))
        }
    }
    let model = Resolver::resolve_with_loader(
        root(),
        "https://example.test/root.json",
        &mut Loader { inspected: false },
    )
    .unwrap();
    assert_eq!(model.references[0].annotations.len(), 2);
    assert!(validator::validate_annotations(&model).is_empty());
}

#[test]
fn schema_version_hints_require_the_core_term_without_a_qualifier() {
    struct Loader {
        expected: Option<&'static str>,
        inspected: bool,
    }
    impl DocumentLoader for Loader {
        fn inspect_reference(&mut self, request: ReferenceRequest<'_>) -> Result<(), String> {
            assert_eq!(request.absolute_uri, "https://example.test/types");
            assert_eq!(request.schema_version, self.expected);
            self.inspected = true;
            Ok(())
        }
        fn load(&mut self, uri: &str) -> Result<CsdlDocument, String> {
            assert!(self.inspected);
            assert_eq!(uri, "https://example.test/types");
            Ok(doc(r#"{"$Version":"4.01","Public":{}}"#))
        }
    }
    for (term, qualifier, expected) in [
        (None, None, None),
        (Some("Core.SchemaVersion"), None, Some("2026-10")),
        (Some("Core.SchemaVersion"), Some("View"), None),
        (Some("Other.SchemaVersion"), None, None),
    ] {
        let xml_annotation = term.map(|term| {
            let qualifier = qualifier.map(|q| format!(r#" Qualifier="{q}""#)).unwrap_or_default();
            format!(r#"<Annotation xmlns="http://docs.oasis-open.org/odata/ns/edm" Term="{term}"{qualifier} String="2026-10"/>"#)
        }).unwrap_or_default();
        let json_annotation = term.map(|term| {
            let qualifier = qualifier.map(|q| format!("#{q}")).unwrap_or_default();
            format!(r#", "@{term}{qualifier}":"2026-10""#)
        }).unwrap_or_default();
        let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:Reference Uri="types"><edmx:Include Namespace="Public"/>{xml_annotation}</edmx:Reference><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Org.OData.Core.V1" Alias="Core"><Term Name="SchemaVersion" Type="Edm.String"/></Schema><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Other"><Term Name="SchemaVersion" Type="Edm.String"/></Schema></edmx:DataServices></edmx:Edmx>"#);
        let json = format!(r#"{{"$Version":"4.01","$Reference":{{"types":{{"$Include":[{{"$Namespace":"Public"}}]{json_annotation}}}}},"Org.OData.Core.V1":{{"$Alias":"Core","SchemaVersion":{{"$Kind":"Term"}}}},"Other":{{"SchemaVersion":{{"$Kind":"Term"}}}}}}"#);
        for document in [parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict).unwrap(), parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict).unwrap()] {
            let mut loader = Loader { expected, inspected: false };
            let model = Resolver::resolve_with_loader(document, "https://example.test/root", &mut loader).unwrap();
            assert!(loader.inspected);
            validator::validate_document(&model).unwrap();
            assert!(validator::validate_annotations(&model).is_empty());
        }
    }
}

#[test]
fn structural_cast_paths_cannot_use_hidden_loaded_namespaces() {
    let external = doc(
        r#"{"$Version":"4.01","Public":{"E":{"$Kind":"EntityType","Data":{"$Type":"Hidden.C"}}},"Hidden":{"C":{"$Kind":"ComplexType"},"E":{"$Kind":"EntityType","$BaseType":"Public.E","Nav":{"$Kind":"NavigationProperty","$Type":"Public.E"}}}}"#,
    );
    let root = doc(
        r#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"Public"}]}},"Root":{"C":{"$Kind":"EntityContainer","Items":{"$Collection":true,"$Type":"Public.E","$NavigationPropertyBinding":{"Hidden.E/Nav":"Items"}}}}}"#,
    );
    let error =
        Resolver::resolve_with_loader(root, "https://example.test/root.json", &mut |_: &str| {
            Ok(external.clone())
        })
        .unwrap_err();
    assert!(format!("{error:?}").contains("not in scope"));
}

#[test]
fn hidden_namespaces_never_supply_expression_bindings_or_external_targets() {
    let external = doc(
        r#"{"$Version":"4.01","Public":{"C":{"$Kind":"ComplexType","Data":{"$Type":"Hidden.C"}}},"Hidden":{"C":{"$Kind":"ComplexType"},"E":{"$Kind":"EnumType","A":0},"F":[{"$Kind":"Function","$ReturnType":{"$Type":"Edm.String"}}],"T":{"$Kind":"Term","$Type":"Edm.String"},"@Hidden.T":{"$LabeledElement":"value","$Name":"L"}}}"#,
    );
    for expression in [
        r#"{"$Cast":{},"$Type":"Hidden.C"}"#,
        r#"{"$EnumMember":"Hidden.E/A"}"#,
        r#"{"$Apply":[],"$Function":"Hidden.F"}"#,
        r#"{"$LabeledElementReference":"Hidden.L"}"#,
        r#"{"$ModelElementPath":"Hidden.C"}"#,
    ] {
        let source = format!(
            r#"{{"$Version":"4.01","$Reference":{{"types.json":{{"$Include":[{{"$Namespace":"Public"}}]}}}},"Root":{{"T":{{"$Kind":"Term","$Type":"Edm.Untyped"}},"@Root.T":{expression},"$Annotations":{{"Hidden.C":{{"@Root.T":null}}}}}}}}"#
        );
        let model = Resolver::resolve_with_loader(
            doc(&source),
            "https://example.test/root.json",
            &mut |_: &str| Ok(external.clone()),
        )
        .unwrap();
        let annotations = model.schemas[0].annotations.get().unwrap();
        let bindings = &annotations[0].expression_bindings;
        assert!(
            bindings.types.is_empty()
                && bindings.functions.values().all(Vec::is_empty)
                && bindings.enum_members.values().all(Vec::is_empty)
                && bindings.labels.is_empty(),
            "{expression}"
        );
        let issues = validator::validate_annotations(&model);
        assert!(
            issues
                .iter()
                .any(|d| d.issue == validator::AnnotationIssue::UnresolvedTarget)
        );
        assert!(
            issues.iter().any(|d| matches!(
                d.issue,
                validator::AnnotationIssue::IncompatibleValue
                    | validator::AnnotationIssue::InvalidExpressionPath
            )),
            "{expression}: {issues:?}"
        );
    }
}

#[test]
fn include_annotations_retains_nested_expression_and_annotation_metadata() {
    let root = doc(
        r#"{"$Version":"4.01","$Reference":{"v.json":{"$IncludeAnnotations":[{"$TermNamespace":"V","$Qualifier":"Phone","$TargetNamespace":"Root"}]}},"Root":{"Item":{"$Kind":"ComplexType"}}}"#,
    );
    let external = doc(
        r#"{"$Version":"4.01","$Reference":{"root.json":{"$Include":[{"$Namespace":"Root"}]}},"V":{"Text":{"$Kind":"Term","$Type":"Edm.String"},"Flag":{"$Kind":"Term","$Type":"Edm.Boolean"},"$Annotations":{"Root.Item":{"@V.Text#Phone":{"$If":[true,"yes","no"],"@V.Flag":true},"@V.Text#Phone@V.Flag":false,"@V.Text#Tablet":"excluded"}}}}"#,
    );
    let model =
        Resolver::resolve_with_loader(root, "https://example.test/root.json", &mut |_: &str| {
            Ok(external.clone())
        })
        .unwrap();
    let annotations = model.schemas[0].annotations.get().unwrap();
    assert_eq!(annotations.len(), 3);
    assert!(annotations.iter().all(|a| a.term.is_some()));
    assert_eq!(
        annotations
            .iter()
            .filter(|a| a.term_name == "V.Flag")
            .count(),
        2
    );
    assert!(
        annotations
            .iter()
            .all(|a| a.qualifier.as_deref() != Some("Tablet"))
    );
    assert!(
        validator::validate_annotations(&model).is_empty(),
        "{:?}",
        validator::validate_annotations(&model)
    );
}
