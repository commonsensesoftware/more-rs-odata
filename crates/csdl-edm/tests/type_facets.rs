#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{parser, resolver::Resolver, validator::validate_document};

fn check(body: &str, valid: bool) {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let json = document.to_json_value().to_string();
    for document in [document, parser::from_json_reader(json.as_bytes()).unwrap()] {
        let resolved = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let result = validate_document(&resolved);
        assert_eq!(result.is_ok(), valid, "{body}: {result:?}");
    }
}

#[test]
fn resolves_every_spatial_type_and_stream_with_facets() {
    for ty in [
        "Geography",
        "GeographyPoint",
        "GeographyLineString",
        "GeographyPolygon",
        "GeographyMultiPoint",
        "GeographyMultiLineString",
        "GeographyMultiPolygon",
        "GeographyCollection",
        "Geometry",
        "GeometryPoint",
        "GeometryLineString",
        "GeometryPolygon",
        "GeometryMultiPoint",
        "GeometryMultiLineString",
        "GeometryMultiPolygon",
        "GeometryCollection",
    ] {
        check(
            &format!(
                r#"<ComplexType Name="Place"><Property Name="Position" Type="Edm.{ty}" SRID="variable"/></ComplexType>"#
            ),
            true,
        );
    }
    check(
        r#"<ComplexType Name="File"><Property Name="Content" Type="Edm.Stream" MaxLength="1024"/></ComplexType>"#,
        true,
    );
}

#[test]
fn enforces_facet_applicability_and_ranges() {
    for (ty, facet, valid) in [
        ("String", r#"MaxLength="10" Unicode="false""#, true),
        ("String", r#"MaxLength="0""#, false),
        ("Boolean", r#"MaxLength="10""#, false),
        ("Decimal", r#"Precision="4" Scale="2""#, true),
        ("Decimal", r#"Precision="2" Scale="4""#, false),
        ("Decimal", r#"Precision="0""#, false),
        ("Decimal", r#"Scale="floating""#, true),
        ("Decimal", r#"Scale="VARIABLE""#, true),
        ("TimeOfDay", r#"Precision="12""#, true),
        ("Duration", r#"Precision="13""#, false),
        ("Int32", r#"Precision="2""#, false),
        ("Decimal", r#"Unicode="false""#, false),
        ("String", r#"SRID="4326""#, false),
    ] {
        check(
            &format!(
                r#"<ComplexType Name="Value"><Property Name="Data" Type="Edm.{ty}" {facet}/></ComplexType>"#
            ),
            valid,
        );
    }
}

#[test]
fn retains_definition_facets_and_checks_all_use_sites() {
    let prefix = r#"<TypeDefinition Name="Amount" UnderlyingType="Edm.Decimal" Precision="3"/>"#;
    for (usage, valid) in [
        (r#"<Term Name="Value" Type="Demo.Amount" Scale="2"/>"#, true),
        (
            r#"<Term Name="Value" Type="Demo.Amount" Scale="4"/>"#,
            false,
        ),
        (
            r#"<Term Name="Value" Type="Demo.Amount" Precision="3"/>"#,
            false,
        ),
        (
            r#"<Function Name="F"><Parameter Name="x" Type="Edm.Int32" Scale="2"/><ReturnType Type="Edm.String"/></Function>"#,
            false,
        ),
        (
            r#"<Action Name="A"><ReturnType Type="Edm.String" Precision="3"/></Action>"#,
            false,
        ),
    ] {
        check(&format!("{prefix}{usage}"), valid);
    }
}

#[test]
fn rejects_malformed_facets_without_silently_dropping_them() {
    for facet in [
        r#""$MaxLength":true"#,
        r#""$Precision":{}"#,
        r#""$Scale":-1"#,
        r#""$SRID":[]"#,
        r#""$Unicode":"false""#,
        r#""$Nullable":"true""#,
    ] {
        let json = format!(
            r#"{{"$Version":"4.01","Demo":{{"Value":{{"$Kind":"ComplexType","Data":{{"$Type":"Edm.String",{facet}}}}}}}}}"#
        );
        assert!(parser::from_json_reader(json.as_bytes()).is_err(), "{json}");
    }
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo"><ComplexType Name="Value"><Property Name="Data" Type="Edm.String" Nullable="nonsense"/></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#;
    assert!(parser::from_xml_reader(xml.as_bytes()).is_err());
}

#[test]
fn floating_scale_is_versioned_and_unknown_primitives_are_diagnostic() {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0"><edmx:DataServices><Schema Namespace="Demo"><Term Name="Amount" Type="Edm.Decimal" Scale="floating"/></Schema></edmx:DataServices></edmx:Edmx>"#;
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let resolved = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
    assert!(validate_document(&resolved).is_err());
    let xml = xml.replace("Edm.Decimal", "Edm.DoesNotExist");
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    assert!(matches!(Resolver::resolve_document(document.edmx.unwrap()), Err(csdl_edm::resolver::ResolveError::UnknownType(name)) if name == "Edm.DoesNotExist"));
}
