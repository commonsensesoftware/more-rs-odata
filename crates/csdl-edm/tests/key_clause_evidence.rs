#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

fn check(xml: &str, json: &str, expected: bool) {
    check_version(xml, json, "4.01", expected)
}

fn check_version(xml: &str, json: &str, version: &str, expected: bool) {
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
        assert_eq!(result.is_ok(), expected, "{result:?}\n{xml}\n{json}");
    }
}

#[test]
fn singleton_and_scalar_navigation_targets_have_versioned_key_requirements() {
    for version in ["4.0", "4.01"] {
        for key in [false, true] {
            let xml_key = if key { r#"<Key><PropertyRef Name="Id"/></Key>"# } else { "" };
            let json_key = if key { r#", "$Key":["Id"]"# } else { "" };
            let target_xml = format!(r#"<EntityType Name="Target">{xml_key}<Property Name="Id" Type="Edm.Int32" Nullable="false"/></EntityType>"#);
            let target_json = format!(r#""Target":{{"$Kind":"EntityType"{json_key},"Id":{{"$Type":"Edm.Int32","$Nullable":false}}}}"#);
            check_version(
                &format!(r#"{target_xml}<EntityContainer Name="Service"><Singleton Name="Current" Type="N.Target"/></EntityContainer>"#),
                &format!(r#"{target_json},"Service":{{"$Kind":"EntityContainer","Current":{{"$Type":"N.Target"}}}}"#),
                version, key || version == "4.01",
            );
            for contained in [false, true] {
                check_version(
                    &format!(r#"{target_xml}<EntityType Name="Owner"><Key><PropertyRef Name="Id"/></Key><Property Name="Id" Type="Edm.Int32" Nullable="false"/><NavigationProperty Name="Related" Type="N.Target" ContainsTarget="{contained}"/></EntityType>"#),
                    &format!(r#"{target_json},"Owner":{{"$Kind":"EntityType","$Key":["Id"],"Id":{{"$Type":"Edm.Int32","$Nullable":false}},"Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$ContainsTarget":{contained}}}}}"#),
                    version, key || version == "4.01",
                );
            }
        }
    }
}

#[test]
fn key_primitive_types_nullability_and_collection_cardinality() {
    for ty in [
        "Boolean",
        "Byte",
        "Date",
        "DateTimeOffset",
        "Decimal",
        "Duration",
        "Guid",
        "Int16",
        "Int32",
        "Int64",
        "SByte",
        "String",
        "TimeOfDay",
        "Binary",
        "Single",
        "Double",
        "Stream",
    ] {
        for (nullable, collection) in [(false, false), (true, false), (false, true)] {
            let xml_ty = if collection {
                format!("Collection(Edm.{ty})")
            } else {
                format!("Edm.{ty}")
            };
            check(
                &format!(
                    r#"<EntityType Name="E"><Key><PropertyRef Name="Id"/></Key><Property Name="Id" Type="{xml_ty}" Nullable="{nullable}"/></EntityType>"#
                ),
                &format!(
                    r#""E":{{"$Kind":"EntityType","$Key":["Id"],"Id":{{"$Type":"Edm.{ty}","$Nullable":{nullable},"$Collection":{collection}}}}}"#
                ),
                !nullable
                    && !collection
                    && !matches!(ty, "Binary" | "Single" | "Double" | "Stream"),
            );
        }
    }
}

#[test]
fn enum_and_type_definition_keys_follow_underlying_type_rules() {
    check(
        r#"<EnumType Name="Choice"><Member Name="One" Value="1"/></EnumType><EntityType Name="E"><Key><PropertyRef Name="Id"/></Key><Property Name="Id" Type="N.Choice" Nullable="false"/></EntityType>"#,
        r#""Choice":{"$Kind":"EnumType","One":1},"E":{"$Kind":"EntityType","$Key":["Id"],"Id":{"$Type":"N.Choice"}}"#,
        true,
    );
    for (underlying, valid) in [
        ("String", true),
        ("Int32", true),
        ("Double", false),
        ("Binary", false),
    ] {
        check(
            &format!(
                r#"<TypeDefinition Name="Identifier" UnderlyingType="Edm.{underlying}"/><EntityType Name="E"><Key><PropertyRef Name="Id"/></Key><Property Name="Id" Type="N.Identifier" Nullable="false"/></EntityType>"#
            ),
            &format!(
                r#""Identifier":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.{underlying}"}},"E":{{"$Kind":"EntityType","$Key":["Id"],"Id":{{"$Type":"N.Identifier"}}}}"#
            ),
            valid,
        );
    }
}

#[test]
fn complex_key_paths_require_unique_aliases_and_nonnullable_single_values() {
    for (alias, nullable, collection, valid) in [
        (Some("ValueId"), false, false, true),
        (None, false, false, false),
        (Some("Value"), false, false, false),
        (Some("bad/name"), false, false, false),
        (Some("ValueId"), true, false, false),
        (Some("ValueId"), false, true, false),
    ] {
        let alias_xml = alias
            .map(|a| format!(r#" Alias="{a}""#))
            .unwrap_or_default();
        let key_json = alias
            .map(|a| serde_json::json!([{a:"Value/Id"}]))
            .unwrap_or_else(|| serde_json::json!(["Value/Id"]));
        let ty = if collection { "Collection(N.C)" } else { "N.C" };
        check(
            &format!(
                r#"<ComplexType Name="C"><Property Name="Id" Type="Edm.Int32" Nullable="false"/></ComplexType><EntityType Name="E"><Key><PropertyRef Name="Value/Id"{alias_xml}/></Key><Property Name="Value" Type="{ty}" Nullable="{nullable}"/></EntityType>"#
            ),
            &format!(
                r#""C":{{"$Kind":"ComplexType","Id":{{"$Type":"Edm.Int32"}}}},"E":{{"$Kind":"EntityType","$Key":{key_json},"Value":{{"$Type":"N.C","$Nullable":{nullable},"$Collection":{collection}}}}}"#
            ),
            valid,
        );
    }
}

#[test]
fn direct_key_properties_cannot_have_aliases_and_keys_cannot_be_empty() {
    check(
        r#"<EntityType Name="E"><Key><PropertyRef Name="Id" Alias="Other"/></Key><Property Name="Id" Type="Edm.Int32" Nullable="false"/></EntityType>"#,
        r#""E":{"$Kind":"EntityType","$Key":[{"Other":"Id"}],"Id":{"$Type":"Edm.Int32"}}"#,
        false,
    );
    check(
        r#"<EntityType Name="E"><Key><PropertyRef Name="Id"/></Key><Property Name="Id" Type="Edm.Int32" Nullable="false"/></EntityType>"#,
        r#""E":{"$Kind":"EntityType","$Key":["Id"],"Id":{"$Type":"Edm.Int32"}}"#,
        true,
    );
    check(
        r#"<EntityType Name="E"><Key/><Property Name="Id" Type="Edm.Int32" Nullable="false"/></EntityType>"#,
        r#""E":{"$Kind":"EntityType","$Key":[],"Id":{"$Type":"Edm.Int32"}}"#,
        false,
    );
}

#[test]
fn nested_complex_key_paths_check_nullability_at_every_level() {
    for outer in [true, false] {
        for inner in [true, false] {
            for leaf in [true, false] {
                check(
                    &format!(
                        r#"<ComplexType Name="Inner"><Property Name="Id" Type="Edm.Int32" Nullable="{leaf}"/></ComplexType><ComplexType Name="Outer"><Property Name="Child" Type="N.Inner" Nullable="{inner}"/></ComplexType><EntityType Name="E"><Key><PropertyRef Name="Value/Child/Id" Alias="K"/></Key><Property Name="Value" Type="N.Outer" Nullable="{outer}"/></EntityType>"#
                    ),
                    &format!(
                        r#""Inner":{{"$Kind":"ComplexType","Id":{{"$Type":"Edm.Int32","$Nullable":{leaf}}}}},"Outer":{{"$Kind":"ComplexType","Child":{{"$Type":"N.Inner","$Nullable":{inner}}}}},"E":{{"$Kind":"EntityType","$Key":[{{"K":"Value/Child/Id"}}],"Value":{{"$Type":"N.Outer","$Nullable":{outer}}}}}"#
                    ),
                    !outer && !inner && !leaf,
                );
            }
        }
    }
}

#[test]
fn related_entity_keys_require_all_members_and_nonnullable_single_navigation() {
    for (complete, nullable, collection, version, expected) in [
        (true, false, false, "4.01", true),
        (false, false, false, "4.01", false),
        (true, true, false, "4.01", false),
        (true, false, true, "4.01", false),
        (true, false, false, "4.0", false),
    ] {
        let second_xml = if complete {
            r#"<PropertyRef Name="Parent/B" Alias="ParentB"/>"#
        } else {
            ""
        };
        let second_json = if complete {
            r#",{"ParentB":"Parent/B"}"#
        } else {
            ""
        };
        let ty = if collection {
            "Collection(N.Parent)"
        } else {
            "N.Parent"
        };
        let xml_nullable = if collection {
            String::new()
        } else {
            format!(r#" Nullable="{nullable}""#)
        };
        let json_nullable = if collection {
            String::new()
        } else {
            format!(r#", "$Nullable":{nullable}"#)
        };
        check_version(
            &format!(
                r#"<EntityType Name="Parent"><Key><PropertyRef Name="A"/><PropertyRef Name="B"/></Key><Property Name="A" Type="Edm.Int32" Nullable="false"/><Property Name="B" Type="Edm.String" Nullable="false"/></EntityType><EntityType Name="Child"><Key><PropertyRef Name="Parent/A" Alias="ParentA"/>{second_xml}</Key><NavigationProperty Name="Parent" Type="{ty}"{xml_nullable}/></EntityType>"#
            ),
            &format!(
                r#""Parent":{{"$Kind":"EntityType","$Key":["A","B"],"A":{{"$Type":"Edm.Int32"}},"B":{{}}}},"Child":{{"$Kind":"EntityType","$Key":[{{"ParentA":"Parent/A"}}{second_json}],"Parent":{{"$Kind":"NavigationProperty","$Type":"N.Parent","$Collection":{collection}{json_nullable}}}}}"#
            ),
            version,
            expected,
        );
    }
    for complete in [true, false] {
        let second_xml = if complete {
            r#"<PropertyRef Name="Value/Related/B" Alias="ParentB"/>"#
        } else {
            ""
        };
        let second_json = if complete {
            r#",{"ParentB":"Value/Related/B"}"#
        } else {
            ""
        };
        check(
            &format!(
                r#"<EntityType Name="Parent"><Key><PropertyRef Name="A"/><PropertyRef Name="B"/></Key><Property Name="A" Type="Edm.Int32" Nullable="false"/><Property Name="B" Type="Edm.String" Nullable="false"/></EntityType><ComplexType Name="C"><NavigationProperty Name="Related" Type="N.Parent" Nullable="false"/></ComplexType><EntityType Name="Child"><Key><PropertyRef Name="Value/Related/A" Alias="ParentA"/>{second_xml}</Key><Property Name="Value" Type="N.C" Nullable="false"/></EntityType>"#
            ),
            &format!(
                r#""Parent":{{"$Kind":"EntityType","$Key":["A","B"],"A":{{"$Type":"Edm.Int32"}},"B":{{}}}},"C":{{"$Kind":"ComplexType","Related":{{"$Kind":"NavigationProperty","$Type":"N.Parent"}}}},"Child":{{"$Kind":"EntityType","$Key":[{{"ParentA":"Value/Related/A"}}{second_json}],"Value":{{"$Type":"N.C"}}}}"#
            ),
            complete,
        );
    }
}

#[test]
fn key_aliases_cannot_collide_with_inherited_properties_or_each_other() {
    for (alias, expected) in [("Good", true), ("Taken", false), ("Navi", false)] {
        check(
            &format!(
                r#"<ComplexType Name="C"><Property Name="Id" Type="Edm.Int32" Nullable="false"/></ComplexType><EntityType Name="Base"><Property Name="Taken" Type="Edm.String"/><NavigationProperty Name="Navi" Type="N.Base"/></EntityType><EntityType Name="E" BaseType="N.Base"><Key><PropertyRef Name="Value/Id" Alias="{alias}"/></Key><Property Name="Value" Type="N.C" Nullable="false"/></EntityType>"#
            ),
            &format!(
                r#""C":{{"$Kind":"ComplexType","Id":{{"$Type":"Edm.Int32"}}}},"Base":{{"$Kind":"EntityType","Taken":{{}},"Navi":{{"$Kind":"NavigationProperty","$Type":"N.Base"}}}},"E":{{"$Kind":"EntityType","$BaseType":"N.Base","$Key":[{{"{alias}":"Value/Id"}}],"Value":{{"$Type":"N.C"}}}}"#
            ),
            expected,
        );
    }
    for (second_alias, expected) in [("Second", true), ("First", false)] {
        check(
            &format!(
                r#"<ComplexType Name="C"><Property Name="A" Type="Edm.Int32" Nullable="false"/><Property Name="B" Type="Edm.String" Nullable="false"/></ComplexType><EntityType Name="E"><Key><PropertyRef Name="Value/A" Alias="First"/><PropertyRef Name="Value/B" Alias="{second_alias}"/></Key><Property Name="Value" Type="N.C" Nullable="false"/></EntityType>"#
            ),
            &format!(
                r#""C":{{"$Kind":"ComplexType","A":{{"$Type":"Edm.Int32"}},"B":{{}}}},"E":{{"$Kind":"EntityType","$Key":[{{"First":"Value/A"}},{{"{second_alias}":"Value/B"}}],"Value":{{"$Type":"N.C"}}}}"#
            ),
            expected,
        );
    }
}

#[test]
fn entity_sets_and_containment_collections_require_keys() {
    for has_key in [true, false] {
        let key_xml = if has_key {
            r#"<Key><PropertyRef Name="Id"/></Key>"#
        } else {
            ""
        };
        let key_json = if has_key { r#", "$Key":["Id"]"# } else { "" };
        for (xml_use, json_use, required) in [
            (
                r#"<EntityContainer Name="C"><EntitySet Name="Items" EntityType="N.E"/></EntityContainer>"#,
                r#""C":{"$Kind":"EntityContainer","Items":{"$Collection":true,"$Type":"N.E"}}"#,
                true,
            ),
            (
                r#"<EntityType Name="Source"><NavigationProperty Name="Items" Type="Collection(N.E)" ContainsTarget="true"/></EntityType>"#,
                r#""Source":{"$Kind":"EntityType","Items":{"$Kind":"NavigationProperty","$Type":"N.E","$Collection":true,"$ContainsTarget":true}}"#,
                true,
            ),
            (
                r#"<ComplexType Name="Source"><NavigationProperty Name="Items" Type="Collection(N.E)" ContainsTarget="true"/></ComplexType>"#,
                r#""Source":{"$Kind":"ComplexType","Items":{"$Kind":"NavigationProperty","$Type":"N.E","$Collection":true,"$ContainsTarget":true}}"#,
                true,
            ),
            (
                r#"<EntityType Name="Source"><NavigationProperty Name="Items" Type="Collection(N.E)" ContainsTarget="false"/></EntityType>"#,
                r#""Source":{"$Kind":"EntityType","Items":{"$Kind":"NavigationProperty","$Type":"N.E","$Collection":true,"$ContainsTarget":false}}"#,
                false,
            ),
            (
                r#"<EntityContainer Name="C"><Singleton Name="Item" Type="N.E"/></EntityContainer>"#,
                r#""C":{"$Kind":"EntityContainer","Item":{"$Type":"N.E"}}"#,
                false,
            ),
        ] {
            check(
                &format!(
                    r#"<EntityType Name="E">{key_xml}<Property Name="Id" Type="Edm.Int32" Nullable="false"/></EntityType>{xml_use}"#
                ),
                &format!(
                    r#""E":{{"$Kind":"EntityType"{key_json},"Id":{{"$Type":"Edm.Int32"}}}},{json_use}"#
                ),
                has_key || !required,
            );
            let inherited_xml = xml_use.replace("N.E", "N.Derived");
            let inherited_json = json_use.replace("N.E", "N.Derived");
            check(
                &format!(
                    r#"<EntityType Name="E">{key_xml}<Property Name="Id" Type="Edm.Int32" Nullable="false"/></EntityType><EntityType Name="Derived" BaseType="N.E"/>{inherited_xml}"#
                ),
                &format!(
                    r#""E":{{"$Kind":"EntityType"{key_json},"Id":{{"$Type":"Edm.Int32"}}}},"Derived":{{"$Kind":"EntityType","$BaseType":"N.E"}},{inherited_json}"#
                ),
                has_key || !required,
            );
        }
    }
}
