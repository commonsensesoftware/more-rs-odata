#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

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
