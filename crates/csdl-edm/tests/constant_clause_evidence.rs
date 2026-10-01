#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

fn check(kind: &str, value: &str, valid: bool) {
    // Inputs are authored independently in each representation; JSON strings
    // receive their literal type from the resolved term.
    let temporal = matches!(kind, "DateTimeOffset" | "TimeOfDay" | "Duration");
    let xml_precision = if temporal { r#" Precision="12""# } else { "" };
    let json_precision = if temporal { r#", "$Precision":12"# } else { "" };
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.{kind}"{xml_precision}/><ComplexType Name="Owner"><Annotation Term="N.Value"><{kind}>{value}</{kind}></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(
        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"Edm.{kind}"{json_precision}}},"Owner":{{"$Kind":"ComplexType","@N.Value":"{value}"}}}}}}"#
    );
    for parsed in [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
    ] {
        let result = parsed
            .map_err(|e| e.to_string())
            .and_then(|d| Resolver::resolve_document(d.edmx.unwrap()).map_err(|e| format!("{e:?}")))
            .and_then(|model| {
                validator::validate_document(&model).map_err(|e| format!("{e:?}"))?;
                let diagnostics = validator::validate_annotations(&model);
                if diagnostics.is_empty() {
                    Ok(())
                } else {
                    Err(format!("{diagnostics:?}"))
                }
            });
        assert_eq!(result.is_ok(), valid, "{result:?}: {kind} {value}");
    }
}

#[test]
fn date_constants_require_valid_calendar_dates_without_timezone() {
    for (value, valid) in [
        ("2024-02-29", true),
        ("2000-02-29", true),
        ("2025-12-31", true),
        ("2023-02-29", false),
        ("1900-02-29", false),
        ("2025-04-31", false),
        ("2025-00-10", false),
        ("2025-13-01", false),
        ("2025-01-00", false),
        ("2025-01-01Z", false),
        ("2025-01-01+01:00", false),
        ("2025-1-1", false),
    ] {
        check("Date", value, valid);
    }
}

#[test]
fn guid_constants_require_hexadecimal_uuid_shape() {
    for (value, valid) in [
        ("01234567-89ab-cdef-0123-456789abcdef", true),
        ("01234567-89AB-CDEF-0123-456789ABCDEF", true),
        ("00000000-0000-0000-0000-000000000000", true),
        ("0123456789abcdef0123456789abcdef", false),
        ("01234567-89ab-cdef-0123-456789abcdeg", false),
        ("01234567-89ab-cdef-0123-456789abcde", false),
        ("{01234567-89ab-cdef-0123-456789abcdef}", false),
    ] {
        check("Guid", value, valid);
    }
}

#[test]
fn timestamp_constants_require_timezone_and_reject_end_of_day() {
    for (value, valid) in [
        ("2024-02-29T23:59:59Z", true),
        ("2024-02-29T12:30:00.123456789012+14:00", true),
        ("2024-02-29T12:30:00-14:00", true),
        ("2024-02-29T12:30Z", true),
        ("2024-02-29T23:59:60Z", true),
        ("2023-02-29T12:30:00Z", false),
        ("2024-02-29T24:00:00Z", false),
        ("2024-02-29T12:30:00", false),
        ("2024-02-29T12:30:00+14:01", false),
        ("2024-02-29T12:30:00+15:00", false),
        ("2024-02-29T12:60:00Z", false),
        ("2024-02-29T12:30:61Z", false),
    ] {
        check("DateTimeOffset", value, valid);
    }
}

#[test]
fn time_constants_enforce_clock_boundaries_without_timezone() {
    for (value, valid) in [
        ("00:00:00", true),
        ("23:59:59", true),
        ("12:30", true),
        ("23:59:60", true),
        ("12:30:00.123456789012", true),
        ("24:00:00", false),
        ("12:60:00", false),
        ("12:30:61", false),
        ("12:30:00Z", false),
        ("12:30:00+01:00", false),
        ("1:30:00", false),
        ("12:30:00.", false),
    ] {
        check("TimeOfDay", value, valid);
    }
}

#[test]
fn duration_constants_use_day_time_components() {
    for (value, valid) in [
        ("PT0S", true),
        ("P1D", true),
        ("-P1DT2H3M4.5S", true),
        ("PT25H", true),
        ("PT0.123456789012S", true),
        ("P", false),
        ("PT", false),
        ("P1Y", false),
        ("P1M", false),
        ("P1W", false),
        ("P1DT", false),
        ("PT1S2H", false),
        ("PT1H2H", false),
        ("PT-1S", false),
    ] {
        check("Duration", value, valid);
    }
}
