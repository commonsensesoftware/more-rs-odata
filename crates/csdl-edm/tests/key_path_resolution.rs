#![cfg(all(feature = "resolver", feature = "validator"))]

//! Resolution of entity `Key` property refs into typed reference paths over the
//! EDM graph (`edm::KeyPathSegment`).

use csdl_edm::edm::{key_path_to_string, EntityType, KeyPathSegment, Model, SchemaElement};
use csdl_edm::parser::from_xml_reader;
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{validate_document, ValidationError};

const CSDL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0">
  <edmx:DataServices>
    <Schema Namespace="Test" xmlns="http://docs.oasis-open.org/odata/ns/edm">
      <ComplexType Name="Address">
        <Property Name="Zip" Type="Edm.String" Nullable="false" />
      </ComplexType>
      <EntityType Name="Customer">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
      </EntityType>
      <EntityType Name="Order">
        <Key><PropertyRef Name="Shipping/Zip" Alias="ShipZip" /></Key>
        <Property Name="Shipping" Type="Test.Address" Nullable="false" />
      </EntityType>
      <EntityType Name="Region">
        <Key><PropertyRef Name="Where" /></Key>
        <Property Name="Where" Type="Test.Address" Nullable="false" />
      </EntityType>
    </Schema>
  </edmx:DataServices>
</edmx:Edmx>"#;

fn model() -> std::sync::Arc<Model> {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    document_model.schemas.first().cloned().expect("schema")
}

fn entity(model: &Model, name: &str) -> std::sync::Arc<EntityType> {
    model
        .elements
        .iter()
        .find_map(|element| match element.as_ref() {
            SchemaElement::EntityType(entity) if entity.name == name => Some(entity.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("entity {name} not found"))
}

fn property_name(segment: &KeyPathSegment) -> String {
    match segment {
        KeyPathSegment::Property(w) => w.upgrade().expect("property alive").name.clone(),
        other => panic!("expected Property segment, got {other:?}"),
    }
}

#[test]
fn simple_key_resolves_to_a_property_reference() {
    let model = model();
    let customer = entity(&model, "Customer");

    assert_eq!(customer.keys().len(), 1);
    let key = &customer.keys()[0];
    assert_eq!(key.len(), 1);
    assert_eq!(property_name(&key[0]), "Id");
}

#[test]
fn key_through_complex_typed_property_resolves() {
    let model = model();
    let order = entity(&model, "Order");

    assert_eq!(order.keys().len(), 1);
    let key = &order.keys()[0];

    // "Shipping/Zip" → [Property(Shipping), Property(Zip)].
    assert_eq!(key.len(), 2);
    assert_eq!(property_name(&key[0]), "Shipping");
    assert_eq!(property_name(&key[1]), "Zip");
    assert_eq!(key_path_to_string(key), "Shipping/Zip");
}

#[test]
fn non_scalar_key_resolves_and_is_flagged_by_validator() {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    let model = document_model.schemas.first().cloned().expect("schema");

    // A key that terminates at a complex type still resolves to a property path.
    let region = entity(&model, "Region");
    let key = &region.keys()[0];
    assert_eq!(property_name(&key[0]), "Where");

    // ...but the validator reports it as a non-scalar key.
    let errors = validate_document(&document_model).expect_err("validation should fail");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::NonScalarKeyProperty { entity, key }
                if entity == "Region" && key == "Where"
        )),
        "expected NonScalarKeyProperty for Region/Where, got {errors:?}"
    );
}
