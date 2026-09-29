#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::edm::{BindingPathSegment, EntityContainerElement, SchemaElement};
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{ValidationError, validate_document};
use csdl_edm::{CsdlDocument, parser};
use std::sync::Arc;

fn documents(body: &str) -> Vec<CsdlDocument> {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo" Alias="D">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let json = document.to_json_value().to_string();
    vec![document, parser::from_json_reader(json.as_bytes()).unwrap()]
}

const TYPES: &str = r#"
    <EntityType Name="Base"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType>
    <EntityType Name="Derived" BaseType="D.Base"><NavigationProperty Name="Related" Type="D.Base"/></EntityType>"#;

#[test]
fn cross_container_bindings_and_imports_resolve_forward_targets_with_shared_identity() {
    for document in documents(&format!(
        r#"{TYPES}
        <Function Name="List"><ReturnType Type="Collection(D.Base)"/></Function>
        <EntityContainer Name="First">
          <EntitySet Name="Sources" EntityType="D.Base"><NavigationPropertyBinding Path="D.Derived/Related" Target="D.Second/Targets"/></EntitySet>
          <FunctionImport Name="List" Function="D.List" EntitySet="D.Second/Targets" IncludeInServiceDocument="true"/>
        </EntityContainer>
        <EntityContainer Name="Second"><EntitySet Name="Targets" EntityType="D.Base"/></EntityContainer>"#
    )) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let schema = &model.schemas[0];
        assert_eq!(schema.containers().len(), 2);
        let EntityContainerElement::EntitySet(source) = schema.containers()[0].elements[0].as_ref()
        else {
            panic!()
        };
        let EntityContainerElement::EntitySet(target) = schema.containers()[1].elements[0].as_ref()
        else {
            panic!()
        };
        let binding = &source.navigation_property_bindings()[0];
        assert!(matches!(
            &binding.path[0],
            BindingPathSegment::EntityTypeCast(_)
        ));
        assert!(
            matches!(&binding.target[0], BindingPathSegment::EntitySet(reference) if Arc::ptr_eq(&reference.upgrade().unwrap(), target))
        );
        let EntityContainerElement::FunctionImport(import) =
            schema.containers()[0].elements[1].as_ref()
        else {
            panic!()
        };
        let function = schema
            .elements
            .iter()
            .find_map(|e| {
                if let SchemaElement::Function(f) = e.as_ref() {
                    Some(f)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(Arc::ptr_eq(&import.overloads[0], function));
    }
}

#[test]
fn container_inheritance_preserves_resources_and_allows_type_narrowing() {
    for document in documents(&format!(
        r#"{TYPES}
        <EntityContainer Name="Parent"><EntitySet Name="Items" EntityType="D.Base"/><Singleton Name="Current" Type="D.Base"/></EntityContainer>
        <EntityContainer Name="Child" Extends="D.Parent"><EntitySet Name="Items" EntityType="D.Derived"/></EntityContainer>"#
    )) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let containers = model.schemas[0].containers();
        let EntityContainerElement::EntitySet(items) = containers[1].elements[0].as_ref() else {
            panic!()
        };
        assert_eq!(items.target.name, "Derived");
        assert!(Arc::ptr_eq(
            &containers[0].elements[1],
            &containers[1].elements[1]
        ));
    }
}

#[test]
fn target_paths_traverse_singleton_complex_properties_to_containment() {
    for document in documents(&format!(
        r#"{TYPES}
        <ComplexType Name="Holder"><NavigationProperty Name="Contained" Type="D.Base" ContainsTarget="true"/></ComplexType>
        <EntityType Name="Root"><Property Name="Details" Type="D.Holder"/></EntityType>
        <EntityContainer Name="Container"><Singleton Name="Root" Type="D.Root"/>
          <EntitySet Name="Sources" EntityType="D.Derived"><NavigationPropertyBinding Path="Related" Target="Root/Details/Contained"/></EntitySet>
        </EntityContainer>"#
    )) {
        validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap()).unwrap();
    }
}

#[test]
fn import_selects_only_unbound_overloads_and_rejects_bound_only_targets() {
    let functions = r#"
        <Function Name="Find"><Parameter Name="id" Type="Edm.Int32"/><ReturnType Type="Edm.String"/></Function>
        <Function Name="Find" IsBound="true"><Parameter Name="binding" Type="Edm.String"/><ReturnType Type="Edm.Boolean"/></Function>"#;
    for document in documents(&format!(
        r#"{functions}<EntityContainer Name="C"><FunctionImport Name="Find" Function="D.Find"/></EntityContainer>"#
    )) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let EntityContainerElement::FunctionImport(import) =
            model.schemas[0].containers()[0].elements[0].as_ref()
        else {
            panic!()
        };
        assert_eq!(import.overloads.len(), 1);
        assert!(!import.overloads[0].is_bound);
    }
    for document in documents(
        r#"<Action Name="Run" IsBound="true"><Parameter Name="binding" Type="Edm.String"/></Action><EntityContainer Name="C"><ActionImport Name="Run" Action="D.Run"/></EntityContainer>"#,
    ) {
        assert!(Resolver::resolve_document(document.edmx.unwrap()).is_err());
    }
}

