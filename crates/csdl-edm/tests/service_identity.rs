#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{
    conformance::{DocumentKind, ValidationProfile},
    parser,
    resolver::Resolver,
    validator::validate_document_as,
};
use std::sync::Arc;

#[test]
fn explicit_service_container_is_preserved_and_resolves_by_identity() {
    let input = r#"{"$Version":"4.01","$EntityContainer":"N.Second","N":{"First":{"$Kind":"EntityContainer"},"Second":{"$Kind":"EntityContainer"}}}"#;
    let document = parser::from_json_reader(input.as_bytes()).unwrap();
    assert_eq!(document.to_json_value()["$EntityContainer"], "N.Second");
    let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
    let selected = model.service_container.as_ref().unwrap();
    assert_eq!(selected.name, "Second");
    assert!(Arc::ptr_eq(selected, &model.schemas[0].containers()[1]));
    assert!(
        validate_document_as(&model, DocumentKind::Model, ValidationProfile::Strict).is_valid()
    );
    assert!(
        !validate_document_as(
            &model,
            DocumentKind::ServiceMetadata,
            ValidationProfile::Strict
        )
        .is_valid()
    );
}

#[test]
fn model_documents_do_not_acquire_an_arbitrary_service_container() {
    let input = r#"{"$Version":"4.01","N":{"Only":{"$Kind":"EntityContainer"}}}"#;
    let document = parser::from_json_reader(input.as_bytes()).unwrap();
    assert!(document.to_json_value().get("$EntityContainer").is_none());
    let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
    assert!(model.service_container.is_none());
    assert!(
        validate_document_as(&model, DocumentKind::Model, ValidationProfile::Strict).is_valid()
    );
    assert!(
        !validate_document_as(
            &model,
            DocumentKind::ServiceMetadata,
            ValidationProfile::Tolerant
        )
        .is_valid()
    );
}

#[test]
fn service_container_must_be_namespace_qualified_and_exist() {
    for target in [
        "Alias.Service",
        "N.Missing",
        "N.Type",
        ".Service",
        "Service",
    ] {
        let input = format!(
            r#"{{"$Version":"4.01","$EntityContainer":"{target}","N":{{"$Alias":"Alias","Service":{{"$Kind":"EntityContainer"}},"Type":{{"$Kind":"ComplexType"}}}}}}"#
        );
        let document = parser::from_json_reader(input.as_bytes()).unwrap();
        assert!(
            Resolver::resolve_document(document.edmx.unwrap()).is_err(),
            "{target}"
        );
    }
}

#[test]
fn xml_service_identity_roundtrips_to_json() {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><EntityType Name="Item"/><EntityContainer Name="Service"><Singleton Name="Current" Type="N.Item"/></EntityContainer></Schema></edmx:DataServices></edmx:Edmx>"#;
    let document =
        parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict).unwrap();
    let json = document.to_json_value().to_string();
    let reparsed =
        parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict).unwrap();
    assert_eq!(document, reparsed);
    let model = Resolver::resolve_document(reparsed.edmx.unwrap()).unwrap();
    assert!(
        validate_document_as(
            &model,
            DocumentKind::ServiceMetadata,
            ValidationProfile::Strict
        )
        .is_valid()
    );
}
