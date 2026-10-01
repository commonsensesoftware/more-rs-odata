#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

fn check(xml: &str, json: &str, valid: bool) {
    check_version(xml, json, "4.01", valid);
}

fn check_version(xml: &str, json: &str, version: &str, valid: bool) {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="{version}"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{xml}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(r#"{{"$Version":"{version}","N":{{{json}}}}}"#);
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
fn import_names_are_identifiers_and_unique_across_operation_kinds() {
    for first in ["Action", "Function"] {
        for second in ["Action", "Function"] {
            let first_target = if first == "Action" { "N.Run" } else { "N.Find" };
            let second_target = if second == "Action" {
                "N.Run"
            } else {
                "N.Find"
            };
            for (name, valid) in [
                ("Other", true),
                ("Imported", false),
                ("1Invalid", false),
                ("", false),
            ] {
                check(
                    &format!(
                        r#"<Action Name="Run"/><Function Name="Find"><ReturnType Type="Edm.String"/></Function><EntityContainer Name="Service"><{first}Import Name="Imported" {first}="{first_target}"/><{second}Import Name="{name}" {second}="{second_target}"/></EntityContainer>"#
                    ),
                    &format!(
                        r#""Run":[{{"$Kind":"Action"}}],"Find":[{{"$Kind":"Function","$ReturnType":{{}}}}],"Service":{{"$Kind":"EntityContainer","Imported":{{"${first}":"{first_target}"}},"{name}":{{"${second}":"{second_target}"}}}}"#
                    ),
                    valid,
                );
            }
        }
    }
}

#[test]
fn singleton_nullable_cannot_be_authored_in_40() {
    for version in ["4.0", "4.01"] {
        for nullable in [None, Some(false), Some(true)] {
            let xml_nullable = nullable
                .map(|v| format!(r#" Nullable="{v}""#))
                .unwrap_or_default();
            let json_nullable = nullable
                .map(|v| format!(r#", "$Nullable":{v}"#))
                .unwrap_or_default();
            check_version(
                &format!(
                    r#"{XML_TYPES}<EntityContainer Name="Service"><Singleton Name="Item" Type="N.Entity"{xml_nullable}/></EntityContainer>"#
                ),
                &format!(
                    r#"{JSON_TYPES},"Service":{{"$Kind":"EntityContainer","Item":{{"$Type":"N.Entity"{json_nullable}}}}}"#
                ),
                version,
                version == "4.01" || nullable.is_none(),
            );
        }
    }
}

#[test]
fn json_singleton_nullable_defaults_false_and_requires_a_boolean() {
    for (value, expected) in [
        (None, Some(false)),
        (Some("false"), Some(false)),
        (Some("true"), Some(true)),
        (Some("null"), None),
        (Some("1"), None),
        (Some(r#""true""#), None),
    ] {
        let member = value
            .map(|v| format!(r#", "$Nullable":{v}"#))
            .unwrap_or_default();
        let json = format!(
            r#"{{"$Version":"4.01","N":{{{JSON_TYPES},"Service":{{"$Kind":"EntityContainer","Item":{{"$Type":"N.Entity"{member}}}}}}}}}"#
        );
        let parsed = parser::from_json_reader_with_profile(json.as_bytes(), P::Strict);
        let Some(expected) = expected else {
            assert!(parsed.is_err(), "{json}");
            continue;
        };
        let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
        validator::validate_document(&model).unwrap();
        let csdl_edm::edm::EntityContainerElement::Singleton(item) =
            model.schemas[0].containers()[0].elements[0].as_ref()
        else {
            panic!("expected singleton");
        };
        assert_eq!(item.nullable.unwrap_or(false), expected);
    }
}

#[test]
fn imports_require_names_and_operation_references() {
    for kind in ["Action", "Function"] {
        for name in [false, true] {
            for reference in [false, true] {
                let xml_name = if name { r#" Name="Imported""# } else { "" };
                let xml_reference = if reference {
                    format!(r#" {kind}="N.Run""#)
                } else {
                    String::new()
                };
                let json_name = if name { "Imported" } else { "" };
                let json_reference = if reference { r#""N.Run""# } else { "null" };
                check(
                    &format!(
                        r#"<{kind} Name="Run"><ReturnType Type="Edm.String"/></{kind}><EntityContainer Name="Service"><{kind}Import{xml_name}{xml_reference}/></EntityContainer>"#
                    ),
                    &format!(
                        r#""Run":[{{"$Kind":"{kind}","$ReturnType":{{}}}}],"Service":{{"$Kind":"EntityContainer","{json_name}":{{"${kind}":{json_reference}}}}}"#
                    ),
                    name && reference,
                );
            }
        }
        check(
            &format!(
                r#"<{kind} Name="Run"><ReturnType Type="Edm.String"/></{kind}><EntityContainer Name="Service"><{kind}Import Name="Imported"/></EntityContainer>"#
            ),
            &format!(
                r#""Run":[{{"$Kind":"{kind}","$ReturnType":{{}}}}],"Service":{{"$Kind":"EntityContainer","Imported":{{}}}}"#
            ),
            false,
        );
    }
}

const XML_TYPES: &str = r#"<EntityType Name="Entity"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType><ComplexType Name="Complex"/>"#;
const JSON_TYPES: &str = r#""Entity":{"$Kind":"EntityType","$Key":["ID"],"ID":{"$Type":"Edm.Int32"}},"Complex":{"$Kind":"ComplexType"}"#;

#[test]
fn import_entity_set_targets_resolve_to_sets_in_scope() {
    for kind in ["Action", "Function"] {
        for collection in [false, true] {
            let returned = if collection {
                "Collection(N.Entity)"
            } else {
                "N.Entity"
            };
            for (target, valid) in [
                ("Items", true),
                ("N.Other/Items", true),
                ("Missing", false),
                ("N.Missing/Items", false),
                ("Single", false),
            ] {
                check(
                    &format!(
                        r#"{XML_TYPES}<{kind} Name="Run"><ReturnType Type="{returned}"/></{kind}><EntityContainer Name="Other"><EntitySet Name="Items" EntityType="N.Entity"/></EntityContainer><EntityContainer Name="Service"><EntitySet Name="Items" EntityType="N.Entity"/><Singleton Name="Single" Type="N.Entity"/><{kind}Import Name="Imported" {kind}="N.Run" EntitySet="{target}"/></EntityContainer>"#
                    ),
                    &format!(
                        r#"{JSON_TYPES},"Run":[{{"$Kind":"{kind}","$ReturnType":{{"$Type":"N.Entity","$Collection":{collection}}}}}],"Other":{{"$Kind":"EntityContainer","Items":{{"$Collection":true,"$Type":"N.Entity"}}}},"Service":{{"$Kind":"EntityContainer","Items":{{"$Collection":true,"$Type":"N.Entity"}},"Single":{{"$Type":"N.Entity"}},"Imported":{{"${kind}":"N.Run","$EntitySet":"{target}"}}}}"#
                    ),
                    valid,
                );
            }
        }
    }
}
#[test]
fn function_import_exposes_all_unbound_overloads_and_excludes_bound_ones() {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Function Name="Find"><Parameter Name="code" Type="Edm.Int32"/><ReturnType Type="Edm.String"/></Function><Function Name="Find"><Parameter Name="text" Type="Edm.String"/><ReturnType Type="Edm.String"/></Function><Function Name="Find" IsBound="true"><Parameter Name="binding" Type="Edm.Boolean"/><ReturnType Type="Edm.String"/></Function><EntityContainer Name="Service"><FunctionImport Name="Imported" Function="N.Find"/></EntityContainer></Schema></edmx:DataServices></edmx:Edmx>"#;
    let json = r#"{"$Version":"4.01","N":{"Find":[{"$Kind":"Function","$Parameter":[{"$Name":"code","$Type":"Edm.Int32"}],"$ReturnType":{}},{"$Kind":"Function","$Parameter":[{"$Name":"text"}],"$ReturnType":{}},{"$Kind":"Function","$IsBound":true,"$Parameter":[{"$Name":"binding","$Type":"Edm.Boolean"}],"$ReturnType":{}}],"Service":{"$Kind":"EntityContainer","Imported":{"$Function":"N.Find"}}}}"#;
    for parsed in [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
    ] {
        let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
        validator::validate_document(&model).unwrap();
        let import = model.schemas[0].containers()[0]
            .elements
            .iter()
            .find_map(|e| match e.as_ref() {
                csdl_edm::edm::EntityContainerElement::FunctionImport(i) => Some(i),
                _ => None,
            })
            .unwrap();
        assert_eq!(import.overloads.len(), 2);
        let mut names: Vec<_> = import
            .overloads
            .iter()
            .map(|f| {
                assert!(!f.is_bound);
                f.parameters[0].name.as_str()
            })
            .collect();
        names.sort();
        assert_eq!(names, ["code", "text"]);
    }
}

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
