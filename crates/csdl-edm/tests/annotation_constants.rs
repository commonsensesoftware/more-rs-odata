use csdl_edm::parser;

#[test]
fn invalid_xml_constants_are_rejected_in_every_attachment_form() {
    for (kind, value) in [
        ("Bool", "yes"),
        ("Int", "overflow"),
        ("Int", "9223372036854775808"),
        ("Float", "oops"),
    ] {
        for expression in [
            format!(r#"<Annotation Term="Demo.Value" {kind}="{value}"/>"#),
            format!(r#"<Annotation Term="Demo.Value"><{kind}>{value}</{kind}></Annotation>"#),
            format!(
                r#"<Annotation Term="Demo.Value"><Record><PropertyValue Property="P" {kind}="{value}"/></Record></Annotation>"#
            ),
            format!(
                r#"<Annotation Term="Demo.Value"><Record><Annotation Term="Demo.Nested" {kind}="{value}"/></Record></Annotation>"#
            ),
        ] {
            let xml = format!(
                r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx"><edmx:DataServices><Schema Namespace="Demo">{expression}</Schema></edmx:DataServices></edmx:Edmx>"#
            );
            assert!(
                parser::from_xml_reader(xml.as_bytes()).is_err(),
                "{expression}"
            );
        }
    }
}

#[test]
fn valid_xml_constants_include_special_floats_and_integer_limits() {
    for (kind, value) in [
        ("Bool", "false"),
        ("Int", "-9223372036854775808"),
        ("Float", "INF"),
        ("Float", "-INF"),
        ("Float", "NaN"),
    ] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx"><edmx:DataServices><Schema Namespace="Demo"><Annotation Term="Demo.Value" {kind}="{value}"/><Annotation Term="Demo.Other"><{kind}>{value}</{kind}></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        parser::from_xml_reader(xml.as_bytes()).unwrap();
    }
}
