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

const XML_TYPES: &str = r#"<EntityType Name="Entity"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType><ComplexType Name="Complex"/>"#;
const JSON_TYPES: &str = r#""Entity":{"$Kind":"EntityType","$Key":["ID"],"ID":{"$Type":"Edm.Int32"}},"Complex":{"$Kind":"ComplexType"}"#;

#[test]
fn entity_sets_and_singletons_require_entity_types_in_scope() {
    for collection in [false, true] {
        let (kind, attribute) = if collection {
            ("EntitySet", "EntityType")
        } else {
            ("Singleton", "Type")
        };
        let marker = if collection {
            r#""$Collection":true,"#
        } else {
            ""
        };
        for (ty, valid) in [
            ("N.Entity", true),
            ("N.Complex", false),
            ("Edm.String", false),
            ("Missing.Entity", false),
        ] {
            check(
                &format!(
                    r#"{XML_TYPES}<EntityContainer Name="Service"><{kind} Name="Items" {attribute}="{ty}"/></EntityContainer>"#
                ),
                &format!(
                    r#"{JSON_TYPES},"Service":{{"$Kind":"EntityContainer","Items":{{{marker}"$Type":"{ty}"}}}}"#
                ),
                valid,
            );
        }
        check(
            &format!(
                r#"{XML_TYPES}<EntityContainer Name="Service"><{kind} Name="Items"/></EntityContainer>"#
            ),
            &format!(
                r#"{JSON_TYPES},"Service":{{"$Kind":"EntityContainer","Items":{{"$Type":null}}}}"#
            ),
            false,
        );
    }
}

#[test]
fn container_child_names_are_identifiers_unique_across_sets_and_singletons() {
    for first_collection in [false, true] {
        for second_collection in [false, true] {
            let (first_kind, first_attribute) = if first_collection {
                ("EntitySet", "EntityType")
            } else {
                ("Singleton", "Type")
            };
            let (second_kind, second_attribute) = if second_collection {
                ("EntitySet", "EntityType")
            } else {
                ("Singleton", "Type")
            };
            let first_marker = if first_collection {
                r#""$Collection":true,"#
            } else {
                ""
            };
            let second_marker = if second_collection {
                r#""$Collection":true,"#
            } else {
                ""
            };
            for (name, valid) in [
                ("Other", true),
                ("Items", false),
                ("1Invalid", false),
                ("", false),
            ] {
                check(
                    &format!(
                        r#"{XML_TYPES}<EntityContainer Name="Service"><{first_kind} Name="Items" {first_attribute}="N.Entity"/><{second_kind} Name="{name}" {second_attribute}="N.Entity"/></EntityContainer>"#
                    ),
                    &format!(
                        r#"{JSON_TYPES},"Service":{{"$Kind":"EntityContainer","Items":{{{first_marker}"$Type":"N.Entity"}},"{name}":{{{second_marker}"$Type":"N.Entity"}}}}"#
                    ),
                    valid,
                );
            }
        }
    }
}
