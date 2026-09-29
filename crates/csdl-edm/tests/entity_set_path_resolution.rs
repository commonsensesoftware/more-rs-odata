#![cfg(all(feature = "resolver", feature = "validator"))]

//! Resolution of operation `EntitySetPath` into typed reference paths over the
//! EDM graph (`edm::EntitySetPathSegment`).

use csdl_edm::edm::{EntitySetPathSegment, Function, Model, SchemaElement};
use csdl_edm::parser::from_xml_reader;
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{validate_document, ValidationError};

const CSDL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0">
  <edmx:DataServices>
    <Schema Namespace="Test" xmlns="http://docs.oasis-open.org/odata/ns/edm">
      <EntityType Name="Category">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
        <NavigationProperty Name="Products" Type="Collection(Test.Product)" />
      </EntityType>
      <EntityType Name="Product">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
      </EntityType>
      <Function Name="TopProducts" IsBound="true" EntitySetPath="cat/Products">
        <Parameter Name="cat" Type="Test.Category" />
        <ReturnType Type="Collection(Test.Product)" />
      </Function>
      <Function Name="BrokenPath" IsBound="true" EntitySetPath="cat/Ghost">
        <Parameter Name="cat" Type="Test.Category" />
        <ReturnType Type="Collection(Test.Product)" />
      </Function>
    </Schema>
  </edmx:DataServices>
</edmx:Edmx>"#;

fn model() -> std::sync::Arc<Model> {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    document_model.schemas.first().cloned().expect("schema")
}

fn function(model: &Model, name: &str) -> std::sync::Arc<Function> {
    model
        .elements
        .iter()
        .find_map(|element| match element.as_ref() {
            SchemaElement::Function(function) if function.name == name => Some(function.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("function {name} not found"))
}

#[test]
fn entity_set_path_resolves_binding_parameter_and_navigation() {
    let model = model();
    let top = function(&model, "TopProducts");
    let path = top.entity_set_path.as_deref().expect("entity set path");

    // "cat/Products" → [BindingParameter(cat), NavigationProperty(Products)].
    assert_eq!(path.len(), 2);
    assert!(
        matches!(&path[0], EntitySetPathSegment::BindingParameter(name) if name == "cat"),
        "expected BindingParameter(cat), got {:?}",
        path[0]
    );
    match &path[1] {
        EntitySetPathSegment::NavigationProperty(w) => {
            assert_eq!(w.upgrade().expect("nav alive").name, "Products");
        }
        other => panic!("expected NavigationProperty segment, got {other:?}"),
    }
}

#[test]
fn unresolved_entity_set_path_segment_is_flagged_by_validator() {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    let model = document_model.schemas.first().cloned().expect("schema");

    // "cat/Ghost" → the trailing segment cannot be resolved.
    let broken = function(&model, "BrokenPath");
    let path = broken.entity_set_path.as_deref().expect("entity set path");
    assert!(
        matches!(path.last(), Some(EntitySetPathSegment::Unresolved(name)) if name == "Ghost"),
        "expected trailing Unresolved(Ghost), got {path:?}"
    );

    let errors = validate_document(&document_model).expect_err("validation should fail");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::InvalidEntitySetPath { operation, .. } if operation == "BrokenPath"
        )),
        "expected InvalidEntitySetPath for BrokenPath, got {errors:?}"
    );
}
