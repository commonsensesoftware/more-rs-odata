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
