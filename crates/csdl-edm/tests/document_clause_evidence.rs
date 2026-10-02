use csdl_edm::{conformance::ValidationProfile as P, parser};

fn xml(reference: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01">{reference}<edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Local"/></edmx:DataServices></edmx:Edmx>"#
    )
}

fn json(reference: &str) -> String {
    format!(r#"{{"$Version":"4.01","$Reference":{{"types.json":{reference}}},"Local":{{}}}}"#)
}

#[test]
fn reference_uri_attributes_and_json_member_names_preserve_identity() {
    for uri in [
        "types",
        "../types",
        "https://example.test/types?x=1&y=2",
        "urn:example:types",
        "types%20v1",
    ] {
        let escaped = uri.replace('&', "&amp;");
        let source_xml = xml(&format!(
            r#"<edmx:Reference Uri="{escaped}"><edmx:Include Namespace="Types"/></edmx:Reference>"#
        ));
        let source_json = format!(
            r#"{{"$Version":"4.01","$Reference":{{"{uri}":{{"$Include":[{{"$Namespace":"Types"}}]}}}},"Local":{{}}}}"#
        );
        for document in [
            parser::from_xml_reader_with_profile(source_xml.as_bytes(), P::Strict).unwrap(),
            parser::from_json_reader_with_profile(source_json.as_bytes(), P::Strict).unwrap(),
        ] {
            let references = &document.edmx.as_ref().unwrap().references;
            assert_eq!(references.len(), 1);
            assert_eq!(references[0].uri, uri);
            assert_eq!(
                document.to_json_value()["$Reference"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                vec![uri]
            );
        }
    }
    assert!(
        parser::from_xml_reader_with_profile(
            xml(r#"<edmx:Reference><edmx:Include Namespace="Types"/></edmx:Reference>"#).as_bytes(),
            P::Strict
        )
        .is_err()
    );
    // JSON references are keyed by their URI, not an array of URI-less objects.
    for reference in [
        r#"[{"$Include":[{"$Namespace":"Types"}]}]"#,
        "null",
        "true",
        "1",
    ] {
        let source = format!(r#"{{"$Version":"4.01","$Reference":{reference},"Local":{{}}}}"#);
        assert!(parser::from_json_reader_with_profile(source.as_bytes(), P::Strict).is_err());
    }
}

#[test]
fn document_versions_require_supported_strings_and_survive_output() {
    for version in [
        "4.0", "4.01", "4", "4.00", "4.1", "3.0", "5.0", "", " 4.01 ",
    ] {
        let source_xml = xml("").replace("Version=\"4.01\"", &format!("Version=\"{version}\""));
        let source_json = format!(r#"{{"$Version":"{version}","Local":{{}}}}"#);
        for result in [
            parser::from_xml_reader_with_profile(source_xml.as_bytes(), P::Strict),
            parser::from_json_reader_with_profile(source_json.as_bytes(), P::Strict),
        ] {
            let valid = matches!(version, "4.0" | "4.01");
            assert_eq!(result.is_ok(), valid, "{version}: {result:?}");
            if let Ok(document) = result {
                assert_eq!(
                    document.edmx.as_ref().unwrap().version.as_deref(),
                    Some(version)
                );
                for format in [csdl_edm::CsdlFormat::Xml, csdl_edm::CsdlFormat::Json] {
                    let mut bytes = Vec::new();
                    document
                        .write_as_with_profile(&mut bytes, format, P::Strict)
                        .unwrap();
                    let parsed = match format {
                        csdl_edm::CsdlFormat::Xml => {
                            parser::from_xml_reader_with_profile(bytes.as_slice(), P::Strict)
                        }
                        csdl_edm::CsdlFormat::Json => {
                            parser::from_json_reader_with_profile(bytes.as_slice(), P::Strict)
                        }
                    }
                    .unwrap();
                    assert_eq!(parsed.edmx.unwrap().version.as_deref(), Some(version));
                }
            }
        }
    }
    let missing_xml = xml("").replace(" Version=\"4.01\"", "");
    assert!(parser::from_xml_reader_with_profile(missing_xml.as_bytes(), P::Strict).is_err());
    for source in [
        r#"{"Local":{}}"#,
        r#"{"$Version":4.01,"Local":{}}"#,
        r#"{"$Version":null,"Local":{}}"#,
        r#"{"$Version":true,"Local":{}}"#,
    ] {
        assert!(parser::from_json_reader_with_profile(source.as_bytes(), P::Strict).is_err());
    }
}

#[test]
fn xml_references_require_uri_and_at_least_one_include_kind() {
    for (reference, valid) in [
        (
            r#"<edmx:Reference Uri="types.xml"><edmx:Include Namespace="Types"/></edmx:Reference>"#,
            true,
        ),
        (
            r#"<edmx:Reference Uri="types.xml"><edmx:IncludeAnnotations TermNamespace="Terms"/></edmx:Reference>"#,
            true,
        ),
        (
            r#"<edmx:Reference><edmx:Include Namespace="Types"/></edmx:Reference>"#,
            false,
        ),
        (r#"<edmx:Reference Uri="types.xml"/>"#, false),
    ] {
        let result = parser::from_xml_reader_with_profile(xml(reference).as_bytes(), P::Strict);
        assert_eq!(result.is_ok(), valid, "{reference}: {result:?}");
    }
}

#[test]
fn include_objects_require_namespace_in_each_representation() {
    for (attributes, valid) in [
        (r#"Namespace="Types""#, true),
        (r#"Namespace="Types" Alias="T""#, true),
        (r#"Alias="T""#, false),
    ] {
        let source = xml(&format!(
            r#"<edmx:Reference Uri="types.xml"><edmx:Include {attributes}/></edmx:Reference>"#
        ));
        assert_eq!(
            parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict).is_ok(),
            valid
        );
    }
    for (include, valid) in [
        (r#"[{"$Namespace":"Types"}]"#, true),
        (r#"[{"$Namespace":"Types","$Alias":"T"}]"#, true),
        (r#"[{"$Alias":"T"}]"#, false),
        (r#"{"$Namespace":"Types"}"#, false),
        (r#"["Types"]"#, false),
        (r#"[{"$Namespace":42}]"#, false),
    ] {
        let source = json(&format!(r#"{{"$Include":{include}}}"#));
        let result = parser::from_json_reader_with_profile(source.as_bytes(), P::Strict);
        assert_eq!(result.is_ok(), valid, "{source}: {result:?}");
    }
}

#[test]
fn include_annotations_requires_term_namespace_and_accepts_filters() {
    for (attributes, valid) in [
        (r#"TermNamespace="Terms""#, true),
        (
            r#"TermNamespace="Terms" Qualifier="Phone" TargetNamespace="Types""#,
            true,
        ),
        (r#"Qualifier="Phone""#, false),
    ] {
        let source = xml(&format!(
            r#"<edmx:Reference Uri="terms.xml"><edmx:IncludeAnnotations {attributes}/></edmx:Reference>"#
        ));
        let result = parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict);
        assert_eq!(result.is_ok(), valid, "{source}: {result:?}");
    }
    for (include, valid) in [
        (r#"[{"$TermNamespace":"Terms"}]"#, true),
        (
            r#"[{"$TermNamespace":"Terms","$Qualifier":"Phone","$TargetNamespace":"Types"}]"#,
            true,
        ),
        (r#"[{"$Qualifier":"Phone"}]"#, false),
        (r#"{"$TermNamespace":"Terms"}"#, false),
        (r#"["Terms"]"#, false),
        (r#"[{"$TermNamespace":42}]"#, false),
    ] {
        let source = json(&format!(r#"{{"$IncludeAnnotations":{include}}}"#));
        let result = parser::from_json_reader_with_profile(source.as_bytes(), P::Strict);
        assert_eq!(result.is_ok(), valid, "{source}: {result:?}");
    }
}

#[test]
fn included_namespaces_are_unique_across_references() {
    for (second, valid) in [("Other", true), ("Types", false)] {
        let source = xml(&format!(
            r#"<edmx:Reference Uri="one.xml"><edmx:Include Namespace="Types"/></edmx:Reference><edmx:Reference Uri="two.xml"><edmx:Include Namespace="{second}"/></edmx:Reference>"#
        ));
        assert_eq!(
            parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict).is_ok(),
            valid
        );
        let source = format!(
            r#"{{"$Version":"4.01","$Reference":{{"one.json":{{"$Include":[{{"$Namespace":"Types"}}]}},"two.json":{{"$Include":[{{"$Namespace":"{second}"}}]}}}},"Local":{{}}}}"#
        );
        assert_eq!(
            parser::from_json_reader_with_profile(source.as_bytes(), P::Strict).is_ok(),
            valid
        );
    }
}
