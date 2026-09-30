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
