use csdl_edm::edm::SchemaElement;
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{ValidationError, validate_document};
use csdl_edm::{CsdlDocument, CsdlFormat, parser};
use std::sync::Arc;

fn documents(body: &str) -> Vec<CsdlDocument> {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Demo" Alias="D">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let mut json = Vec::new();
    document.write_as(&mut json, CsdlFormat::Json).unwrap();
    vec![document, parser::from_json_reader(json.as_slice()).unwrap()]
}

#[test]
fn preserves_base_identity_and_inherited_members_in_both_formats() {
    for document in documents(
        r#"
        <EntityType Name="Leaf" BaseType="D.Middle" OpenType="true" HasStream="true"/>
        <EntityType Name="Middle" BaseType="D.Root" OpenType="true" HasStream="true"/>
        <EntityType Name="Root" Abstract="true" OpenType="true" HasStream="true">
          <Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/>
        </EntityType>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let entities = model.schemas[0]
            .elements
            .iter()
            .map(|e| match e.as_ref() {
                SchemaElement::EntityType(e) => e,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert!(Arc::ptr_eq(
            &entities[0]
                .base_type
                .get()
                .unwrap()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap(),
            entities[1]
        ));
        assert!(Arc::ptr_eq(
            &entities[0].properties()[0],
            &entities[2].properties()[0]
        ));
        assert_eq!(entities[0].keys().len(), 1);
    }
}

#[test]
fn validates_inheritance_flags_in_both_formats() {
    for (base, derived, kind) in [
        ("", "Abstract=\"true\"", "EntityType"),
        ("OpenType=\"true\"", "", "EntityType"),
        ("HasStream=\"true\"", "", "EntityType"),
        ("", "HasStream=\"true\"", "EntityType"),
        ("OpenType=\"true\"", "", "ComplexType"),
    ] {
        for document in documents(&format!(
            r#"<{kind} Name="Base" {base}/><{kind} Name="Derived" BaseType="D.Base" {derived}/>"#
        )) {
            let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
            let errors = validate_document(&model).unwrap_err();
            assert!(errors.iter().any(|e| matches!(e, ValidationError::InvalidInheritance { type_name, .. } if type_name == "Derived")));
        }
    }
}

#[test]
fn narrows_complex_properties_and_navigation_targets() {
    for document in documents(
        r#"
        <ComplexType Name="Address"/>
        <ComplexType Name="PostalAddress" BaseType="D.Address"/>
        <EntityType Name="Target"/>
        <EntityType Name="SpecificTarget" BaseType="D.Target"/>
        <EntityType Name="Base">
          <Property Name="Address" Type="D.Address"/>
          <NavigationProperty Name="Target" Type="D.Target"/>
        </EntityType>
        <EntityType Name="Derived" BaseType="D.Base">
          <Property Name="Address" Type="D.PostalAddress"/>
          <NavigationProperty Name="Target" Type="D.SpecificTarget"/>
        </EntityType>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let SchemaElement::EntityType(derived) = model.schemas[0].elements.last().unwrap().as_ref()
        else {
            panic!()
        };
        assert_eq!(derived.properties().len(), 1);
        assert_eq!(derived.navigation_properties().len(), 1);
        assert_eq!(
            derived.navigation_properties()[0]
                .target
                .upgrade()
                .unwrap()
                .name,
            "SpecificTarget"
        );
    }
}

#[test]
fn rejects_cycles_wrong_base_kinds_and_inherited_empty_key_redeclarations() {
    for body in [
        r#"<EntityType Name="A" BaseType="D.B"/><EntityType Name="B" BaseType="D.A"/>"#,
        r#"<ComplexType Name="A" BaseType="D.B"/><ComplexType Name="B" BaseType="D.A"/>"#,
        r#"<EntityType Name="A" BaseType="D.B"/><ComplexType Name="B"/>"#,
        r#"<ComplexType Name="A" BaseType="D.B"/><EntityType Name="B"/>"#,
        r#"<EntityType Name="A"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType><EntityType Name="B" BaseType="D.A"><Key/></EntityType>"#,
    ] {
        for document in documents(body) {
            assert!(Resolver::resolve_document(document.edmx.unwrap()).is_err());
        }
    }
}

#[test]
fn rejects_unrelated_or_multiplicity_changing_redefinitions() {
    for (base_property, derived_property) in [
        (
            r#"<Property Name="P" Type="D.C"/>"#,
            r#"<Property Name="P" Type="D.Other"/>"#,
        ),
        (
            r#"<Property Name="P" Type="D.C"/>"#,
            r#"<Property Name="P" Type="Collection(D.Sub)"/>"#,
        ),
        (
            r#"<NavigationProperty Name="P" Type="D.Target"/>"#,
            r#"<NavigationProperty Name="P" Type="D.OtherTarget"/>"#,
        ),
        (
            r#"<NavigationProperty Name="P" Type="D.Target"/>"#,
            r#"<Property Name="P" Type="D.Sub"/>"#,
        ),
    ] {
        for document in documents(&format!(
            r#"
            <ComplexType Name="C"/><ComplexType Name="Sub" BaseType="D.C"/><ComplexType Name="Other"/>
            <EntityType Name="Target"/><EntityType Name="OtherTarget"/>
            <ComplexType Name="Base">{base_property}</ComplexType>
            <ComplexType Name="Derived" BaseType="D.Base">{derived_property}</ComplexType>"#
        )) {
            assert!(Resolver::resolve_document(document.edmx.unwrap()).is_err());
        }
    }
}

#[test]
fn allows_defining_key_below_keyless_base_and_abstract_complex_below_concrete() {
    for document in documents(
        r#"
        <EntityType Name="Base"/>
        <EntityType Name="Derived" BaseType="D.Base"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType>
        <ComplexType Name="Concrete"/>
        <ComplexType Name="Abstract" Abstract="true" BaseType="D.Concrete"/>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
    }
}

#[test]
fn legacy_documents_reject_narrowing_and_concrete_keyless_entities() {
    for mut document in documents(
        r#"
        <ComplexType Name="BaseValue"/>
        <ComplexType Name="SubValue" BaseType="D.BaseValue"/>
        <ComplexType Name="Base"><Property Name="P" Type="D.BaseValue"/></ComplexType>
        <ComplexType Name="Derived" BaseType="D.Base"><Property Name="P" Type="D.SubValue"/></ComplexType>"#,
    ) {
        document.edmx.as_mut().unwrap().version = Some("4.0".into());
        assert!(Resolver::resolve_document(document.edmx.unwrap()).is_err());
    }
    for mut document in documents(r#"<EntityType Name="Keyless"/>"#) {
        document.edmx.as_mut().unwrap().version = Some("4.0".into());
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        assert!(validate_document(&model).unwrap_err().iter().any(
            |e| matches!(e, ValidationError::MissingEntityKey { entity } if entity == "Keyless")
        ));
    }
}

#[test]
fn resolves_cross_schema_bases_with_forward_aliases_and_shared_identity() {
    let mut document = documents(r#"
        <EntityType Name="Item" BaseType="B.Item"/>
        <ComplexType Name="Value" BaseType="B.Value"/>
        <EntityContainer Name="Container"><EntitySet Name="Items" EntityType="D.Item"/></EntityContainer>"#).remove(0);
    let mut base = documents(r#"
        <EntityType Name="Item"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/>
          <NavigationProperty Name="Derived" Type="D.Item"/>
        </EntityType>
        <ComplexType Name="Value"><Property Name="Text" Type="Edm.String"/></ComplexType>"#).remove(0).edmx.unwrap().schemas.remove(0);
    base.namespace = "Base".into();
    base.alias = Some("B".into());
    document.edmx.as_mut().unwrap().schemas.push(base);
    for reverse in [false, true] {
        let mut document = document.clone();
        if reverse {
            document.edmx.as_mut().unwrap().schemas.reverse();
        }
        let json = document.to_json_value().to_string();
        for document in [document, parser::from_json_reader(json.as_bytes()).unwrap()] {
            let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
            validate_document(&model).unwrap();
            let derived_schema = model
                .schemas
                .iter()
                .find(|s| s.namespace == "Demo")
                .unwrap();
            let base_schema = model
                .schemas
                .iter()
                .find(|s| s.namespace == "Base")
                .unwrap();
            let SchemaElement::EntityType(derived) = derived_schema.elements[0].as_ref() else {
                panic!()
            };
            let SchemaElement::EntityType(base) = base_schema.elements[0].as_ref() else {
                panic!()
            };
            assert_eq!(derived.name, "Item");
            assert!(Arc::ptr_eq(
                &derived
                    .base_type
                    .get()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .upgrade()
                    .unwrap(),
                base
            ));
            assert!(Arc::ptr_eq(
                &base.navigation_properties()[0].target.upgrade().unwrap(),
                derived
            ));
            assert!(Arc::ptr_eq(&derived.properties()[0], &base.properties()[0]));
            assert_eq!(derived.keys().len(), 1);
            let SchemaElement::ComplexType(derived) = derived_schema.elements[1].as_ref() else {
                panic!()
            };
            let SchemaElement::ComplexType(base) = base_schema.elements[1].as_ref() else {
                panic!()
            };
            assert!(Arc::ptr_eq(
                &derived
                    .base_type
                    .get()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .upgrade()
                    .unwrap(),
                base
            ));
        }
    }
}

#[test]
fn rejects_cross_schema_inheritance_cycles_and_conflicting_aliases() {
    let mut document = documents(r#"<EntityType Name="Item" BaseType="B.Item"/>"#).remove(0);
    let mut base = documents(r#"<EntityType Name="Item" BaseType="D.Item"/>"#)
        .remove(0)
        .edmx
        .unwrap()
        .schemas
        .remove(0);
    base.namespace = "Base".into();
    base.alias = Some("B".into());
    document.edmx.as_mut().unwrap().schemas.push(base);
    assert!(Resolver::resolve_document(document.edmx.clone().unwrap()).is_err());
    document.edmx.as_mut().unwrap().schemas[1].alias = Some("D".into());
    assert!(Resolver::resolve_document(document.edmx.unwrap()).is_err());
}

#[test]
fn cross_schema_keys_and_terms_use_qualified_identity() {
    let mut document = documents(r#"
        <EntityType Name="Item"><Key><PropertyRef Name="Value/ID"/></Key><Property Name="Value" Type="B.Value" Nullable="false"/></EntityType>
        <Term Name="Label" Type="Edm.String" BaseTerm="B.Label"/>"#).remove(0);
    let mut base = documents(r#"
        <ComplexType Name="Value"><Property Name="ID" Type="Edm.Int32" Nullable="false"/></ComplexType>
        <Term Name="Label" Type="Edm.String"/>"#).remove(0).edmx.unwrap().schemas.remove(0);
    base.namespace = "Base".into();
    base.alias = Some("B".into());
    document.edmx.as_mut().unwrap().schemas.push(base);
    let json = document.to_json_value().to_string();
    for document in [document, parser::from_json_reader(json.as_bytes()).unwrap()] {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
    }
}
