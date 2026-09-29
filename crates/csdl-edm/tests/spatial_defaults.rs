#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{CsdlFormat, parser, resolver::Resolver, validator::validate_document};

fn xml_document(ty: &str, value: &str, facet: &str) -> csdl_edm::CsdlDocument {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo"><ComplexType Name="Value"><Property Name="Position" Type="Edm.{ty}" DefaultValue="{value}" {facet}/></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    parser::from_xml_reader(xml.as_bytes()).unwrap()
}

#[test]
fn spatial_families_roundtrip_between_wkt_and_geojson() {
    for (ty, wkt) in [
        ("GeographyPoint", "Point(1 2)"),
        ("GeometryLineString", "LineString(1 2,3 4)"),
        ("GeographyPolygon", "Polygon((0 0,1 0,1 1,0 0))"),
        ("GeometryMultiPoint", "MultiPoint((1 2),(3 4))"),
        (
            "GeographyMultiLineString",
            "MultiLineString((1 2,3 4),(5 6,7 8))",
        ),
        ("GeometryMultiPolygon", "MultiPolygon(((0 0,1 0,1 1,0 0)))"),
        (
            "GeographyCollection",
            "GeometryCollection(Point(1 2),LineString(1 2,3 4))",
        ),
    ] {
        let srid = if ty.starts_with("Geography") { 4326 } else { 0 };
        let original = xml_document(ty, &format!("SRID={srid};{wkt}"), "");
        let json = original.to_json_value();
        assert!(json["Demo"]["Value"]["Position"]["$DefaultValue"].is_object());
        let from_json = parser::from_json_reader(json.to_string().as_bytes()).unwrap();
        let mut xml = Vec::new();
        from_json.write_as(&mut xml, CsdlFormat::Xml).unwrap();
        let roundtrip = parser::from_xml_reader(xml.as_slice()).unwrap();
        assert_eq!(roundtrip.to_json_value(), json, "{ty}");
        for document in [original, from_json, roundtrip] {
            validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap())
                .unwrap();
        }
    }
}

#[test]
fn validates_spatial_shape_srid_and_coordinates() {
    for (ty, value, facet) in [
        ("GeographyPoint", "SRID=0;Point(1 2)", ""),
        ("GeometryPoint", "SRID=0;LineString(1 2,3 4)", ""),
        ("GeometryPoint", "SRID=0;Point(1)", ""),
        ("GeometryPolygon", "SRID=0;Polygon((0 0,1 0,1 1,2 2))", ""),
        ("GeometryPoint", "SRID=3857;Point(1 2)", r#"SRID="0""#),
    ] {
        let document = xml_document(ty, value, facet);
        assert!(
            validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap())
                .is_err(),
            "{value}"
        );
    }
    let document = xml_document(
        "GeometryPoint",
        "SRID=3857;Point(1 2)",
        r#"SRID="variable""#,
    );
    let json = document.to_json_value();
    assert_eq!(
        json["Demo"]["Value"]["Position"]["$DefaultValue"]["crs"]["properties"]["name"],
        "EPSG:3857"
    );
    validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap()).unwrap();
}

#[test]
fn type_definition_srid_controls_defaults_without_explicit_geojson_crs() {
    let json = r#"{"$Version":"4.01","Demo":{"$Alias":"D","MapPoint":{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.GeometryPoint","$SRID":3857},"Value":{"$Kind":"ComplexType","Position":{"$Type":"D.MapPoint","$DefaultValue":{"type":"Point","coordinates":[1,2]}}}}}"#;
    let document = parser::from_json_reader(json.as_bytes()).unwrap();
    let expected = document.to_json_value();
    let mut xml = Vec::new();
    document.write_as(&mut xml, CsdlFormat::Xml).unwrap();
    assert!(String::from_utf8_lossy(&xml).contains("SRID=3857;Point(1 2)"));
    let parsed = parser::from_xml_reader(xml.as_slice()).unwrap();
    assert_eq!(parsed.to_json_value(), expected);
    validate_document(&Resolver::resolve_document(parsed.edmx.unwrap()).unwrap()).unwrap();
}
