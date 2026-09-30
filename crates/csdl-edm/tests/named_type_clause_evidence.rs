#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

fn documents(xml: &str, json: &str) -> Vec<std::io::Result<csdl_edm::CsdlDocument>> {
    vec![parser::from_xml_reader_with_profile(format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{xml}</Schema></edmx:DataServices></edmx:Edmx>"#).as_bytes(), P::Strict), parser::from_json_reader_with_profile(format!(r#"{{"$Version":"4.01","N":{{{json}}}}}"#).as_bytes(), P::Strict)]
}

fn check(xml: &str, json: &str, valid: bool) {
    for parsed in documents(xml, json) {
        let result = parsed
            .map_err(|e| e.to_string())
            .and_then(|d| Resolver::resolve_document(d.edmx.unwrap()).map_err(|e| format!("{e:?}")))
            .and_then(|m| validator::validate_document(&m).map_err(|e| format!("{e:?}")));
        assert_eq!(result.is_ok(), valid, "{result:?}\n{xml}\n{json}");
    }
}

#[test]
fn enum_member_values_must_fit_the_underlying_integer_type() {
    for (ty, min, max, below, above) in [
        ("Byte", "0", "255", "-1", "256"),
        ("SByte", "-128", "127", "-129", "128"),
        ("Int16", "-32768", "32767", "-32769", "32768"),
        (
            "Int32",
            "-2147483648",
            "2147483647",
            "-2147483649",
            "2147483648",
        ),
        (
            "Int64",
            "-9223372036854775808",
            "9223372036854775807",
            "-9223372036854775809",
            "9223372036854775808",
        ),
    ] {
        for (value, valid) in [(min, true), (max, true), (below, false), (above, false)] {
            check(
                &format!(
                    r#"<EnumType Name="E" UnderlyingType="Edm.{ty}"><Member Name="Value" Value="{value}"/></EnumType>"#
                ),
                &format!(
                    r#""E":{{"$Kind":"EnumType","$UnderlyingType":"Edm.{ty}","Value":{value}}}"#
                ),
                valid,
            );
        }
    }
    for value in ["1.5", "true"] {
        check(
            &format!(r#"<EnumType Name="E"><Member Name="Value" Value="{value}"/></EnumType>"#),
            &format!(r#""E":{{"$Kind":"EnumType","Value":{value}}}"#),
            false,
        );
    }
}

#[test]
fn flags_require_explicit_nonnegative_values() {
    for (value, valid) in [("0", true), ("1", true), ("3", true), ("-1", false)] {
        check(
            &format!(
                r#"<EnumType Name="E" IsFlags="true"><Member Name="Value" Value="{value}"/></EnumType>"#
            ),
            &format!(r#""E":{{"$Kind":"EnumType","$IsFlags":true,"Value":{value}}}"#),
            valid,
        );
    }
    check(
        r#"<EnumType Name="E" IsFlags="true"><Member Name="Value"/></EnumType>"#,
        r#""E":{"$Kind":"EnumType","$IsFlags":true,"Value":null}"#,
        false,
    );
}

#[test]
fn implicit_xml_members_keep_document_order_and_cannot_mix_explicit_values() {
    for document in documents(
        r#"<EnumType Name="E"><Member Name="Z"/><Member Name="A"/><Member Name="M"/></EnumType>"#,
        r#""E":{"$Kind":"EnumType","Z":0,"A":1,"M":2}"#,
    ) {
        let model = Resolver::resolve_document(document.unwrap().edmx.unwrap()).unwrap();
        validator::validate_document(&model).unwrap();
        let csdl_edm::edm::SchemaElement::EnumType(enumeration) =
            model.schemas[0].elements[0].as_ref()
        else {
            panic!()
        };
        assert_eq!(
            enumeration
                .members
                .iter()
                .map(|m| (m.name.as_str(), m.resolved_value))
                .collect::<Vec<_>>(),
            vec![("Z", 0), ("A", 1), ("M", 2)]
        );
    }
    check(
        r#"<EnumType Name="E"><Member Name="Z"/><Member Name="A" Value="1"/></EnumType>"#,
        r#""E":{"$Kind":"EnumType","Z":null,"A":1}"#,
        false,
    );
}

#[test]
fn type_definitions_cannot_use_named_or_nonprimitive_underlying_types() {
    for (ty, valid) in [
        ("Edm.String", true),
        ("Edm.Decimal", true),
        ("Edm.DateTimeOffset", true),
        ("N.Other", false),
        ("N.Record", false),
        ("Edm.EntityType", false),
    ] {
        check(
            &format!(
                r#"<TypeDefinition Name="Other" UnderlyingType="Edm.String"/><ComplexType Name="Record"/><TypeDefinition Name="Value" UnderlyingType="{ty}"/>"#
            ),
            &format!(
                r#""Other":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.String"}},"Record":{{"$Kind":"ComplexType"}},"Value":{{"$Kind":"TypeDefinition","$UnderlyingType":"{ty}"}}"#
            ),
            valid,
        );
    }
}

#[test]
fn inherited_type_definition_facets_cannot_be_respecified() {
    for (xml_facet, json_facet, valid) in [
        (r#"Scale="2""#, r#""$Scale":2"#, true),
        (r#"Precision="4""#, r#""$Precision":4"#, false),
        (r#"Precision="5""#, r#""$Precision":5"#, false),
        (r#"Scale="5""#, r#""$Scale":5"#, false),
    ] {
        check(
            &format!(
                r#"<TypeDefinition Name="Amount" UnderlyingType="Edm.Decimal" Precision="4"/><ComplexType Name="C"><Property Name="Value" Type="N.Amount" {xml_facet}/></ComplexType>"#
            ),
            &format!(
                r#""Amount":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.Decimal","$Precision":4}},"C":{{"$Kind":"ComplexType","Value":{{"$Type":"N.Amount",{json_facet}}}}}"#
            ),
            valid,
        );
    }
    for (ty, facet, value) in [
        ("String", "MaxLength", "20"),
        ("String", "Unicode", "false"),
        ("Decimal", "Precision", "4"),
        ("Decimal", "Scale", "2"),
        ("GeographyPoint", "SRID", "4326"),
    ] {
        for repeated in [false, true] {
            let xml_usage = if repeated {
                format!(r#"{facet}="{value}""#)
            } else {
                String::new()
            };
            let json_usage = if repeated {
                format!(r#", "${facet}":{value}"#)
            } else {
                String::new()
            };
            check(
                &format!(
                    r#"<TypeDefinition Name="Defined" UnderlyingType="Edm.{ty}" {facet}="{value}"/><ComplexType Name="C"><Property Name="Value" Type="N.Defined" {xml_usage}/></ComplexType>"#
                ),
                &format!(
                    r#""Defined":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.{ty}","${facet}":{value}}},"C":{{"$Kind":"ComplexType","Value":{{"$Type":"N.Defined"{json_usage}}}}}"#
                ),
                !repeated,
            );
        }
    }
}

#[test]
fn enum_and_type_definition_required_members_are_checked_before_resolution() {
    for (xml, json, valid) in [
        (
            r#"<EnumType Name="E"><Member Name="One" Value="1"/></EnumType>"#,
            r#""E":{"$Kind":"EnumType","One":1}"#,
            true,
        ),
        (
            r#"<EnumType><Member Name="One" Value="1"/></EnumType>"#,
            r#""E":{"One":1}"#,
            false,
        ),
        (
            r#"<EnumType Name="E"/>"#,
            r#""E":{"$Kind":"EnumType"}"#,
            false,
        ),
        (
            r#"<TypeDefinition Name="T" UnderlyingType="Edm.String"/>"#,
            r#""T":{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.String"}"#,
            true,
        ),
        (
            r#"<TypeDefinition UnderlyingType="Edm.String"/>"#,
            r#""T":{"$UnderlyingType":"Edm.String"}"#,
            false,
        ),
        (
            r#"<TypeDefinition Name="T"/>"#,
            r#""T":{"$Kind":"TypeDefinition"}"#,
            false,
        ),
    ] {
        for parsed in documents(xml, json) {
            assert_eq!(parsed.is_ok(), valid, "{xml} / {json}: {parsed:?}");
        }
    }
}
