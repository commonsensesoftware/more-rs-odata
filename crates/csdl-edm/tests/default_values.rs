use csdl_edm::{parser, resolver::Resolver, validator::validate_document};

fn documents(body: &str) -> Vec<csdl_edm::CsdlDocument> {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo" Alias="D">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let json = document.to_json_value().to_string();
    vec![document, parser::from_json_reader(json.as_bytes()).unwrap()]
}

#[test]
fn json_numeric_boolean_and_defined_defaults_survive_roundtrip() {
    let json = r#"{"$Version":"4.01","Demo":{"$Alias":"D","Count":{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.Int32"},"Value":{"$Kind":"ComplexType","Enabled":{"$Type":"Edm.Boolean","$DefaultValue":false},"Count":{"$Type":"D.Count","$DefaultValue":42},"Huge":{"$Type":"Edm.Decimal","$Scale":"variable","$DefaultValue":"123456789012345678901234567890.123"}}}}"#;
    let document = parser::from_json_reader(json.as_bytes()).unwrap();
    let output = document.to_json_value();
    assert_eq!(output["Demo"]["Value"]["Enabled"]["$DefaultValue"], false);
    assert_eq!(output["Demo"]["Value"]["Count"]["$DefaultValue"], 42);
    assert_eq!(
        output["Demo"]["Value"]["Huge"]["$DefaultValue"],
        "123456789012345678901234567890.123"
    );
    let mut xml = Vec::new();
    document
        .write_as(&mut xml, csdl_edm::CsdlFormat::Xml)
        .unwrap();
    let parsed = parser::from_xml_reader(xml.as_slice()).unwrap();
    assert_eq!(parsed.to_json_value(), output);
    validate_document(&Resolver::resolve_document(parsed.edmx.unwrap()).unwrap()).unwrap();
}

#[test]
fn defaults_must_match_scalar_type_and_facets() {
    for (ty, value, facet, valid) in [
        ("Boolean", "yes", "", false),
        ("Byte", "256", "", false),
        ("Int16", "-32768", "", true),
        ("Int32", "2147483648", "", false),
        ("String", "abc", r#"MaxLength="2""#, false),
        ("String", "é", r#"Unicode="false""#, false),
        ("Guid", "bad", "", false),
        ("Guid", "01234567-89ab-cdef-0123-456789abcdef", "", true),
        ("Decimal", "12.34", r#"Precision="4" Scale="2""#, true),
        ("Decimal", "12.34", r#"Precision="3" Scale="2""#, false),
        ("Decimal", "1.25", "", false),
        ("Decimal", "1.25e5", r#"Scale="floating""#, true),
        ("Date", "2024-02-29", "", true),
        ("Date", "2025-02-29", "", false),
        ("TimeOfDay", "12:30:00.123", r#"Precision="3""#, true),
        ("TimeOfDay", "24:30:00", "", false),
        ("DateTimeOffset", "2024-02-29T12:30:00+14:00", "", true),
        ("DateTimeOffset", "2024-02-29T12:30:00+14:01", "", false),
        ("Duration", "-P1DT2H3M4.5S", r#"Precision="1""#, true),
        ("Duration", "P", "", false),
    ] {
        for document in documents(&format!(
            r#"<ComplexType Name="Value"><Property Name="Data" Type="Edm.{ty}" DefaultValue="{value}" {facet}/></ComplexType>"#
        )) {
            let result =
                validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap());
            assert_eq!(result.is_ok(), valid, "{ty} {value}: {result:?}");
        }
    }
}

#[test]
fn enum_and_defined_defaults_use_resolved_types() {
    for (value, valid) in [("Read,Write", true), ("Missing", false)] {
        for document in documents(&format!(
            r#"<EnumType Name="Access" IsFlags="true"><Member Name="Read" Value="1"/><Member Name="Write" Value="2"/></EnumType><Term Name="Allowed" Type="D.Access" DefaultValue="{value}"/>"#
        )) {
            let result =
                validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap());
            assert_eq!(result.is_ok(), valid, "{result:?}");
        }
    }
    for document in documents(
        r#"<TypeDefinition Name="Short" UnderlyingType="Edm.String" MaxLength="2"/><Term Name="Name" Type="D.Short" DefaultValue="long"/>"#,
    ) {
        assert!(
            validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap())
                .is_err()
        );
    }
}
