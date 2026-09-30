use csdl_edm::{conformance::ValidationProfile as P, parser};

fn xml(references: &str, declarations: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01">{references}<edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{declarations}</Schema></edmx:DataServices></edmx:Edmx>"#
    )
}

#[test]
fn reference_uri_uniqueness_has_independent_xml_and_json_cases() {
    for (second, valid) in [("other.xml", true), ("types.xml", false)] {
        let source = xml(
            &format!(
                r#"<edmx:Reference Uri="types.xml"><edmx:Include Namespace="A"/></edmx:Reference><edmx:Reference Uri="{second}"><edmx:Include Namespace="B"/></edmx:Reference>"#
            ),
            "",
        );
        assert_eq!(
            parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict).is_ok(),
            valid
        );
    }
    for (source, valid) in [
        (
            r#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"A"}]},"other.json":{"$Include":[{"$Namespace":"B"}]}},"N":{}}"#,
            true,
        ),
        (
            r#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"A"}]},"types.json":{"$Include":[{"$Namespace":"B"}]}},"N":{}}"#,
            false,
        ),
    ] {
        assert_eq!(
            parser::from_json_reader_with_profile(source.as_bytes(), P::Strict).is_ok(),
            valid
        );
    }
}

#[test]
fn enum_member_names_are_identifiers_and_unique_in_each_representation() {
    for (members, valid) in [
        (r#"<Member Name="One"/><Member Name="Two"/>"#, true),
        (r#"<Member Name="One"/><Member Name="One"/>"#, false),
        (r#"<Member Name="1Invalid"/>"#, false),
    ] {
        let source = xml("", &format!(r#"<EnumType Name="E">{members}</EnumType>"#));
        assert_eq!(
            parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict).is_ok(),
            valid,
            "{source}"
        );
    }
    for (members, valid) in [
        (r#""One":1,"Two":2"#, true),
        (r#""One":1,"One":2"#, false),
        (r#""1Invalid":1"#, false),
    ] {
        let source =
            format!(r#"{{"$Version":"4.01","N":{{"E":{{"$Kind":"EnumType",{members}}}}}}}"#);
        assert_eq!(
            parser::from_json_reader_with_profile(source.as_bytes(), P::Strict).is_ok(),
            valid,
            "{source}"
        );
    }
}
