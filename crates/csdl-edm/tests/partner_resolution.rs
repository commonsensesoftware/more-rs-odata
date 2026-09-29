#![cfg(all(feature = "resolver", feature = "validator"))]

//! Resolution of navigation `Partner` into typed reference paths over the EDM
//! graph (`edm::BindingPathSegment`).

use csdl_edm::edm::{BindingPathSegment, EntityType, Model, SchemaElement};
use csdl_edm::parser::from_xml_reader;
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{validate_document, ValidationError};

const CSDL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0">
  <edmx:DataServices>
    <Schema Namespace="Test" xmlns="http://docs.oasis-open.org/odata/ns/edm">
      <EntityType Name="Customer">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
        <NavigationProperty Name="Orders" Type="Collection(Test.Order)" Partner="Customer" />
      </EntityType>
      <EntityType Name="Order">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
        <NavigationProperty Name="Customer" Type="Test.Customer" Partner="Orders" />
      </EntityType>
      <EntityType Name="Lonely">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
        <NavigationProperty Name="Friend" Type="Test.Customer" Partner="Ghost" />
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

fn navigation_name(segment: &BindingPathSegment) -> String {
    match segment {
        BindingPathSegment::NavigationProperty(w) => w.upgrade().expect("nav alive").name.clone(),
        other => panic!("expected NavigationProperty segment, got {other:?}"),
    }
}

#[test]
fn partner_resolves_to_navigation_on_target_type() {
    let model = model();
    let customer = entity(&model, "Customer");
    let orders = customer
        .navigation_properties()
        .iter()
        .find(|nav| nav.name == "Orders")
        .expect("Orders navigation");

    // Partner "Customer" resolves against the target (Order) to its Customer nav.
    let partner = orders.partner().expect("resolved partner");
    assert_eq!(partner.len(), 1);
    assert_eq!(navigation_name(&partner[0]), "Customer");
}

#[test]
fn unknown_partner_is_unresolved_and_flagged_by_validator() {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    let model = document_model.schemas.first().cloned().expect("schema");

    let lonely = entity(&model, "Lonely");
    let friend = lonely
        .navigation_properties()
        .iter()
        .find(|nav| nav.name == "Friend")
        .expect("Friend navigation");
    let partner = friend.partner().expect("resolved partner");
    assert!(
        matches!(&partner[0], BindingPathSegment::Unresolved(name) if name == "Ghost"),
        "expected Unresolved(Ghost), got {partner:?}"
    );

    let errors = validate_document(&document_model).expect_err("validation should fail");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::UnknownNavigationPartner {
                entity,
                navigation,
                partner,
                target_entity,
            } if entity == "Lonely"
                && navigation == "Friend"
                && partner == "Ghost"
                && target_entity == "Customer"
        )),
        "expected UnknownNavigationPartner for Lonely/Friend, got {errors:?}"
    );
}
