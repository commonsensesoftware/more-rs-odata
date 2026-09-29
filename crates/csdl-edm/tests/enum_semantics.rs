#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{edm::SchemaElement, parser, resolver::Resolver, validator::validate_document};

fn document(body: &str) -> csdl_edm::CsdlDocument {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    parser::from_xml_reader(xml.as_bytes()).unwrap()
}

#[test]
fn implicit_members_resolve_in_order_and_serialize_as_json_numbers() {
    let document = document(
        r#"<EnumType Name="Color"><Member Name="Red"/><Member Name="Green"/><Member Name="Blue"/></EnumType>"#,
    );
    let json = document.to_json_value();
    assert_eq!(json["Demo"]["Color"]["Blue"], 2);
    let json = parser::from_json_reader(json.to_string().as_bytes()).unwrap();
    for document in [document, json] {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let SchemaElement::EnumType(t) = model.schemas[0].elements[0].as_ref() else {
            panic!()
        };
        assert_eq!(
            t.members
                .iter()
                .map(|m| m.resolved_value)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }
}

#[test]
fn validates_underlying_ranges_flags_and_explicitness() {
    for body in [
        r#"<EnumType Name="E"/>"#,
        r#"<EnumType Name="E" UnderlyingType="Edm.String"><Member Name="A" Value="0"/></EnumType>"#,
        r#"<EnumType Name="E" UnderlyingType="Edm.Byte"><Member Name="A" Value="256"/></EnumType>"#,
        r#"<EnumType Name="E" UnderlyingType="Edm.SByte"><Member Name="A" Value="-129"/></EnumType>"#,
        r#"<EnumType Name="E"><Member Name="A"/><Member Name="B" Value="1"/></EnumType>"#,
        r#"<EnumType Name="E" IsFlags="true"><Member Name="A"/></EnumType>"#,
        r#"<EnumType Name="E" IsFlags="true"><Member Name="A" Value="-1"/></EnumType>"#,
    ] {
        let model = Resolver::resolve_document(document(body).edmx.unwrap()).unwrap();
        assert!(validate_document(&model).is_err(), "{body}");
    }
    let valid = document(
        r#"<EnumType Name="E" IsFlags="true"><Member Name="A" Value="1"/><Member Name="Alias" Value="1"/><Member Name="B" Value="2"/><Member Name="Both" Value="3"/></EnumType>"#,
    );
    validate_document(&Resolver::resolve_document(valid.edmx.unwrap()).unwrap()).unwrap();
}

#[test]
fn malformed_numeric_values_are_not_treated_as_implicit() {
    for value in [
        "null",
        "true",
        "1.5",
        r#""not-a-number""#,
        "9223372036854775808",
    ] {
        let json =
            format!(r#"{{"$Version":"4.01","Demo":{{"E":{{"$Kind":"EnumType","A":{value}}}}}}}"#);
        assert!(
            parser::from_json_reader(json.as_bytes()).is_err(),
            "{value}"
        );
    }
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx"><edmx:DataServices><Schema Namespace="Demo"><EnumType Name="E"><Member Name="A" Value="not-a-number"/></EnumType></Schema></edmx:DataServices></edmx:Edmx>"#;
    assert!(parser::from_xml_reader(xml.as_bytes()).is_err());
}
