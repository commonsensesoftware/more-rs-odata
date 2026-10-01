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
fn parameter_types_resolve_in_scope_for_scalar_and_collection_values() {
    for kind in ["Action", "Function"] {
        for collection in [false, true] {
            for (ty, valid) in [
                ("Edm.String", true),
                ("N.Entity", true),
                ("N.Complex", true),
                ("N.Choice", true),
                ("N.Code", true),
                ("Missing.Type", false),
                ("Edm.Unknown", false),
            ] {
                let xml_type = if collection {
                    format!("Collection({ty})")
                } else {
                    ty.to_owned()
                };
                check(
                    &format!(
                        r#"<EntityType Name="Entity"/><ComplexType Name="Complex"/><EnumType Name="Choice"><Member Name="One" Value="1"/></EnumType><TypeDefinition Name="Code" UnderlyingType="Edm.String"/><{kind} Name="Run"><Parameter Name="value" Type="{xml_type}" Nullable="false"/><ReturnType Type="Edm.String"/></{kind}>"#
                    ),
                    &format!(
                        r#""Entity":{{"$Kind":"EntityType"}},"Complex":{{"$Kind":"ComplexType"}},"Choice":{{"$Kind":"EnumType","One":1}},"Code":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.String"}},"Run":[{{"$Kind":"{kind}","$Parameter":[{{"$Name":"value","$Type":"{ty}","$Collection":{collection}}}],"$ReturnType":{{}}}}]"#
                    ),
                    valid,
                );
            }
        }
    }
}

#[test]
fn function_return_consistency_includes_collection_multiplicity() {
    for bound in [false, true] {
        for first_collection in [false, true] {
            for second_collection in [false, true] {
                let first_type = if first_collection {
                    "Collection(Edm.String)"
                } else {
                    "Edm.String"
                };
                let second_type = if second_collection {
                    "Collection(Edm.String)"
                } else {
                    "Edm.String"
                };
                let xml_binding = if bound {
                    r#"<Parameter Name="binding" Type="Edm.Boolean"/>"#
                } else {
                    ""
                };
                let json_binding = if bound {
                    r#"{"$Name":"binding","$Type":"Edm.Boolean"},"#
                } else {
                    ""
                };
                check(
                    &format!(
                        r#"<Function Name="Find" IsBound="{bound}">{xml_binding}<Parameter Name="code" Type="Edm.Int32"/><ReturnType Type="{first_type}" Nullable="false"/></Function><Function Name="Find" IsBound="{bound}">{xml_binding}<Parameter Name="text" Type="Edm.String"/><ReturnType Type="{second_type}" Nullable="false"/></Function>"#
                    ),
                    &format!(
                        r#""Find":[{{"$Kind":"Function","$IsBound":{bound},"$Parameter":[{json_binding}{{"$Name":"code","$Type":"Edm.Int32"}}],"$ReturnType":{{"$Collection":{first_collection}}}}},{{"$Kind":"Function","$IsBound":{bound},"$Parameter":[{json_binding}{{"$Name":"text"}}],"$ReturnType":{{"$Collection":{second_collection}}}}}]"#
                    ),
                    first_collection == second_collection,
                );
            }
        }
    }
}

