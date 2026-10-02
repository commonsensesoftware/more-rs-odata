#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{
    conformance::ValidationProfile as P, edm::*, parser, resolver::Resolver, validator::*,
};

#[test]
fn collection_items_use_supplied_function_and_url_result_types() {
    for (xml, json) in [
        (
            r#"<Apply Function="Client.value"/>"#,
            r#"{"$Apply":[],"$Function":"Client.value"}"#,
        ),
        (
            "<UrlRef><String>value</String></UrlRef>",
            r#"{"$UrlRef":"value"}"#,
        ),
    ] {
        for model in models_with_collection(
            &format!("<Collection>{xml}</Collection>"),
            &format!("[{json}]"),
            true,
        ) {
            for (ty, collection, valid) in [
                (PrimitiveType::Int32, false, true),
                (PrimitiveType::String, false, false),
                (PrimitiveType::Int32, true, false),
            ] {
                let returned = supplied(ty, collection);

                let mut options = AnnotationValidationOptions::default();
                options
                    .url_result_types
                    .insert("value".into(), returned.clone());
                options.client_functions.insert(
                    "Client.value".into(),
                    vec![ClientFunctionSignature {
                        parameters: vec![],
                        return_type: returned,
                    }],
                );
                let diagnostics = validate_annotations_with_options(&model, &options);
                assert_eq!(diagnostics.is_empty(), valid, "{xml}: {diagnostics:?}");
            }
        }
    }
}

#[test]
fn url_reference_requires_a_scalar_string_operand() {
    for (xml, json, valid) in [
        ("<String>value</String>", r#""value""#, true),
        (
            "<String>https://example.test/value</String>",
            r#""https://example.test/value""#,
            true,
        ),
        ("<Int>1</Int>", "1", false),
        ("<Bool>true</Bool>", "true", false),
        (
            "<Collection><String>value</String></Collection>",
            r#"["value"]"#,
            false,
        ),
    ] {
        for model in models(
            &format!("<UrlRef>{xml}</UrlRef>"),
            &format!(r#"{{"$UrlRef":{json}}}"#),
        ) {
            let mut options = AnnotationValidationOptions::default();
            for uri in ["value", "https://example.test/value"] {
                options
                    .url_result_types
                    .insert(uri.into(), supplied(PrimitiveType::Int32, false));
            }
            let diagnostics = validate_annotations_with_options(&model, &options);
            assert_eq!(diagnostics.is_empty(), valid, "{diagnostics:?}: {xml}");
        }
    }
}

#[test]
fn xml_url_reference_requires_exactly_one_child_expression() {
    for body in ["", "<String>first</String><String>second</String>"] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.Int32"/><ComplexType Name="Owner"><Annotation Term="N.Value"><UrlRef>{body}</UrlRef></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        assert!(parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).is_err());
    }
}

fn supplied(ty: PrimitiveType, collection: bool) -> ExpressionType {
    ExpressionType {
        ty: TermType::Primitive(ty),
        is_collection: collection,
        is_ordered: false,
        nullable: false,
        facets: TypeFacets::default(),
    }
}

#[test]
fn supplied_client_functions_require_nonreserved_qualified_names() {
    for (name, valid) in [
        ("Client.measure", true),
        ("measure", false),
        ("odata.custom", false),
        ("odata.length", false),
    ] {
        for model in models(
            &format!(r#"<Apply Function="{name}"><Bool>true</Bool></Apply>"#),
            &format!(r#"{{"$Apply":[true],"$Function":"{name}"}}"#),
        ) {
            let mut options = AnnotationValidationOptions::default();
            options.client_functions.insert(
                name.into(),
                vec![ClientFunctionSignature {
                    parameters: vec![supplied(PrimitiveType::Boolean, false)],
                    return_type: supplied(PrimitiveType::Int32, false),
                }],
            );
            let diagnostics = validate_annotations_with_options(&model, &options);
            assert_eq!(diagnostics.is_empty(), valid, "{name}: {diagnostics:?}");
        }
    }
}

fn models(xml_expression: &str, json_expression: &str) -> Vec<DocumentModel> {
    models_with_collection(xml_expression, json_expression, false)
}

fn models_with_collection(
    xml_expression: &str,
    json_expression: &str,
    collection: bool,
) -> Vec<DocumentModel> {
    let ty = if collection {
        "Collection(Edm.Int32)"
    } else {
        "Edm.Int32"
    };
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{ty}" Nullable="false"/><ComplexType Name="Owner"><Annotation Term="N.Value">{xml_expression}</Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(
        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"Edm.Int32","$Collection":{collection}}},"Owner":{{"$Kind":"ComplexType","@N.Value":{json_expression}}}}}}}"#
    );
    [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
    ]
    .into_iter()
    .map(|p| {
        let m = Resolver::resolve_document(p.unwrap().edmx.unwrap()).unwrap();
        validate_document(&m).unwrap();
        m
    })
    .collect()
}

#[test]
fn url_reference_validation_uses_supplied_result_type_and_multiplicity() {
    for model in models(
        r#"<UrlRef><String>https://example.test/value</String></UrlRef>"#,
        r#"{"$UrlRef":"https://example.test/value"}"#,
    ) {
        assert!(
            validate_annotations(&model)
                .iter()
                .any(|d| d.issue == AnnotationIssue::UnknownExpressionType)
        );
        for (ty, collection, valid) in [
            (PrimitiveType::Int32, false, true),
            (PrimitiveType::String, false, false),
            (PrimitiveType::Int32, true, false),
        ] {
            let mut options = AnnotationValidationOptions::default();
            options.url_result_types.insert(
                "https://example.test/value".into(),
                supplied(ty, collection),
            );
            let diagnostics = validate_annotations_with_options(&model, &options);
            assert_eq!(diagnostics.is_empty(), valid, "{diagnostics:?}");
        }
    }
}

#[test]
fn client_function_signatures_validate_arity_argument_and_result_types() {
    for (xml, json, valid_argument) in [
        ("<String>a</String>", r#"["a"]"#, true),
        ("<Bool>true</Bool>", "[true]", false),
        ("", "[]", false),
        (
            "<String>a</String><String>b</String>",
            r#"["a","b"]"#,
            false,
        ),
    ] {
        for model in models(
            &format!(r#"<Apply Function="Client.measure">{xml}</Apply>"#),
            &format!(r#"{{"$Apply":{json},"$Function":"Client.measure"}}"#),
        ) {
            assert!(!validate_annotations(&model).is_empty());
            for (returned, collection, valid_result) in [
                (PrimitiveType::Int32, false, true),
                (PrimitiveType::String, false, false),
                (PrimitiveType::Int32, true, false),
            ] {
                let mut options = AnnotationValidationOptions::default();
                options.client_functions.insert(
                    "Client.measure".into(),
                    vec![ClientFunctionSignature {
                        parameters: vec![supplied(PrimitiveType::String, false)],
                        return_type: supplied(returned, collection),
                    }],
                );
                let diagnostics = validate_annotations_with_options(&model, &options);
                assert_eq!(
                    diagnostics.is_empty(),
                    valid_argument && valid_result,
                    "{diagnostics:?}"
                );
            }
        }
    }
}
