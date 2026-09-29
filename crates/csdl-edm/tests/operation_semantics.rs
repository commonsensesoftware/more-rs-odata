#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::edm::{EntitySetPathSegment, SchemaElement};
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{ValidationError, validate_document};
use csdl_edm::{CsdlDocument, parser};

fn documents(body: &str) -> Vec<CsdlDocument> {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo" Alias="D">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let json = document.to_json_value().to_string();
    vec![document, parser::from_json_reader(json.as_bytes()).unwrap()]
}

fn invalid(body: &str, reason: &str) {
    for document in documents(body) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let errors = validate_document(&model).unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, ValidationError::InvalidOperation { reason: actual, .. } if *actual == reason)), "{errors:?}");
    }
}

#[test]
fn accepts_valid_function_and_action_overloads() {
    for document in documents(
        r#"
        <Function Name="Find"><Parameter Name="code" Type="Edm.Int32"/><ReturnType Type="Edm.String"/></Function>
        <Function Name="Find"><Parameter Name="label" Type="Edm.String"/><ReturnType Type="Edm.String"/></Function>
        <Action Name="Run"/>
        <Action Name="Run" IsBound="true"><Parameter Name="binding" Type="Edm.Int32"/></Action>
        <Action Name="Run" IsBound="true"><Parameter Name="binding" Type="Collection(Edm.Int32)"/></Action>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        assert_eq!(model.schemas[0].elements.len(), 5);
    }
}

#[test]
fn rejects_duplicate_overload_name_sets_or_ordered_types() {
    for second in [
        r#"<Parameter Name="code" Type="Edm.String"/>"#,
        r#"<Parameter Name="other" Type="Edm.Int32"/>"#,
    ] {
        invalid(
            &format!(
                r#"
            <Function Name="Find"><Parameter Name="code" Type="Edm.Int32"/><ReturnType Type="Edm.String"/></Function>
            <Function Name="Find">{second}<ReturnType Type="Edm.String"/></Function>"#
            ),
            "Function overloads need distinct parameter-name sets and ordered type signatures",
        );
    }
    invalid(
        r#"<Action Name="Run"/><Action Name="Run"><Parameter Name="argument" Type="Edm.String"/></Action>"#,
        "Action overloads must have distinct binding types; unbound actions cannot overload",
    );
}

#[test]
fn rejects_inconsistent_returns_and_duplicate_parameters() {
    invalid(
        r#"
        <Function Name="Find"><Parameter Name="code" Type="Edm.Int32"/><ReturnType Type="Edm.String"/></Function>
        <Function Name="Find"><Parameter Name="label" Type="Edm.String"/><ReturnType Type="Edm.Boolean"/></Function>"#,
        "Function overloads with the same binding type must have the same return type",
    );
    invalid(
        r#"<Action Name="Run"><Parameter Name="p" Type="Edm.String"/><Parameter Name="p" Type="Edm.Int32"/></Action>"#,
        "Parameter names must be non-empty and unique",
    );
}

#[test]
fn entity_set_path_resolves_derived_cast_and_checks_return_compatibility() {
    let types = r#"<EntityType Name="Base"/><EntityType Name="Derived" BaseType="D.Base"/><EntityType Name="Unrelated"/>"#;
    for document in documents(&format!(
        r#"{types}<Function Name="Narrow" IsBound="true" IsComposable="true" EntitySetPath="binding/D.Derived"><Parameter Name="binding" Type="D.Base"/><ReturnType Type="D.Derived"/></Function>"#
    )) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let SchemaElement::Function(function) = model.schemas[0].elements.last().unwrap().as_ref()
        else {
            panic!()
        };
        assert!(matches!(
            &function.entity_set_path.as_ref().unwrap()[1],
            EntitySetPathSegment::EntityTypeCast(_)
        ));
    }
    invalid(
        &format!(
            r#"{types}<Function Name="Wrong" IsBound="true" EntitySetPath="binding"><Parameter Name="binding" Type="D.Base"/><ReturnType Type="D.Unrelated"/></Function>"#
        ),
        "EntitySetPath must end at a compatible entity type",
    );
    invalid(
        &format!(
            r#"{types}<Function Name="Wrong" IsBound="true" EntitySetPath="binding"><Parameter Name="binding" Type="D.Base"/><ReturnType Type="Edm.String"/></Function>"#
        ),
        "EntitySetPath requires an entity-valued return type",
    );
}

#[test]
fn json_defaults_operation_types_and_nullability() {
    let json = r#"{"$Version":"4.01","Demo":{"Echo":[{"$Kind":"Function","$Parameter":[{"$Name":"text"}],"$ReturnType":{}}]}}"#;
    let document = parser::from_json_reader(json.as_bytes()).unwrap();
    let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
    validate_document(&model).unwrap();
    let SchemaElement::Function(function) = model.schemas[0].elements[0].as_ref() else {
        panic!()
    };
    assert_eq!(function.parameters[0].nullable, Some(false));
    assert_eq!(function.return_type.as_ref().unwrap().nullable, Some(false));
}

#[test]
fn rejects_collection_entity_return_nullability_and_scalar_path_traversal() {
    invalid(
        r#"<EntityType Name="Item"/><Function Name="Items"><ReturnType Type="Collection(D.Item)" Nullable="false"/></Function>"#,
        "Collection-of-entity return type must not specify Nullable",
    );
    for path in ["binding/Text", "binding//", "binding/D.Other"] {
        for document in documents(&format!(
            r#"
            <EntityType Name="Item"><Property Name="Text" Type="Edm.String"/></EntityType><EntityType Name="Other"/>
            <Function Name="Find" IsBound="true" EntitySetPath="{path}"><Parameter Name="binding" Type="D.Item"/><ReturnType Type="D.Item"/></Function>"#
        )) {
            let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
            assert!(
                validate_document(&model)
                    .unwrap_err()
                    .iter()
                    .any(|e| matches!(e, ValidationError::InvalidEntitySetPath { .. }))
            );
        }
    }
}

#[test]
fn rejects_malformed_operation_declarations() {
    for members in [
        r#""$Parameter":{}"#,
        r#""$ReturnType":[]"#,
        r#""$IsBound":"true"#,
        r#""$Parameter":[{"$Type":1,"$Name":"p"}]"#,
    ] {
        let json =
            format!(r#"{{"$Version":"4.01","Demo":{{"Run":[{{"$Kind":"Action",{members}}}]}}}}"#);
        assert!(parser::from_json_reader(json.as_bytes()).is_err());
    }
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx"><edmx:DataServices><Schema Namespace="Demo"><Function Name="Invalid"><ReturnType Type="Edm.String"/><ReturnType Type="Edm.String"/></Function></Schema></edmx:DataServices></edmx:Edmx>"#;
    assert!(parser::from_xml_reader(xml.as_bytes()).is_err());
}
