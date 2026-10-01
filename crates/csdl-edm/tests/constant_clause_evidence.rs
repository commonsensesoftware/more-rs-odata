#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

fn check(kind: &str, value: &str, valid: bool) {
    // Inputs are authored independently in each representation; JSON strings
    // receive their literal type from the resolved term.
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.{kind}"/><ComplexType Name="Owner"><Annotation Term="N.Value"><{kind}>{value}</{kind}></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(
        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"Edm.{kind}"}},"Owner":{{"$Kind":"ComplexType","@N.Value":"{value}"}}}}}}"#
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