#[test]
fn rejects_duplicate_semantic_binding_paths_and_invalid_imports() {
    for document in documents(&format!(
        r#"{TYPES}
        <EntityContainer Name="C"><EntitySet Name="Items" EntityType="D.Base">
          <NavigationPropertyBinding Path="D.Derived/Related" Target="Items"/>
          <NavigationPropertyBinding Path="Demo.Derived/Related" Target="Items"/>
        </EntitySet></EntityContainer>"#
    )) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        assert!(validate_document(&model).unwrap_err().iter().any(|e| matches!(e, ValidationError::InvalidNavigationPropertyBinding { reason, .. } if *reason == "Binding paths must be unique within a source")));
    }
    for document in documents(&format!(
        r#"{TYPES}
        <Function Name="Value"><Parameter Name="p" Type="Edm.String"/><ReturnType Type="Edm.String"/></Function>
        <EntityContainer Name="C"><EntitySet Name="Items" EntityType="D.Base"/><FunctionImport Name="Value" Function="D.Value" EntitySet="Items" IncludeInServiceDocument="true"/></EntityContainer>"#
    )) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        assert!(
            validate_document(&model)
                .unwrap_err()
                .iter()
                .filter(|e| matches!(e, ValidationError::InvalidImport { .. }))
                .count()
                >= 2
        );
    }
}

#[test]
fn cross_schema_targets_resolve_by_namespace_and_keep_nullable_singletons() {
    let mut document = documents(&format!(r#"{TYPES}<EntityContainer Name="C"><EntitySet Name="Sources" EntityType="D.Derived"><NavigationPropertyBinding Path="Related" Target="B.C/Target"/></EntitySet></EntityContainer>"#)).remove(0);
    let mut other = documents(r#"<EntityContainer Name="C"><Singleton Name="Target" Type="D.Base" Nullable="true"/></EntityContainer>"#).remove(0).edmx.unwrap().schemas.remove(0);
    other.namespace = "Other".into();
    other.alias = Some("B".into());
    document.edmx.as_mut().unwrap().schemas.push(other);
    let json = document.to_json_value().to_string();
    for document in [document, parser::from_json_reader(json.as_bytes()).unwrap()] {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let EntityContainerElement::Singleton(target) =
            model.schemas[1].containers()[0].elements[0].as_ref()
        else {
            panic!()
        };
        assert_eq!(target.nullable, Some(true));
        let EntityContainerElement::EntitySet(source) =
            model.schemas[0].containers()[0].elements[0].as_ref()
        else {
            panic!()
        };
        assert!(
            matches!(&source.navigation_property_bindings()[0].target[0], BindingPathSegment::Singleton(s) if Arc::ptr_eq(&s.upgrade().unwrap(), target))
        );
    }
}

#[test]
fn rejects_wrong_qualified_container_and_complex_terminal_target() {
    for target in ["Unknown.C/Target", "Target/Details"] {
        for document in documents(&format!(
            r#"{TYPES}<ComplexType Name="Details"/><EntityType Name="Owner"><Property Name="Details" Type="D.Details"/></EntityType>
            <EntityContainer Name="C"><Singleton Name="Target" Type="D.Owner"/><EntitySet Name="Sources" EntityType="D.Derived"><NavigationPropertyBinding Path="Related" Target="{target}"/></EntitySet></EntityContainer>"#
        )) {
            let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
            assert!(validate_document(&model).is_err());
        }
    }
}

#[test]
fn json_import_annotations_and_binding_errors_are_not_discarded() {
    let json = r#"{"$Version":"4.01","Demo":{"C":{"$Kind":"EntityContainer","Run":{"$Action":"Demo.Run","@Demo.Label":"hello"}}}}"#;
    let document = parser::from_json_reader(json.as_bytes()).unwrap();
    assert_eq!(
        document.to_json_value()["Demo"]["C"]["Run"]["@Demo.Label"],
        "hello"
    );
    let json = r#"{"$Version":"4.01","Demo":{"C":{"$Kind":"EntityContainer","Items":{"$Type":"Demo.Item","$Collection":true,"$NavigationPropertyBinding":{"Related":false}}}}}"#;
    assert!(parser::from_json_reader(json.as_bytes()).is_err());
}

#[test]
fn terminal_binding_cast_is_version_specific_and_container_cycles_fail() {
    for document in documents(&format!(
        r#"{TYPES}<EntityContainer Name="C"><EntitySet Name="Items" EntityType="D.Derived"><NavigationPropertyBinding Path="Related/D.Derived" Target="Items"/></EntitySet></EntityContainer>"#
    )) {
        let mut edmx = document.edmx.unwrap();
        validate_document(&Resolver::resolve_document(edmx.clone()).unwrap()).unwrap();
        edmx.version = Some("4.0".into());
        assert!(validate_document(&Resolver::resolve_document(edmx).unwrap()).is_err());
    }
    for document in documents(
        r#"<EntityContainer Name="A" Extends="D.B"/><EntityContainer Name="B" Extends="D.A"/>"#,
    ) {
        assert!(Resolver::resolve_document(document.edmx.unwrap()).is_err());
    }
}
