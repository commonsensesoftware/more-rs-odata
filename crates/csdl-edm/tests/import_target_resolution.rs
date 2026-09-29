#![cfg(all(feature = "resolver", feature = "validator"))]

//! Resolution of `FunctionImport`/`ActionImport` `EntitySet` targets into typed
//! reference paths over the EDM graph (`edm::BindingPathSegment`).

use csdl_edm::edm::{BindingPathSegment, EntityContainerElement, FunctionImport, Model};
use csdl_edm::parser::from_xml_reader;
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{validate_document, ValidationError};

const CSDL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0">
  <edmx:DataServices>
    <Schema Namespace="Test" xmlns="http://docs.oasis-open.org/odata/ns/edm">
      <EntityType Name="Product">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
      </EntityType>
      <Function Name="All" IsBound="false">
        <ReturnType Type="Collection(Test.Product)" />
      </Function>
      <EntityContainer Name="C">
        <EntitySet Name="Products" EntityType="Test.Product" />
        <FunctionImport Name="AllProducts" Function="Test.All" EntitySet="Products" />
        <FunctionImport Name="BadImport" Function="Test.All" EntitySet="MissingSet" />
      </EntityContainer>
    </Schema>
  </edmx:DataServices>
</edmx:Edmx>"#;

fn model() -> std::sync::Arc<Model> {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    document_model.schemas.first().cloned().expect("schema")
}

fn function_import(model: &Model, name: &str) -> std::sync::Arc<FunctionImport> {
    let container = model.entity_container.as_ref().expect("container");
    container
        .elements
        .iter()
        .find_map(|element| match element.as_ref() {
            EntityContainerElement::FunctionImport(import) if import.name == name => {
                Some(import.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("function import {name} not found"))
}

#[test]
fn import_target_resolves_to_entity_set_reference() {
    let model = model();
    let import = function_import(&model, "AllProducts");
    let target = import.entity_set.as_deref().expect("entity set target");

    assert_eq!(target.len(), 1);
    match &target[0] {
        BindingPathSegment::EntitySet(w) => {
            assert_eq!(w.upgrade().expect("set alive").name, "Products");
        }
        other => panic!("expected EntitySet segment, got {other:?}"),
    }
}

#[test]
fn unknown_import_target_is_unresolved_and_flagged_by_validator() {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    let model = document_model.schemas.first().cloned().expect("schema");

    let import = function_import(&model, "BadImport");
    let target = import.entity_set.as_deref().expect("entity set target");
    assert!(
        matches!(&target[0], BindingPathSegment::Unresolved(name) if name == "MissingSet"),
        "expected Unresolved(MissingSet), got {target:?}"
    );

    let errors = validate_document(&document_model).expect_err("validation should fail");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ValidationError::UnknownContainerTarget {
                source_kind: "FunctionImport.EntitySet",
                source,
                target,
                ..
            } if source == "BadImport" && target == "MissingSet"
        )),
        "expected UnknownContainerTarget for BadImport/MissingSet, got {errors:?}"
    );
}