#[test]
fn parameters_require_names_and_xml_types() {
    for kind in ["Action", "Function"] {
        for (xml_parameter, json_parameter, valid) in [
            (
                r#"Name="value" Type="Edm.String""#,
                r#"{"$Name":"value","$Type":"Edm.String"}"#,
                true,
            ),
            (r#"Type="Edm.String""#, r#"{"$Type":"Edm.String"}"#, false),
            (
                r#"Name="value""#,
                r#"{"$Name":"value","$Type":null}"#,
                false,
            ),
            ("", "{}", false),
        ] {
            check(
                &format!(
                    r#"<{kind} Name="Run"><Parameter {xml_parameter}/><ReturnType Type="Edm.String"/></{kind}>"#
                ),
                &format!(
                    r#""Run":[{{"$Kind":"{kind}","$Parameter":[{json_parameter}],"$ReturnType":{{}}}}]"#
                ),
                valid,
            );
        }
        // JSON deliberately defaults an omitted type to Edm.String.
        check(
            &format!(
                r#"<{kind} Name="Run"><Parameter Name="value" Type="Edm.String"/><ReturnType Type="Edm.String"/></{kind}>"#
            ),
            &format!(
                r#""Run":[{{"$Kind":"{kind}","$Parameter":[{{"$Name":"value"}}],"$ReturnType":{{}}}}]"#
            ),
            true,
        );
    }
}

#[test]
fn operation_entity_set_paths_start_at_binding_and_traverse_navigation_or_casts() {
    for kind in ["Action", "Function"] {
        for (path, valid) in [
            ("binding", true),
            ("binding/Related", true),
            ("binding/N.Derived", true),
            ("binding/N.Derived/Related", true),
            ("other", false),
            ("other/Related", false),
            ("binding/Text", false),
            ("binding/Missing", false),
            ("binding/N.Other", false),
            ("binding//Related", false),
        ] {
            let returned = if path == "binding/N.Derived" {
                "N.Derived"
            } else {
                "N.Entity"
            };
            check(
                &format!(
                    r#"<EntityType Name="Entity"><Property Name="Text" Type="Edm.String"/><NavigationProperty Name="Related" Type="N.Entity"/></EntityType><EntityType Name="Derived" BaseType="N.Entity"/><EntityType Name="Other"/><{kind} Name="Run" IsBound="true" EntitySetPath="{path}"><Parameter Name="binding" Type="N.Entity"/><Parameter Name="other" Type="N.Entity"/><ReturnType Type="{returned}"/></{kind}>"#
                ),
                &format!(
                    r#""Entity":{{"$Kind":"EntityType","Text":{{}},"Related":{{"$Kind":"NavigationProperty","$Type":"N.Entity"}}}},"Derived":{{"$Kind":"EntityType","$BaseType":"N.Entity"}},"Other":{{"$Kind":"EntityType"}},"Run":[{{"$Kind":"{kind}","$IsBound":true,"$EntitySetPath":"{path}","$Parameter":[{{"$Name":"binding","$Type":"N.Entity"}},{{"$Name":"other","$Type":"N.Entity"}}],"$ReturnType":{{"$Type":"{returned}"}}}}]"#
                ),
                valid,
            );
        }
    }
}

#[test]
fn operation_declarations_require_xml_names_and_json_kinds() {
    for kind in ["Action", "Function"] {
        for present in [false, true] {
            let xml_name = if present { r#" Name="Run""# } else { "" };
            let json_kind = if present {
                format!(r#""$Kind":"{kind}","#)
            } else {
                String::new()
            };
            check(
                &format!(r#"<{kind}{xml_name}><ReturnType Type="Edm.String"/></{kind}>"#),
                &format!(r#""Run":[{{{json_kind}"$ReturnType":{{}}}}]"#),
                present,
            );
        }
    }
}

#[test]
fn functions_require_exactly_one_return_declaration() {
    for (xml_return, json_return, valid) in [
        (
            r#"<ReturnType Type="Edm.String"/>"#,
            r#", "$ReturnType":{}"#,
            true,
        ),
        ("", "", false),
        (
            r#"<ReturnType Type="Edm.String"/><ReturnType Type="Edm.String"/>"#,
            r#", "$ReturnType":[]"#,
            false,
        ),
    ] {
        check(
            &format!(r#"<Function Name="Find">{xml_return}</Function>"#),
            &format!(r#""Find":[{{"$Kind":"Function"{json_return}}}]"#),
            valid,
        );
    }
    check(
        r#"<Action Name="Run"/>"#,
        r#""Run":[{"$Kind":"Action"}]"#,
        true,
    );
}

#[test]
fn operation_return_types_must_resolve_in_scope() {
    for kind in ["Action", "Function"] {
        for collection in [false, true] {
            for (ty, valid) in [
                ("Edm.String", true),
                ("N.Entity", true),
                ("N.Complex", true),
                ("Missing.Type", false),
                ("Edm.Unknown", false),
            ] {
                let xml_ty = if collection {
                    format!("Collection({ty})")
                } else {
                    ty.to_owned()
                };
                let xml_nullable = if collection && ty != "N.Entity" {
                    r#" Nullable="false""#
                } else {
                    ""
                };
                check(
                    &format!(
                        r#"<EntityType Name="Entity"/><ComplexType Name="Complex"/><{kind} Name="Run"><ReturnType Type="{xml_ty}"{xml_nullable}/></{kind}>"#
                    ),
                    &format!(
                        r#""Entity":{{"$Kind":"EntityType"}},"Complex":{{"$Kind":"ComplexType"}},"Run":[{{"$Kind":"{kind}","$ReturnType":{{"$Type":"{ty}","$Collection":{collection}}}}}]"#
                    ),
                    valid,
                );
            }
        }
    }
}

#[test]
fn entity_collection_returns_cannot_author_nullable() {
    for kind in ["Action", "Function"] {
        for nullable in [None, Some(false), Some(true)] {
            let xml_nullable = nullable
                .map(|b| format!(r#" Nullable="{b}""#))
                .unwrap_or_default();
            let json_nullable = nullable
                .map(|b| format!(r#", "$Nullable":{b}"#))
                .unwrap_or_default();
            check(
                &format!(
                    r#"<EntityType Name="Entity"/><{kind} Name="Run"><ReturnType Type="Collection(N.Entity)"{xml_nullable}/></{kind}>"#
                ),
                &format!(
                    r#""Entity":{{"$Kind":"EntityType"}},"Run":[{{"$Kind":"{kind}","$ReturnType":{{"$Type":"N.Entity","$Collection":true{json_nullable}}}}}]"#
                ),
                nullable.is_none(),
            );
        }
    }
}

#[test]
fn actions_overload_only_by_bound_parameter_type() {
    for bound in [false, true] {
        for (ty, collection, valid_bound) in [
            ("N.First", false, false),
            ("N.Second", false, true),
            ("N.First", true, true),
        ] {
            let xml_type = if collection {
                format!("Collection({ty})")
            } else {
                ty.to_owned()
            };
            check(
                &format!(
                    r#"<EntityType Name="First"/><EntityType Name="Second"/><Action Name="Run" IsBound="{bound}"><Parameter Name="binding" Type="N.First"/></Action><Action Name="Run" IsBound="{bound}"><Parameter Name="other" Type="{xml_type}"/><Parameter Name="extra" Type="Edm.Int32"/></Action>"#
                ),
                &format!(
                    r#""First":{{"$Kind":"EntityType"}},"Second":{{"$Kind":"EntityType"}},"Run":[{{"$Kind":"Action","$IsBound":{bound},"$Parameter":[{{"$Name":"binding","$Type":"N.First"}}]}},{{"$Kind":"Action","$IsBound":{bound},"$Parameter":[{{"$Name":"other","$Type":"{ty}","$Collection":{collection}}},{{"$Name":"extra","$Type":"Edm.Int32"}}]}}]"#
                ),
                bound && valid_bound,
            );
        }
    }
    check(
        r#"<Action Name="Run"/><Action Name="Other"/>"#,
        r#""Run":[{"$Kind":"Action"}],"Other":[{"$Kind":"Action"}]"#,
        true,
    );
}

#[test]
fn parameter_names_are_identifiers_unique_within_each_overload() {
    for kind in ["Action", "Function"] {
        for bound in [false, true] {
            for (name, valid) in [
                ("value", true),
                ("binding", false),
                ("1Invalid", false),
                ("", false),
            ] {
                let xml_return = if kind == "Function" {
                    r#"<ReturnType Type="Edm.String"/>"#
                } else {
                    ""
                };
                let json_return = if kind == "Function" {
                    r#", "$ReturnType":{}"#
                } else {
                    ""
                };
                check(
                    &format!(
                        r#"<{kind} Name="Run" IsBound="{bound}"><Parameter Name="binding" Type="Edm.String"/><Parameter Name="{name}" Type="Edm.Int32"/>{xml_return}</{kind}>"#
                    ),
                    &format!(
                        r#""Run":[{{"$Kind":"{kind}","$IsBound":{bound},"$Parameter":[{{"$Name":"binding"}},{{"$Name":"{name}","$Type":"Edm.Int32"}}]{json_return}}}]"#
                    ),
                    valid,
                );
            }
        }
    }
}

#[test]
fn function_overloads_require_distinct_names_and_ordered_types() {
    for bound in [false, true] {
        for (first_name, first_type, second_name, second_type, valid) in [
            ("a", "Edm.String", "b", "Edm.Int32", false),
            ("b", "Edm.String", "a", "Edm.Int32", false),
            ("x", "Edm.Int32", "y", "Edm.String", false),
            ("x", "Edm.String", "y", "Edm.Int32", true),
        ] {
            let xml_binding = if bound {
                r#"<Parameter Name="binding" Type="Edm.Boolean"/>"#
            } else {
                ""
            };
            let json_binding = if bound {
                r#"{"$Name":"binding","$Type":"Edm.Boolean"},"#
            } else {
                ""
            };
            check(
                &format!(
                    r#"<Function Name="Find" IsBound="{bound}">{xml_binding}<Parameter Name="a" Type="Edm.Int32"/><Parameter Name="b" Type="Edm.String"/><ReturnType Type="Edm.String"/></Function><Function Name="Find" IsBound="{bound}">{xml_binding}<Parameter Name="{first_name}" Type="{first_type}"/><Parameter Name="{second_name}" Type="{second_type}"/><ReturnType Type="Edm.String"/></Function>"#
                ),
                &format!(
                    r#""Find":[{{"$Kind":"Function","$IsBound":{bound},"$Parameter":[{json_binding}{{"$Name":"a","$Type":"Edm.Int32"}},{{"$Name":"b","$Type":"Edm.String"}}],"$ReturnType":{{}}}},{{"$Kind":"Function","$IsBound":{bound},"$Parameter":[{json_binding}{{"$Name":"{first_name}","$Type":"{first_type}"}},{{"$Name":"{second_name}","$Type":"{second_type}"}}],"$ReturnType":{{}}}}]"#
                ),
                valid,
            );
        }
    }
}

#[test]
fn function_return_types_agree_within_each_binding_group() {
    for bound in [false, true] {
        for different_binding in [false, true] {
            if !bound && different_binding {
                continue;
            }
            for return_type in ["Edm.String", "Edm.Boolean"] {
                let binding_type = if different_binding {
                    "Edm.Int32"
                } else {
                    "Edm.Boolean"
                };
                let xml_first = if bound {
                    r#"<Parameter Name="binding" Type="Edm.Boolean"/>"#
                } else {
                    ""
                };
                let json_first = if bound {
                    r#"{"$Name":"binding","$Type":"Edm.Boolean"},"#
                } else {
                    ""
                };
                let xml_second = if bound {
                    format!(r#"<Parameter Name="binding" Type="{binding_type}"/>"#)
                } else {
                    String::new()
                };
                let json_second = if bound {
                    format!(r#"{{"$Name":"binding","$Type":"{binding_type}"}},"#)
                } else {
                    String::new()
                };
                check(
                    &format!(
                        r#"<Function Name="Find" IsBound="{bound}">{xml_first}<Parameter Name="code" Type="Edm.Int32"/><ReturnType Type="Edm.String"/></Function><Function Name="Find" IsBound="{bound}">{xml_second}<Parameter Name="text" Type="Edm.String"/><ReturnType Type="{return_type}"/></Function>"#
                    ),
                    &format!(
                        r#""Find":[{{"$Kind":"Function","$IsBound":{bound},"$Parameter":[{json_first}{{"$Name":"code","$Type":"Edm.Int32"}}],"$ReturnType":{{"$Type":"Edm.String"}}}},{{"$Kind":"Function","$IsBound":{bound},"$Parameter":[{json_second}{{"$Name":"text","$Type":"Edm.String"}}],"$ReturnType":{{"$Type":"{return_type}"}}}}]"#
                    ),
                    different_binding || return_type == "Edm.String",
                );
            }
        }
    }
}
