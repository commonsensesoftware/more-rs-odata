#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{parser, resolver::Resolver, validator::validate_document};

#[test]
fn malformed_declaration_flags_are_not_silently_false() {
    for (kind, flag) in [
        ("EntityType", "Abstract"),
        ("EntityType", "OpenType"),
        ("EntityType", "HasStream"),
        ("ComplexType", "Abstract"),
        ("ComplexType", "OpenType"),
    ] {
        let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo"><{kind} Name="Item" {flag}="invalid"/></Schema></edmx:DataServices></edmx:Edmx>"#);
        assert!(parser::from_xml_reader(xml.as_bytes()).is_err(), "{xml}");
        let json = format!(
            r#"{{"$Version":"4.01","Demo":{{"Item":{{"$Kind":"{kind}","${flag}":"invalid"}}}}}}"#
        );
        assert!(parser::from_json_reader(json.as_bytes()).is_err(), "{json}");
    }
}

#[test]
fn container_names_share_the_schema_child_namespace() {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo"><ComplexType Name="Duplicate"/><EntityContainer Name="Duplicate"/></Schema></edmx:DataServices></edmx:Edmx>"#;
    let parsed = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let resolved = Resolver::resolve_document(parsed.edmx.unwrap()).unwrap();
    assert!(validate_document(&resolved).is_err());
}
