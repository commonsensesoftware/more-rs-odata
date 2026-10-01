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
fn imports_require_unbound_operations_of_the_correct_kind_in_scope() {
    for kind in ["Action", "Function"] {
        for bound in [false, true] {
            for (target, found) in [("N.Run", true), ("Missing.Run", false), ("N.Other", false)] {
                let other = if kind == "Action" {
                    "Function"
                } else {
                    "Action"
                };
                check(
                    &format!(
                        r#"<{kind} Name="Run" IsBound="{bound}"><Parameter Name="binding" Type="Edm.String"/><ReturnType Type="Edm.String"/></{kind}><{other} Name="Other"><ReturnType Type="Edm.String"/></{other}><EntityContainer Name="Service"><{kind}Import Name="Imported" {kind}="{target}"/></EntityContainer>"#
                    ),
                    &format!(
                        r#""Run":[{{"$Kind":"{kind}","$IsBound":{bound},"$Parameter":[{{"$Name":"binding"}}],"$ReturnType":{{}}}}],"Other":[{{"$Kind":"{other}","$ReturnType":{{}}}}],"Service":{{"$Kind":"EntityContainer","Imported":{{"${kind}":"{target}"}}}}"#
                    ),
                    found && !bound,
                );
            }
        }
    }
}

#[test]
fn container_children_require_xml_names_and_types() {
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
        for (name, ty, valid) in [
            (true, true, true),
            (false, true, false),
            (true, false, false),
            (false, false, false),
        ] {
            let xml_name = if name { r#" Name="Items""# } else { "" };
            let xml_type = if ty {
                format!(r#" {attribute}="N.Entity""#)
            } else {
                String::new()
            };
            let json_name = if name { "Items" } else { "" };
            let json_type = if ty { r#""N.Entity""# } else { "null" };
            check(
                &format!(
                    r#"{XML_TYPES}<EntityContainer Name="Service"><{kind}{xml_name}{xml_type}/></EntityContainer>"#
                ),
                &format!(
                    r#"{JSON_TYPES},"Service":{{"$Kind":"EntityContainer","{json_name}":{{{marker}"$Type":{json_type}}}}}"#
                ),
                valid,
            );
        }
        check(
            &format!(
                r#"{XML_TYPES}<EntityContainer Name="Service"><{kind} Name="Items"/></EntityContainer>"#
            ),
            &format!(
                r#"{JSON_TYPES},"Service":{{"$Kind":"EntityContainer","Items":{{{marker}"@N.Note":"missing type"}}}}"#
            ),
            false,
        );
    }
}

#[test]
fn container_members_cannot_collide_with_operation_imports() {
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
        for operation in ["Action", "Function"] {
            for (name, valid) in [("Imported", true), ("Items", false)] {
                check(
                    &format!(
                        r#"{XML_TYPES}<{operation} Name="Run"><ReturnType Type="Edm.String"/></{operation}><EntityContainer Name="Service"><{kind} Name="Items" {attribute}="N.Entity"/><{operation}Import Name="{name}" {operation}="N.Run"/></EntityContainer>"#
                    ),
                    &format!(
                        r#"{JSON_TYPES},"Run":[{{"$Kind":"{operation}","$ReturnType":{{}}}}],"Service":{{"$Kind":"EntityContainer","Items":{{{marker}"$Type":"N.Entity"}},"{name}":{{"${operation}":"N.Run"}}}}"#
                    ),
                    valid,
                );
            }
        }
    }
}

#[test]
fn inherited_sets_and_singletons_can_only_narrow_their_entity_type() {
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
        for (ty, valid) in [("N.Derived", true), ("N.Entity", false), ("N.Other", false)] {
            check(
                &format!(
                    r#"{XML_TYPES}<EntityType Name="Derived" BaseType="N.Entity"/><EntityType Name="Other"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType><EntityContainer Name="Base"><{kind} Name="Items" {attribute}="N.Entity"/></EntityContainer><EntityContainer Name="Service" Extends="N.Base"><{kind} Name="Items" {attribute}="{ty}"/></EntityContainer>"#
                ),
                &format!(
                    r#"{JSON_TYPES},"Derived":{{"$Kind":"EntityType","$BaseType":"N.Entity"}},"Other":{{"$Kind":"EntityType","$Key":["ID"],"ID":{{"$Type":"Edm.Int32"}}}},"Base":{{"$Kind":"EntityContainer","Items":{{{marker}"$Type":"N.Entity"}}}},"Service":{{"$Kind":"EntityContainer","$Extends":"N.Base","Items":{{{marker}"$Type":"{ty}"}}}}"#
                ),
                valid,
            );
        }
    }
}

#[test]
fn inherited_container_children_cannot_change_kind_or_redeclare_imports() {
    for (xml_child, json_child) in [
        (
            r#"<Singleton Name="Items" Type="N.Entity"/>"#,
            r#""Items":{"$Type":"N.Entity"}"#,
        ),
        (
            r#"<ActionImport Name="Act" Action="N.Run"/>"#,
            r#""Act":{"$Action":"N.Run"}"#,
        ),
        (
            r#"<FunctionImport Name="Func" Function="N.Find"/>"#,
            r#""Func":{"$Function":"N.Find"}"#,
        ),
    ] {
        for redeclare in [false, true] {
            let xml_child = if redeclare { xml_child } else { "" };
            let json_child = if redeclare {
                format!(",{json_child}")
            } else {
                String::new()
            };
            check(
                &format!(
                    r#"{XML_TYPES}<Action Name="Run"/><Function Name="Find"><ReturnType Type="Edm.String"/></Function><EntityContainer Name="Base"><EntitySet Name="Items" EntityType="N.Entity"/><ActionImport Name="Act" Action="N.Run"/><FunctionImport Name="Func" Function="N.Find"/></EntityContainer><EntityContainer Name="Middle" Extends="N.Base"><Singleton Name="MiddleItem" Type="N.Entity"/></EntityContainer><EntityContainer Name="Service" Extends="N.Middle"><Singleton Name="ServiceItem" Type="N.Entity"/>{xml_child}</EntityContainer>"#
                ),
                &format!(
                    r#"{JSON_TYPES},"Run":[{{"$Kind":"Action"}}],"Find":[{{"$Kind":"Function","$ReturnType":{{}}}}],"Base":{{"$Kind":"EntityContainer","Items":{{"$Collection":true,"$Type":"N.Entity"}},"Act":{{"$Action":"N.Run"}},"Func":{{"$Function":"N.Find"}}}},"Middle":{{"$Kind":"EntityContainer","$Extends":"N.Base","MiddleItem":{{"$Type":"N.Entity"}}}},"Service":{{"$Kind":"EntityContainer","$Extends":"N.Middle","ServiceItem":{{"$Type":"N.Entity"}}{json_child}}}"#
                ),
                !redeclare,
            );
        }
    }
}

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
