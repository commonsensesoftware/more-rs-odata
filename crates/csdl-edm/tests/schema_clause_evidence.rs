use csdl_edm::{conformance::ValidationProfile as P, parser};

fn xml(schemas: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices xmlns="http://docs.oasis-open.org/odata/ns/edm">{schemas}</edmx:DataServices></edmx:Edmx>"#
    )
}

fn check(xml_schemas: &str, json_schemas: &str, valid: bool) {
    let xml = parser::from_xml_reader_with_profile(xml(xml_schemas).as_bytes(), P::Strict);
    let json = parser::from_json_reader_with_profile(
        format!(r#"{{"$Version":"4.01",{json_schemas}}}"#).as_bytes(),
        P::Strict,
    );
    assert_eq!(xml.is_ok(), valid, "{xml_schemas}: {xml:?}");
    assert_eq!(json.is_ok(), valid, "{json_schemas}: {json:?}");
}

#[test]
fn namespaces_are_unique_and_reserved_names_are_rejected() {
    check(
        r#"<Schema Namespace="A"/><Schema Namespace="B"/>"#,
        r#""A":{},"B":{}"#,
        true,
    );
    check(
        r#"<Schema Namespace="A"/><Schema Namespace="A"/>"#,
        r#""A":{},"A":{}"#,
        false,
    );
    for name in [
        "Edm",
        "odata",
        "System",
        "Transient",
        "1Invalid",
        "Invalid..Name",
    ] {
        check(
            &format!(r#"<Schema Namespace="{name}"/>"#),
            &format!(r#""{name}":{{}}"#),
            false,
        );
    }
    check(
        r#"<Schema Namespace="Company.Model"/>"#,
        r#""Company.Model":{}"#,
        true,
    );
}

#[test]
fn aliases_are_simple_identifiers_with_document_global_scope() {
    for (alias, valid) in [
        ("Short", true),
        ("With.Dot", false),
        ("1Invalid", false),
        ("", false),
    ] {
        check(
            &format!(r#"<Schema Namespace="N" Alias="{alias}"/>"#),
            &format!(r#""N":{{"$Alias":"{alias}"}}"#),
            valid,
        );
    }
    check(
        r#"<Schema Namespace="A" Alias="X"/><Schema Namespace="B" Alias="X"/>"#,
        r#""A":{"$Alias":"X"},"B":{"$Alias":"X"}"#,
        false,
    );
    check(
        r#"<Schema Namespace="A" Alias="B"/><Schema Namespace="B"/>"#,
        r#""A":{"$Alias":"B"},"B":{}"#,
        false,
    );
    check(
        r#"<Schema Namespace="A" Alias="X"/><Schema Namespace="B"><ComplexType Name="C"><Property Name="P" Type="X.D"/></ComplexType></Schema><Schema Namespace="C"/>"#,
        r#""A":{"$Alias":"X"},"B":{"C":{"$Kind":"ComplexType","P":{"$Type":"X.D"}}},"C":{}"#,
        true,
    );
}

#[test]
fn type_names_share_the_schema_uniqueness_scope() {
    check(
        r#"<Schema Namespace="N"><ComplexType Name="A"/><EntityType Name="B"/></Schema>"#,
        r#""N":{"A":{"$Kind":"ComplexType"},"B":{"$Kind":"EntityType"}}"#,
        true,
    );
    check(
        r#"<Schema Namespace="N"><ComplexType Name="A"/><EntityType Name="A"/></Schema>"#,
        r#""N":{"A":{"$Kind":"ComplexType"},"A":{"$Kind":"EntityType"}}"#,
        false,
    );
    check(
        r#"<Schema Namespace="N"><ComplexType Name="A"/><ComplexType Name="a"/></Schema>"#,
        r#""N":{"A":{"$Kind":"ComplexType"},"a":{"$Kind":"ComplexType"}}"#,
        true,
    );
}

#[test]
fn xml_schema_and_external_annotation_groups_require_their_members() {
    for (schema, valid) in [
        (r#"<Schema Namespace="N"/>"#, true),
        (r#"<Schema/>"#, false),
        (
            r#"<Schema Namespace="N"><Annotations Target="N.C"><Annotation Term="N.T" String="text"/></Annotations></Schema>"#,
            true,
        ),
        (
            r#"<Schema Namespace="N"><Annotations Target="N.C" Qualifier="Phone"><Annotation Term="N.T" String="text"/></Annotations></Schema>"#,
            true,
        ),
        (
            r#"<Schema Namespace="N"><Annotations><Annotation Term="N.T" String="text"/></Annotations></Schema>"#,
            false,
        ),
        (
            r#"<Schema Namespace="N"><Annotations Target="N.C"/></Schema>"#,
            false,
        ),
    ] {
        let result = parser::from_xml_reader_with_profile(xml(schema).as_bytes(), P::Strict);
        assert_eq!(result.is_ok(), valid, "{schema}: {result:?}");
    }
}
