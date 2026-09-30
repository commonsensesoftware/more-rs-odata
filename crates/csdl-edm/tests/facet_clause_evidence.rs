#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

fn sources(ty: &str, xml_facets: &str, json_facets: &str, version: &str) -> (String, String) {
    (
        format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="{version}"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="C"><Property Name="Data" Type="Edm.{ty}" {xml_facets}/></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
        ),
        format!(
            r#"{{"$Version":"{version}","N":{{"C":{{"$Kind":"ComplexType","Data":{{"$Type":"Edm.{ty}",{json_facets}}}}}}}}}"#
        ),
    )
}

fn check(ty: &str, xml_facets: &str, json_facets: &str, version: &str, valid: bool) {
    let (xml, json) = sources(ty, xml_facets, json_facets, version);
    for parsed in [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
    ] {
        let result = parsed
            .map_err(|e| e.to_string())
            .and_then(|d| Resolver::resolve_document(d.edmx.unwrap()).map_err(|e| format!("{e:?}")))
            .and_then(|m| validator::validate_document(&m).map_err(|e| format!("{e:?}")));
        assert_eq!(result.is_ok(), valid, "{ty}: {xml_facets}: {result:?}");
    }
}

#[test]
fn decimal_and_temporal_precision_have_distinct_bounds() {
    for ty in ["Decimal", "DateTimeOffset", "TimeOfDay", "Duration"] {
        for precision in [-1, 0, 1, 7, 12, 13, 28, 38] {
            let valid = if ty == "Decimal" {
                precision > 0
            } else {
                (0..=12).contains(&precision)
            };
            check(
                ty,
                &format!(r#"Precision="{precision}""#),
                &format!(r#""$Precision":{precision}"#),
                "4.01",
                valid,
            );
        }
    }
}

#[test]
fn numeric_scale_cannot_exceed_decimal_precision() {
    for (precision, scale, valid) in [(4, 0, true), (4, 4, true), (4, 5, false), (4, -1, false)] {
        check(
            "Decimal",
            &format!(r#"Precision="{precision}" Scale="{scale}""#),
            &format!(r#""$Precision":{precision},"$Scale":{scale}"#),
            "4.01",
            valid,
        );
    }
}

#[test]
fn floating_scale_version_guard_is_case_insensitive_at_source_boundary() {
    for scale in ["floating", "FLOATING", "Floating"] {
        for version in ["4.0", "4.01"] {
            let (xml, json) = sources(
                "Decimal",
                &format!(r#"Scale="{scale}""#),
                &format!(r#""$Scale":"{scale}""#),
                version,
            );
            for result in [
                parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
            ] {
                assert_eq!(
                    result.is_ok(),
                    version == "4.01",
                    "{version} {scale}: {result:?}"
                );
            }
        }
    }
    for scale in ["variable", "VARIABLE", "Variable"] {
        for version in ["4.0", "4.01"] {
            let (xml, json) = sources(
                "Decimal",
                &format!(r#"Scale="{scale}""#),
                &format!(r#""$Scale":"{scale}""#),
                version,
            );
            for result in [
                parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
            ] {
                let document = result.unwrap();
                assert_eq!(
                    document.to_json_value()["N"]["C"]["Data"]["$Scale"],
                    "variable"
                );
            }
        }
    }
}

#[test]
fn spatial_reference_identifiers_accept_nonnegative_numbers_and_variable() {
    for ty in ["GeometryPoint", "GeographyPoint"] {
        for (srid, json, valid) in [
            ("0", "0", true),
            ("4326", "4326", true),
            ("variable", r#""variable""#, true),
            ("-1", "-1", false),
            ("invalid", r#""invalid""#, false),
        ] {
            check(
                ty,
                &format!(r#"SRID="{srid}""#),
                &format!(r#""$SRID":{json}"#),
                "4.01",
                valid,
            );
        }
    }
}
