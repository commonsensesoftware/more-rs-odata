#![cfg(all(feature = "resolver", feature = "validator"))]

//! Resolution of `NavigationPropertyBinding` Path and Target into reference
//! paths over the EDM graph (`edm::BindingPathSegment`).

use csdl_edm::edm::{
  BindingPathSegment, EntityContainerElement, Model, NavigationPropertyBinding,
};
use csdl_edm::parser::from_xml_reader;
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{ValidationError, validate_document};

const CSDL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0">
  <edmx:DataServices>
    <Schema Namespace="Test" xmlns="http://docs.oasis-open.org/odata/ns/edm">
      <ComplexType Name="Info">
        <NavigationProperty Name="Owner" Type="Test.Person" />
      </ComplexType>
      <EntityType Name="Person">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
      </EntityType>
      <EntityType Name="Category">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
        <NavigationProperty Name="Products" Type="Collection(Test.Product)" />
      </EntityType>
      <EntityType Name="Product">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
        <Property Name="Info" Type="Test.Info" />
        <NavigationProperty Name="Category" Type="Test.Category" />
      </EntityType>
      <EntityType Name="Building">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
        <NavigationProperty Name="Rooms" Type="Collection(Test.Room)" ContainsTarget="true" />
      </EntityType>
      <EntityType Name="Room">
        <Key><PropertyRef Name="Id" /></Key>
        <Property Name="Id" Type="Edm.String" Nullable="false" />
      </EntityType>
      <EntityContainer Name="C">
        <EntitySet Name="Categories" EntityType="Test.Category">
          <NavigationPropertyBinding Path="Products" Target="Products" />
        </EntitySet>
        <EntitySet Name="Products" EntityType="Test.Product">
          <NavigationPropertyBinding Path="Category" Target="Categories" />
          <NavigationPropertyBinding Path="Info/Owner" Target="Persons" />
        </EntitySet>
        <EntitySet Name="Persons" EntityType="Test.Person" />
        <EntitySet Name="Buildings" EntityType="Test.Building">
          <NavigationPropertyBinding Path="Rooms" Target="Buildings/Rooms" />
        </EntitySet>
        <EntitySet Name="BadSet" EntityType="Test.Category">
          <NavigationPropertyBinding Path="Products" Target="MissingSet" />
        </EntitySet>
      </EntityContainer>
    </Schema>
  </edmx:DataServices>
</edmx:Edmx>"#;

fn rooms_model() -> std::sync::Arc<Model> {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    document_model.schemas.first().cloned().expect("schema")
}

fn bindings(model: &Model, set_name: &str) -> Vec<NavigationPropertyBinding> {
    let container = model.entity_container.as_ref().expect("container");
    for element in &container.elements {
        if let EntityContainerElement::EntitySet(set) = element.as_ref() {
            if set.name == set_name {
                return set.navigation_property_bindings().to_vec();
            }
        }
    }
    panic!("entity set {set_name} not found");
}

fn nav_name(segment: &BindingPathSegment) -> String {
    match segment {
    BindingPathSegment::NavigationProperty(w) => w.upgrade().expect("nav alive").name.clone(),
        other => panic!("expected NavigationProperty segment, got {other:?}"),
    }
}

fn set_name(segment: &BindingPathSegment) -> String {
    match segment {
    BindingPathSegment::EntitySet(w) => w.upgrade().expect("set alive").name.clone(),
        other => panic!("expected EntitySet segment, got {other:?}"),
    }
}

#[test]
fn simple_path_and_target_resolve_to_references() {
    let model = rooms_model();
    let b = bindings(&model, "Categories");
    assert_eq!(b.len(), 1);

    // Path "Products" → the Products navigation property on Category.
    assert_eq!(b[0].path.len(), 1);
    assert_eq!(nav_name(&b[0].path[0]), "Products");

    // Target "Products" → the Products entity set.
    assert_eq!(b[0].target.len(), 1);
    assert_eq!(set_name(&b[0].target[0]), "Products");
}

#[test]
fn path_through_complex_typed_property_resolves() {
    let model = rooms_model();
    let b = bindings(&model, "Products");
    // Second binding: Path "Info/Owner", Target "Persons".
    let info_owner = b
        .iter()
        .find(|nb| matches!(nb.path.first(), Some(BindingPathSegment::Property(_))))
        .expect("binding with a complex-property path");

    assert_eq!(info_owner.path.len(), 2);
    match &info_owner.path[0] {
        BindingPathSegment::Property(w) => {
          assert_eq!(w.upgrade().expect("prop alive").name, "Info")
        }
        other => panic!("expected Property segment, got {other:?}"),
    }
    assert_eq!(nav_name(&info_owner.path[1]), "Owner");
    assert_eq!(set_name(&info_owner.target[0]), "Persons");
}

#[test]
fn containment_target_path_resolves_through_contained_nav() {
    let model = rooms_model();
    let b = bindings(&model, "Buildings");
    assert_eq!(b.len(), 1);

    // Target "Buildings/Rooms" → [EntitySet(Buildings), NavigationProperty(Rooms)].
    assert_eq!(b[0].target.len(), 2);
    assert_eq!(set_name(&b[0].target[0]), "Buildings");
    assert_eq!(nav_name(&b[0].target[1]), "Rooms");
}

#[test]
fn unknown_target_is_unresolved_and_flagged_by_validator() {
    let document = from_xml_reader(CSDL.as_bytes()).expect("parse");
    let edmx = document.edmx.expect("edmx");
    let document_model = Resolver::resolve_document(edmx).expect("resolve");
    let model = document_model.schemas.first().cloned().expect("schema");

    // The resolved target carries an Unresolved segment naming the bad target.
    let b = bindings(&model, "BadSet");
    assert_eq!(b.len(), 1);
    assert!(
        matches!(&b[0].target[0], BindingPathSegment::Unresolved(name) if name == "MissingSet"),
        "expected Unresolved(\"MissingSet\"), got {:?}",
        b[0].target
    );

    // Validation operates on the resolved path and reports it.
    let errors = validate_document(&document_model).expect_err("validation should fail");
    assert!(
        errors.iter().any(|e| matches!(
            e,
            ValidationError::UnknownContainerTarget {
                source_kind: "EntitySet.NavigationPropertyBinding",
                source,
                target,
                ..
            } if source == "BadSet" && target == "MissingSet"
        )),
        "expected UnknownContainerTarget for BadSet/MissingSet, got {errors:?}"
    );
}
