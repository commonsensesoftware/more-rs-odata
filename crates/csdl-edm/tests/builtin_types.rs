use csdl_edm::{parser, resolver::Resolver, validator::validate_document};

fn parsed(body: &str) -> csdl_edm::CsdlDocument {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    parser::from_xml_reader(xml.as_bytes()).unwrap()
}

fn check(body: &str, valid: bool) {
    let document = parsed(body);
    let json = document.to_json_value().to_string();
    for document in [document, parser::from_json_reader(json.as_bytes()).unwrap()] {
        let resolved = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let result = validate_document(&resolved);
        assert_eq!(result.is_ok(), valid, "{body}: {result:?}");
    }
}

#[test]
fn abstract_types_are_resolved_and_can_be_narrowed() {
    check(
        r#"<EntityType Name="Item"><NavigationProperty Name="Children" Type="Collection(Edm.EntityType)" ContainsTarget="true"/></EntityType>"#,
        true,
    );
    check(
        r#"<EntityType Name="Base"><Property Name="Value" Type="Edm.ComplexType"/><NavigationProperty Name="Target" Type="Edm.EntityType"/></EntityType><EntityType Name="Derived" BaseType="Demo.Base"><Property Name="Value" Type="Demo.Value"/><NavigationProperty Name="Target" Type="Demo.Base"/></EntityType><ComplexType Name="Value"/><Term Name="Anything" Type="Edm.Untyped"/><TypeDefinition Name="Scalar" UnderlyingType="Edm.PrimitiveType"/>"#,
        true,
    );
    for ty in ["Edm.PrimitiveType", "Demo.Scalar", "Edm.Stream"] {
        check(
            &format!(
                r#"<TypeDefinition Name="Scalar" UnderlyingType="Edm.PrimitiveType"/><ComplexType Name="Value"><Property Name="Data" Type="Collection({ty})"/></ComplexType>"#
            ),
            false,
        );
    }
}

#[test]
fn vocabulary_paths_are_confined_to_vocabulary_values() {
    for ty in [
        "AnnotationPath",
        "PropertyPath",
        "NavigationPropertyPath",
        "AnyPropertyPath",
        "ModelElementPath",
    ] {
        let vocabulary = format!(
            r#"<ComplexType Name="Value"><Property Name="Path" Type="Edm.{ty}"/></ComplexType><Term Name="Paths" Type="Collection(Demo.Value)"/>"#
        );
        check(&vocabulary, true);
        check(
            &format!(
                r#"{vocabulary}<EntityType Name="Item"><Property Name="Value" Type="Demo.Value"/></EntityType>"#
            ),
            false,
        );
        check(
            &format!(r#"<Function Name="F"><ReturnType Type="Edm.{ty}"/></Function>"#),
            false,
        );
    }
}

#[test]
fn rejects_abstract_container_targets_and_declared_bases() {
    for body in [
        r#"<EntityContainer Name="C"><EntitySet Name="Items" EntityType="Edm.EntityType"/></EntityContainer>"#,
        r#"<EntityContainer Name="C"><Singleton Name="Item" Type="Edm.EntityType"/></EntityContainer>"#,
        r#"<EntityType Name="Item" BaseType="Edm.EntityType"/>"#,
        r#"<ComplexType Name="Item" BaseType="Edm.ComplexType"/>"#,
        r#"<TypeDefinition Name="Item" UnderlyingType="Edm.Untyped"/>"#,
    ] {
        assert!(
            Resolver::resolve_document(parsed(body).edmx.unwrap()).is_err(),
            "{body}"
        );
    }
}

#[test]
fn stream_parameters_and_legacy_abstract_types_are_restricted() {
    check(
        r#"<Action Name="Upload" IsBound="true"><Parameter Name="content" Type="Edm.Stream"/></Action>"#,
        true,
    );
    check(
        r#"<Action Name="Upload"><Parameter Name="content" Type="Edm.Stream"/></Action>"#,
        false,
    );
    for body in [
        r#"<Term Name="Value" Type="Edm.Untyped"/>"#,
        r#"<TypeDefinition Name="Value" UnderlyingType="Edm.PrimitiveType"/>"#,
    ] {
        let mut resolved = Resolver::resolve_document(parsed(body).edmx.unwrap()).unwrap();
        resolved.version = "4.0".into();
        assert!(validate_document(&resolved).is_err());
    }
}
