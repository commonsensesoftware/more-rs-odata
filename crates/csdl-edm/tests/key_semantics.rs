use csdl_edm::{parser, resolver::Resolver, validator::validate_document};

fn documents(body: &str) -> Vec<csdl_edm::CsdlDocument> {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Demo">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let parsed = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let json = parsed.to_json_value().to_string();
    vec![parsed, parser::from_json_reader(json.as_bytes()).unwrap()]
}

fn check(body: &str, valid: bool) {
    for document in documents(body) {
        let resolved = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let result = validate_document(&resolved);
        assert_eq!(result.is_ok(), valid, "{result:?}");
    }
}

#[test]
fn validates_key_types_nullability_and_aliases_in_both_formats() {
    for ty in [
        "Boolean",
        "Byte",
        "Date",
        "DateTimeOffset",
        "Decimal",
        "Duration",
        "Guid",
        "Int16",
        "Int32",
        "Int64",
        "SByte",
        "String",
        "TimeOfDay",
        "Binary",
        "Single",
        "Double",
    ] {
        check(
            &format!(
                r#"<EntityType Name="Item"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.{ty}" Nullable="false"/></EntityType>"#
            ),
            !matches!(ty, "Binary" | "Single" | "Double"),
        );
    }
    check(
        r#"<EntityType Name="Item"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32"/></EntityType>"#,
        false,
    );
    check(
        r#"<EntityType Name="Item"><Key><PropertyRef Name="ID" Alias="Other"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType>"#,
        false,
    );
    let body = r#"<ComplexType Name="Value"><Property Name="ID" Type="Edm.Int32" Nullable="false"/></ComplexType><EntityType Name="Item"><Key><PropertyRef Name="Value/ID" Alias="ValueID"/></Key><Property Name="Value" Type="Demo.Value" Nullable="false"/></EntityType><EntityType Name="Derived" BaseType="Demo.Item"/>"#;
    check(body, true);
    check(&body.replace(r#" Alias="ValueID""#, ""), false);
    check(
        &body.replace(r#"Alias="ValueID""#, r#"Alias="Value""#),
        false,
    );
    check(
        &body.replace(r#"Alias="ValueID""#, r#"Alias="bad/name""#),
        false,
    );
    check(
        &body.replace(
            r#"Type="Demo.Value" Nullable="false""#,
            r#"Type="Demo.Value" Nullable="true""#,
        ),
        false,
    );
}

#[test]
fn related_entity_requires_its_complete_key_and_401() {
    let body = r#"<EntityType Name="Parent"><Key><PropertyRef Name="A"/><PropertyRef Name="B"/></Key><Property Name="A" Type="Edm.Int32" Nullable="false"/><Property Name="B" Type="Edm.String" Nullable="false"/></EntityType><EntityType Name="Child"><Key><PropertyRef Name="Parent/A" Alias="ParentA"/><PropertyRef Name="Parent/B" Alias="ParentB"/></Key><NavigationProperty Name="Parent" Type="Demo.Parent" Nullable="false"/></EntityType>"#;
    check(body, true);
    check(
        &body.replace(r#"<PropertyRef Name="Parent/B" Alias="ParentB"/>"#, ""),
        false,
    );
    check(
        &body.replace(
            r#"Type="Demo.Parent" Nullable="false""#,
            r#"Type="Demo.Parent" Nullable="true""#,
        ),
        false,
    );
    check(
        &body.replace(r#"Name="Parent/A""#, r#"Name="Parent""#),
        false,
    );
    for document in documents(body) {
        let mut resolved = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        resolved.version = "4.0".into();
        assert!(validate_document(&resolved).is_err());
    }
}

#[test]
fn rejects_empty_keys_and_malformed_json_key_members() {
    for document in documents(r#"<EntityType Name="Item"><Key/></EntityType>"#) {
        assert!(Resolver::resolve_document(document.edmx.unwrap()).is_err());
    }
    for key in [
        r#"{}"#,
        r#"[12]"#,
        r#"[{}]"#,
        r#"[{"A":12}]"#,
        r#"[{"A":"X","B":"Y"}]"#,
    ] {
        let json = format!(
            r#"{{"$Version":"4.01","Demo":{{"Item":{{"$Kind":"EntityType","$Key":{key}}}}}}}"#
        );
        assert!(parser::from_json_reader(json.as_bytes()).is_err(), "{json}");
    }
}
