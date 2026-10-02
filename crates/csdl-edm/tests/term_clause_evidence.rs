#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

#[test]
fn term_defaults_validate_primitive_lexical_forms() {
    for (ty, value, valid) in [
        ("Boolean", "true", true),
        ("Boolean", "false", true),
        ("Boolean", "True", false),
        ("Boolean", "1", false),
        ("Byte", "255", true),
        ("Byte", "256", false),
        ("SByte", "-128", true),
        ("SByte", "-129", false),
        ("Int16", "32767", true),
        ("Int16", "32768", false),
        ("Int32", "-2147483648", true),
        ("Int32", "-2147483649", false),
        ("Int64", "9223372036854775807", true),
        ("Int64", "9223372036854775808", false),
        ("Int64", "1.0", false),
        ("Single", "INF", true),
        ("Single", "inf", false),
        ("Double", "-1.25e2", true),
        ("Double", "NaN", true),
        ("Double", "nan", false),
        ("Decimal", "12", true),
        ("Decimal", "abc", false),
        ("Binary", "AA==", true),
        ("Binary", "AB==", false),
        ("Guid", "01234567-89ab-cdef-0123-456789abcdef", true),
        ("Guid", "01234567-89ab-cdef-0123-456789abcdeg", false),
        ("Date", "2024-02-29", true),
        ("Date", "2023-02-29", false),
        ("DateTimeOffset", "2024-02-29T12:00:00Z", true),
        ("DateTimeOffset", "2024-02-29T12:00:00", false),
        ("TimeOfDay", "23:59:59", true),
        ("TimeOfDay", "24:00:00", false),
        ("Duration", "P1DT2H", true),
        ("Duration", "P", false),
    ] {
        check(
            &format!(r#"<Term Name="Value" Type="Edm.{ty}" DefaultValue="{value}"/>"#),
            &format!(r#""Value":{{"$Kind":"Term","$Type":"Edm.{ty}","$DefaultValue":"{value}"}}"#),
            valid,
        );
    }
}

#[test]
fn string_term_defaults_decode_xml_attribute_escapes() {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.String" DefaultValue=" &quot;&amp;&lt;&gt;&apos;&#9;&#10;&#13;日本語 "/></Schema></edmx:DataServices></edmx:Edmx>"#;
    let json = r#"{"$Version":"4.01","N":{"Value":{"$Kind":"Term","$DefaultValue":" \"&<>'\t\n\r日本語 "}}}"#;
    for parsed in [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
    ] {
        let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
        validator::validate_document(&model).unwrap();
        let csdl_edm::edm::SchemaElement::Term(term) = model.schemas[0].elements[0].as_ref() else {
            panic!("expected term");
        };
        assert_eq!(term.default_value.as_deref(), Some(" \"&<>'\t\n\r日本語 "));
    }
}

fn check(xml: &str, json: &str, valid: bool) {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{xml}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(r#"{{"$Version":"4.01","N":{{{json}}}}}"#);
    for parsed in [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
    ] {
        let result = parsed
            .map_err(|e| e.to_string())
            .and_then(|d| Resolver::resolve_document(d.edmx.unwrap()).map_err(|e| format!("{e:?}")))
            .and_then(|m| validator::validate_document(&m).map_err(|e| format!("{e:?}")));
        assert_eq!(result.is_ok(), valid, "{result:?}\n{xml}\n{json}");
    }
}

#[test]
fn spatial_term_defaults_require_matching_shape_and_srid() {
    for family in ["Geography", "Geometry"] {
        let srid = if family == "Geography" { 4326 } else { 0 };
        for (shape, wkt, geojson) in [
            (
                "Point",
                "Point(1 2)",
                r#"{"type":"Point","coordinates":[1,2]}"#,
            ),
            (
                "LineString",
                "LineString(1 2,3 4)",
                r#"{"type":"LineString","coordinates":[[1,2],[3,4]]}"#,
            ),
            (
                "Polygon",
                "Polygon((0 0,1 0,1 1,0 0))",
                r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}"#,
            ),
            (
                "MultiPoint",
                "MultiPoint((1 2),(3 4))",
                r#"{"type":"MultiPoint","coordinates":[[1,2],[3,4]]}"#,
            ),
            (
                "MultiLineString",
                "MultiLineString((1 2,3 4))",
                r#"{"type":"MultiLineString","coordinates":[[[1,2],[3,4]]]}"#,
            ),
            (
                "MultiPolygon",
                "MultiPolygon(((0 0,1 0,1 1,0 0)))",
                r#"{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]]]}"#,
            ),
            (
                "Collection",
                "GeometryCollection(Point(1 2))",
                r#"{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1,2]}]}"#,
            ),
        ] {
            for (target, valid) in [
                (shape, true),
                ("", true),
                (if shape == "Point" { "Polygon" } else { "Point" }, false),
            ] {
                check(
                    &format!(
                        r#"<Term Name="Value" Type="Edm.{family}{target}" DefaultValue="SRID={srid};{wkt}"/>"#
                    ),
                    &format!(
                        r#""Value":{{"$Kind":"Term","$Type":"Edm.{family}{target}","$DefaultValue":{geojson}}}"#
                    ),
                    valid,
                );
            }
        }
        for (facet_xml, facet_json, valid) in [
            ("", "", false),
            (r#" SRID="3857""#, r#", "$SRID":3857"#, true),
            (r#" SRID="variable""#, r#", "$SRID":"variable""#, true),
        ] {
            check(
                &format!(
                    r#"<Term Name="Value" Type="Edm.{family}Point" DefaultValue="SRID=3857;Point(1 2)"{facet_xml}/>"#
                ),
                &format!(
                    r#""Value":{{"$Kind":"Term","$Type":"Edm.{family}Point","$DefaultValue":{{"type":"Point","coordinates":[1,2],"crs":{{"type":"name","properties":{{"name":"EPSG:3857"}}}}}}{facet_json}}}"#
                ),
                valid,
            );
        }
        for (wkt, geojson) in [
            ("Point(1)", r#"{"type":"Point","coordinates":[1]}"#),
            (
                "Polygon((0 0,1 0,1 1,2 2))",
                r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[2,2]]]}"#,
            ),
        ] {
            check(
                &format!(
                    r#"<Term Name="Value" Type="Edm.{family}" DefaultValue="SRID={srid};{wkt}"/>"#
                ),
                &format!(
                    r#""Value":{{"$Kind":"Term","$Type":"Edm.{family}","$DefaultValue":{geojson}}}"#
                ),
                false,
            );
        }
    }
}

#[test]
fn term_defaults_obey_direct_and_type_definition_facets() {
    for (ty, value, xml_facets, json_facets, valid) in [
        ("String", "é", r#"MaxLength="1""#, r#""$MaxLength":1"#, true),
        (
            "String",
            "éa",
            r#"MaxLength="1""#,
            r#""$MaxLength":1"#,
            false,
        ),
        (
            "String",
            "a",
            r#"Unicode="false""#,
            r#""$Unicode":false"#,
            true,
        ),
        (
            "String",
            "é",
            r#"Unicode="false""#,
            r#""$Unicode":false"#,
            false,
        ),
        (
            "Binary",
            "AA==",
            r#"MaxLength="1""#,
            r#""$MaxLength":1"#,
            true,
        ),
        (
            "Binary",
            "AAA=",
            r#"MaxLength="1""#,
            r#""$MaxLength":1"#,
            false,
        ),
        (
            "Decimal",
            "12.34",
            r#"Precision="4" Scale="2""#,
            r#""$Precision":4,"$Scale":2"#,
            true,
        ),
        (
            "Decimal",
            "123.45",
            r#"Precision="4" Scale="2""#,
            r#""$Precision":4,"$Scale":2"#,
            false,
        ),
        (
            "Decimal",
            "1.234",
            r#"Precision="4" Scale="2""#,
            r#""$Precision":4,"$Scale":2"#,
            false,
        ),
        (
            "Decimal",
            "1.234",
            r#"Scale="variable""#,
            r#""$Scale":"variable""#,
            true,
        ),
        (
            "Decimal",
            "1.2e3",
            r#"Scale="floating""#,
            r#""$Scale":"floating""#,
            true,
        ),
        (
            "TimeOfDay",
            "12:30:00.12",
            r#"Precision="2""#,
            r#""$Precision":2"#,
            true,
        ),
        (
            "TimeOfDay",
            "12:30:00.123",
            r#"Precision="2""#,
            r#""$Precision":2"#,
            false,
        ),
        (
            "DateTimeOffset",
            "2024-01-01T12:30:00.12Z",
            r#"Precision="2""#,
            r#""$Precision":2"#,
            true,
        ),
        (
            "DateTimeOffset",
            "2024-01-01T12:30:00.123Z",
            r#"Precision="2""#,
            r#""$Precision":2"#,
            false,
        ),
        (
            "Duration",
            "PT1.12S",
            r#"Precision="2""#,
            r#""$Precision":2"#,
            true,
        ),
        (
            "Duration",
            "PT1.123S",
            r#"Precision="2""#,
            r#""$Precision":2"#,
            false,
        ),
    ] {
        check(
            &format!(r#"<Term Name="Value" Type="Edm.{ty}" DefaultValue="{value}" {xml_facets}/>"#),
            &format!(
                r#""Value":{{"$Kind":"Term","$Type":"Edm.{ty}","$DefaultValue":"{value}",{json_facets}}}"#
            ),
            valid,
        );
        check(
            &format!(
                r#"<TypeDefinition Name="Defined" UnderlyingType="Edm.{ty}" {xml_facets}/><Term Name="Value" Type="N.Defined" DefaultValue="{value}"/>"#
            ),
            &format!(
                r#""Defined":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.{ty}",{json_facets}}},"Value":{{"$Kind":"Term","$Type":"N.Defined","$DefaultValue":"{value}"}}"#
            ),
            valid,
        );
    }
}

#[test]
fn term_names_are_simple_identifiers_unique_in_the_schema() {
    for (name, valid) in [
        ("Value", true),
        ("_Value2", true),
        ("日本語", true),
        ("1Value", false),
        ("N.Value", false),
        ("bad-name", false),
    ] {
        check(
            &format!(r#"<Term Name="{name}" Type="Edm.String"/>"#),
            &format!(r#""{name}":{{"$Kind":"Term","$Type":"Edm.String"}}"#),
            valid,
        );
    }
    check(
        r#"<Term Name="Value" Type="Edm.String"/><Term Name="Value" Type="Edm.Int32"/>"#,
        r#""Value":{"$Kind":"Term","$Type":"Edm.String"},"Value":{"$Kind":"Term","$Type":"Edm.Int32"}"#,
        false,
    );
    check(
        r#"<ComplexType Name="Value"/><Term Name="Value" Type="Edm.String"/>"#,
        r#""Value":{"$Kind":"ComplexType"},"Value":{"$Kind":"Term","$Type":"Edm.String"}"#,
        false,
    );
    check(
        r#"<Term Name="Value" Type="Edm.String"/><Term Name="Other" Type="Edm.String"/>"#,
        r#""Value":{"$Kind":"Term","$Type":"Edm.String"},"Other":{"$Kind":"Term","$Type":"Edm.String"}"#,
        true,
    );
}

#[test]
fn term_types_resolve_scalar_and_collection_types_in_scope() {
    let xml_types = r#"<ComplexType Name="Detail"/><EntityType Name="Entity" Abstract="true"/><EnumType Name="Choice"><Member Name="One"/></EnumType><TypeDefinition Name="Code" UnderlyingType="Edm.String"/>"#;
    let json_types = r#""Detail":{"$Kind":"ComplexType"},"Entity":{"$Kind":"EntityType","$Abstract":true},"Choice":{"$Kind":"EnumType","One":0},"Code":{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.String"}"#;
    for (name, valid) in [
        ("Edm.String", true),
        ("N.Detail", true),
        ("N.Entity", true),
        ("N.Choice", true),
        ("N.Code", true),
        ("N.Missing", false),
        ("Missing.Detail", false),
    ] {
        for collection in [false, true] {
            let ty = if collection {
                format!("Collection({name})")
            } else {
                name.into()
            };
            check(
                &format!(r#"{xml_types}<Term Name="Value" Type="{ty}"/>"#),
                &format!(
                    r#"{json_types},"Value":{{"$Kind":"Term","$Type":"{name}","$Collection":{collection}}}"#
                ),
                valid,
            );
        }
    }
}

#[test]
fn term_declarations_require_xml_name_and_type_and_json_term_kind() {
    for xml in [r#"<Term Type="Edm.String"/>"#, r#"<Term Name="Value"/>"#] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{xml}</Schema></edmx:DataServices></edmx:Edmx>"#
        );
        assert!(parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).is_err());
    }
    // JSON has an explicit Term kind and defaults the omitted type to Edm.String.
    check(
        r#"<Term Name="Value" Type="Edm.String"/>"#,
        r#""Value":{"$Kind":"Term"}"#,
        true,
    );
    check(
        r#"<Term Name="Value" Type="Edm.String" BaseTerm="N.Base" AppliesTo="Property"/><Term Name="Base" Type="Edm.String"/>"#,
        r#""Value":{"$Kind":"Term","$BaseTerm":"N.Base","$AppliesTo":["Property"]},"Base":{"$Kind":"Term"}"#,
        true,
    );
    check(
        r#"<Term Name="Value" Type=""/>"#,
        r#""Value":{"$Kind":"Term","$Type":null}"#,
        false,
    );
    let json = r#"{"$Version":"4.01","N":{"Value":{"$Type":"Edm.String"}}}"#;
    assert!(parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).is_err());
    for kind in ["null", "true", "1", r#""term""#] {
        let json = format!(
            r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":{kind},"$Type":"Edm.String"}}}}}}"#
        );
        assert!(parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).is_err());
    }
}
