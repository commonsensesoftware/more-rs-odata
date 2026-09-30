use csdl_edm::conformance;
#[cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::parser;

#[test]
fn imported_families_are_explicitly_classified() {
    let evidence: serde_json::Value =
        serde_json::from_str(include_str!("../docs/grammar-evidence.json")).unwrap();
    let mut names = std::collections::HashSet::new();
    let mut cases = 0;
    for family in evidence["families"].as_array().unwrap() {
        assert!(names.insert((
            family["source"].as_str().unwrap(),
            family["family"].as_str().unwrap()
        )));
        assert!(matches!(
            family["disposition"].as_str().unwrap(),
            "partial" | "executed" | "outside-static-scope"
        ));
        cases += family["cases"].as_u64().unwrap_or(0);
    }
    assert_eq!(cases, evidence["abnf_total_cases"].as_u64().unwrap());
    for source in evidence["sources"].as_array().unwrap() {
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("data/conformance/oasis-4.01")
                .join(source["file"].as_str().unwrap())
                .is_file()
        );
        assert_eq!(source["sha256"].as_str().unwrap().len(), 64);
    }
}

#[test]
fn executable_cases_match_the_unmodified_official_corpus() {
    use quick_xml::{Reader, events::Event, name::QName};
    let evidence: serde_json::Value =
        serde_json::from_str(include_str!("../docs/grammar-evidence.json")).unwrap();
    let mut reader = Reader::from_str(include_str!(
        "../data/conformance/oasis-4.01/odata-abnf-testcases.xml"
    ));
    let mut cases = Vec::new();
    let mut rule = String::new();
    let mut valid = true;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) if e.name().as_ref() == b"TestCase" => {
                valid = true;
                for attribute in e.attributes() {
                    let attribute = attribute.unwrap();
                    match attribute.key.as_ref() {
                        b"Rule" => {
                            rule = attribute
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .unwrap()
                                .into_owned()
                        }
                        b"FailAt" => valid = false,
                        _ => {}
                    }
                }
            }
            Event::Start(e) if e.name().as_ref() == b"Input" => {
                let text = reader.read_text(QName(b"Input")).unwrap();
                cases.push((
                    rule.clone(),
                    quick_xml::escape::unescape(&text.decode().unwrap())
                        .unwrap()
                        .into_owned(),
                    valid,
                ));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(
        cases.len() as u64,
        evidence["abnf_total_cases"].as_u64().unwrap()
    );
    for case in evidence["executable_cases"].as_array().unwrap() {
        let (rule, input, valid) = &cases[case["index"].as_u64().unwrap() as usize];
        assert_eq!(case["rule"], *rule);
        assert_eq!(case["input"], *input);
        if case["adaptation"].is_null() {
            assert_eq!(case["valid"], *valid);
        }
    }
}

#[test]
fn imported_identifier_cases_use_the_csdl_identifier_checker() {
    let evidence: serde_json::Value =
        serde_json::from_str(include_str!("../docs/grammar-evidence.json")).unwrap();
    for case in evidence["executable_cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["type"] == "identifier")
    {
        assert_eq!(
            conformance::is_simple_identifier(case["input"].as_str().unwrap()),
            case["valid"].as_bool().unwrap(),
            "{case}"
        );
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn imported_primitive_cases_validate_in_independent_xml_and_json_sources() {
    use csdl_edm::{resolver::Resolver, validator};
    let evidence: serde_json::Value =
        serde_json::from_str(include_str!("../docs/grammar-evidence.json")).unwrap();
    let mut failures = Vec::new();
    for case in evidence["executable_cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["type"] != "identifier")
    {
        let ty = case["type"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let expected = case["valid"].as_bool().unwrap();
        let temporal = matches!(ty, "Edm.DateTimeOffset" | "Edm.TimeOfDay" | "Edm.Duration");
        let facets = if temporal {
            r#" Precision="12""#
        } else if ty == "Edm.Decimal" {
            r#" Scale="floating""#
        } else {
            ""
        };
        let escaped = input
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('"', "&quot;");
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{ty}" DefaultValue="{escaped}"{facets}/></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        let mut json = serde_json::json!({"$Version":"4.01","N":{"Value":{"$Kind":"Term","$Type":ty,"$DefaultValue":input}}});
        if temporal {
            json["N"]["Value"]["$Precision"] = 12.into();
        }
        if ty == "Edm.Decimal" {
            json["N"]["Value"]["$Scale"] = "floating".into();
        }
        if ty == "Edm.Boolean" && matches!(input, "true" | "false") {
            json["N"]["Value"]["$DefaultValue"] = (input == "true").into();
        }
        for (format, parsed) in [
            ("XML", parser::from_xml_reader(xml.as_bytes())),
            (
                "JSON",
                parser::from_json_reader(json.to_string().as_bytes()),
            ),
        ] {
            let valid = parsed
                .ok()
                .and_then(|d| Resolver::resolve_document(d.edmx.unwrap()).ok())
                .is_some_and(|m| validator::validate_document(&m).is_ok());
            if valid != expected {
                failures.push(format!("{format}: {case}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
