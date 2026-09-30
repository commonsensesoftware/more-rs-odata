#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

fn documents(xml: &str, json: &str, version: &str) -> Vec<csdl_edm::CsdlDocument> {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="{version}"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{xml}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(r#"{{"$Version":"{version}","N":{{{json}}}}}"#);
    vec![
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).unwrap(),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).unwrap(),
    ]
}

fn valid(xml: &str, json: &str, version: &str, expected: bool) {
    for document in documents(xml, json, version) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let result = validator::validate_document(&model);
        assert_eq!(result.is_ok(), expected, "{result:?}");
    }
}

#[test]
fn entity_and_complex_inheritance_reject_direct_and_indirect_cycles() {
    for kind in ["EntityType", "ComplexType"] {
        for (base, cyclic) in [("N.A", true), ("N.B", true), ("N.C", false)] {
            let xml = format!(
                r#"<{kind} Name="A" BaseType="{base}"/><{kind} Name="B" BaseType="N.A"/><{kind} Name="C"/>"#
            );
            let json = format!(
                r#""A":{{"$Kind":"{kind}","$BaseType":"{base}"}},"B":{{"$Kind":"{kind}","$BaseType":"N.A"}},"C":{{"$Kind":"{kind}"}}"#
            );
            for document in documents(&xml, &json, "4.01") {
                let result = Resolver::resolve_document(document.edmx.unwrap());
                assert_eq!(result.is_err(), cyclic, "{kind} base {base}");
                if let Ok(model) = result {
                    validator::validate_document(&model).unwrap();
                }
            }
        }
    }
}

#[test]
fn open_entity_and_complex_bases_require_open_derived_types() {
    for kind in ["EntityType", "ComplexType"] {
        for open in [true, false] {
            valid(
                &format!(
                    r#"<{kind} Name="Base" OpenType="true"/><{kind} Name="Derived" BaseType="N.Base" OpenType="{open}"/>"#
                ),
                &format!(
                    r#""Base":{{"$Kind":"{kind}","$OpenType":true}},"Derived":{{"$Kind":"{kind}","$BaseType":"N.Base","$OpenType":{open}}}"#
                ),
                "4.01",
                open,
            );
        }
        valid(
            &format!(
                r#"<{kind} Name="Base"/><{kind} Name="Derived" BaseType="N.Base" OpenType="true"/>"#
            ),
            &format!(
                r#""Base":{{"$Kind":"{kind}"}},"Derived":{{"$Kind":"{kind}","$BaseType":"N.Base","$OpenType":true}}"#
            ),
            "4.01",
            true,
        );
    }
}

#[test]
fn media_bases_require_media_derived_types() {
    for stream in [true, false] {
        valid(
            &format!(
                r#"<EntityType Name="Base" HasStream="true"/><EntityType Name="Derived" BaseType="N.Base" HasStream="{stream}"/>"#
            ),
            &format!(
                r#""Base":{{"$Kind":"EntityType","$HasStream":true}},"Derived":{{"$Kind":"EntityType","$BaseType":"N.Base","$HasStream":{stream}}}"#
            ),
            "4.01",
            stream,
        );
    }
}

#[test]
fn abstract_entity_cannot_derive_from_concrete_base() {
    for abstract_base in [true, false] {
        valid(
            &format!(
                r#"<EntityType Name="Base" Abstract="{abstract_base}"/><EntityType Name="Derived" Abstract="true" BaseType="N.Base"/>"#
            ),
            &format!(
                r#""Base":{{"$Kind":"EntityType","$Abstract":{abstract_base}}},"Derived":{{"$Kind":"EntityType","$Abstract":true,"$BaseType":"N.Base"}}"#
            ),
            "4.01",
            abstract_base,
        );
    }
}

#[test]
fn legacy_concrete_entities_need_declared_or_inherited_keys() {
    for version in ["4.0", "4.01"] {
        valid(
            r#"<EntityType Name="Keyless"/>"#,
            r#""Keyless":{"$Kind":"EntityType"}"#,
            version,
            version == "4.01",
        );
        valid(
            r#"<EntityType Name="Base"><Key><PropertyRef Name="Id"/></Key><Property Name="Id" Type="Edm.Int32" Nullable="false"/></EntityType><EntityType Name="Derived" BaseType="N.Base"/>"#,
            r#""Base":{"$Kind":"EntityType","$Key":["Id"],"Id":{"$Type":"Edm.Int32"}},"Derived":{"$Kind":"EntityType","$BaseType":"N.Base"}"#,
            version,
            true,
        );
        valid(
            r#"<EntityType Name="Abstract" Abstract="true"/>"#,
            r#""Abstract":{"$Kind":"EntityType","$Abstract":true}"#,
            version,
            true,
        );
    }
}
