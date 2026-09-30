use csdl_edm::{
    conformance::{ValidationProfile as P, is_namespace, is_simple_identifier},
    parser,
};

fn xml(body: &str) -> String {
    format!(
        r#"<x:Edmx xmlns:x="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><x:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Demo">{body}</Schema></x:DataServices></x:Edmx>"#
    )
}

#[test]
fn strict_declaration_grammar_and_cardinalities() {
    for body in [
        r#"<EntityType/>"#,
        r#"<EntityType Name="Bad Name"/>"#,
        r#"<Term Name="MissingType"/>"#,
        r#"<ComplexType Name="C"><Key><PropertyRef Name="ID"/></Key></ComplexType>"#,
        r#"<EntityType Name="E"><Key/><Key/></EntityType>"#,
        r#"<EntityType Name="E"><Property Name="P" Type="Edm.String"/><Property Name="P" Type="Edm.String"/></EntityType>"#,
        r#"<Function Name="F"/>"#,
        r#"<EntityType Name="E" IsBound="true"/>"#,
    ] {
        assert!(
            parser::from_xml_reader_with_profile(xml(body).as_bytes(), P::Strict).is_err(),
            "{body}"
        );
    }
    parser::from_xml_reader_with_profile(
        xml(r#"<ComplexType Name="C"><Property Name="P" Type="Edm.String"/></ComplexType>"#)
            .as_bytes(),
        P::Strict,
    )
    .unwrap();
}

#[test]
fn namespaces_and_extensions_cannot_inject_known_declarations() {
    let input = xml(
        r#"<f:EntityType xmlns:f="urn:foreign" Name="Injected"/><Future><EntityType Name="Hidden"/></Future><EntityType Name="Visible" xmlns:f="urn:foreign" f:Abstract="invalid"/>"#,
    );
    for profile in [P::Tolerant, P::Strict] {
        let doc = parser::from_xml_reader_with_profile(input.as_bytes(), profile).unwrap();
        assert_eq!(doc.edmx.unwrap().schemas[0].elements.len(), 1);
    }
    assert!(
        parser::from_xml_reader_with_profile(b"<Edmx Version='4.01'/>".as_slice(), P::Strict)
            .is_err()
    );
    assert!(
        parser::from_xml_reader_with_profile(
            xml("")
                .replace(r#"Version="4.01""#, r#"Version="5.0""#)
                .as_bytes(),
            P::Strict
        )
        .is_err()
    );
}

#[test]
fn unicode_identifiers_follow_csdl_categories_and_codepoint_limits() {
    for name in ["A\u{301}", "A\u{200c}", "Ⅳ", "A\u{203f}B", "_Name"] {
        assert!(is_simple_identifier(name), "{name}");
    }
    for name in ["", "9Name", "A²", "\u{301}A", "A-B"] {
        assert!(!is_simple_identifier(name), "{name}");
    }
    assert!(is_simple_identifier(&"é".repeat(128)));
    assert!(!is_simple_identifier(&"é".repeat(129)));
    assert!(is_namespace("Org.Example"));
    assert!(!is_namespace("Org..Example"));
}

#[test]
fn aliases_and_includes_have_document_scope_uniqueness() {
    for schemas in [
        r#"<Schema Namespace="N" Alias="Edm"/>"#,
        r#"<Schema Namespace="N" Alias="A"/><Schema Namespace="M" Alias="A"/>"#,
        r#"<Schema Namespace="N" Alias="M"/><Schema Namespace="M"/>"#,
        r#"<Schema Namespace="N"/><Schema Namespace="N"/>"#,
    ] {
        let input = format!(
            r#"<Edmx xmlns="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><DataServices xmlns:e="http://docs.oasis-open.org/odata/ns/edm">{}</DataServices></Edmx>"#,
            schemas.replace("<Schema", "<e:Schema")
        );
        assert!(
            parser::from_xml_reader_with_profile(input.as_bytes(), P::Strict).is_err(),
            "{schemas}"
        );
    }
}

#[test]
fn strict_json_does_not_default_malformed_or_missing_declarations() {
    for input in [
        r#"{}"#,
        r#"{"$Version":4.01}"#,
        r#"{"$Version":"4.01","$Version":"4.0"}"#,
        r#"{"$Version":"4.01","N":{"E":{}}}"#,
        r#"{"$Version":"4.01","N":{"E":{"$Kind":"EntityType","$Abstract":"false"}}}"#,
        r#"{"$Version":"4.01","N":{"F":[]}}"#,
        r#"{"$Version":"4.01","N":{"F":{"$Kind":"Function","$ReturnType":{}}}}"#,
    ] {
        assert!(
            parser::from_json_reader_with_profile(input.as_bytes(), P::Strict).is_err(),
            "{input}"
        );
    }
    let input = r#"{"$Version":"4.01","N":{"C":{"$Kind":"ComplexType","P":{}}}}"#;
    parser::from_json_reader_with_profile(input.as_bytes(), P::Strict).unwrap();
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn strict_annotation_diagnostics_do_not_break_tolerant_consumption() {
    let doc = parser::from_xml_reader_with_profile(
        xml(r#"<Annotation Term="Unknown.Term" String="value"/>"#).as_bytes(),
        P::Strict,
    )
    .unwrap();
    let model = csdl_edm::resolver::Resolver::resolve_document(doc.edmx.unwrap()).unwrap();
    let tolerant = csdl_edm::validator::validate_document_with_profile(&model, P::Tolerant);
    let strict = csdl_edm::validator::validate_document_with_profile(&model, P::Strict);
    assert!(tolerant.is_valid());
    assert!(!strict.is_valid());
    assert_eq!(tolerant.annotations.len(), strict.annotations.len());
}
