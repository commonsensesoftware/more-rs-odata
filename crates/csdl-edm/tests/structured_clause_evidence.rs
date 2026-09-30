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
fn member_redeclarations_require_narrowing_and_401() {
    for navigation in [false, true] {
        let target_kind = if navigation {
            "EntityType"
        } else {
            "ComplexType"
        };
        let member_kind = if navigation {
            "NavigationProperty"
        } else {
            "Property"
        };
        for owner_kind in ["EntityType", "ComplexType"] {
            for version in ["4.0", "4.01"] {
                for target in ["Specific", "Target", "Unrelated"] {
                    let owner_abstract_xml = if owner_kind == "EntityType" {
                        r#" Abstract="true""#
                    } else {
                        ""
                    };
                    let owner_abstract_json = if owner_kind == "EntityType" {
                        r#", "$Abstract":true"#
                    } else {
                        ""
                    };
                    let target_abstract_xml = if navigation {
                        r#" Abstract="true""#
                    } else {
                        ""
                    };
                    let target_abstract_json = if navigation {
                        r#", "$Abstract":true"#
                    } else {
                        ""
                    };
                    let xml = format!(
                        r#"<{target_kind} Name="Target"{target_abstract_xml}/><{target_kind} Name="Specific" BaseType="N.Target"{target_abstract_xml}/><{target_kind} Name="Unrelated"{target_abstract_xml}/><{owner_kind} Name="Base"{owner_abstract_xml}><{member_kind} Name="Value" Type="N.Target"/></{owner_kind}><{owner_kind} Name="Derived" BaseType="N.Base"{owner_abstract_xml}><{member_kind} Name="Value" Type="N.{target}"/></{owner_kind}>"#
                    );
                    let json = format!(
                        r#""Target":{{"$Kind":"{target_kind}"{target_abstract_json}}},"Specific":{{"$Kind":"{target_kind}","$BaseType":"N.Target"{target_abstract_json}}},"Unrelated":{{"$Kind":"{target_kind}"{target_abstract_json}}},"Base":{{"$Kind":"{owner_kind}"{owner_abstract_json},"Value":{{"$Kind":"{member_kind}","$Type":"N.Target"}}}},"Derived":{{"$Kind":"{owner_kind}","$BaseType":"N.Base"{owner_abstract_json},"Value":{{"$Kind":"{member_kind}","$Type":"N.{target}"}}}}"#
                    );
                    for document in documents(&xml, &json, version) {
                        let result = Resolver::resolve_document(document.edmx.unwrap())
                            .map_err(|e| format!("{e:?}"))
                            .and_then(|m| {
                                validator::validate_document(&m).map_err(|e| format!("{e:?}"))
                            });
                        assert_eq!(
                            result.is_ok(),
                            version == "4.01" && target == "Specific",
                            "{version} {owner_kind} {member_kind} {target}: {result:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn inherited_members_cannot_change_between_structural_and_navigation_kinds() {
    for (base_kind, derived_kind) in [
        ("Property", "NavigationProperty"),
        ("NavigationProperty", "Property"),
    ] {
        for owner_kind in ["EntityType", "ComplexType"] {
            let base_ty = if base_kind == "Property" {
                "Edm.String"
            } else {
                "N.Target"
            };
            let derived_ty = if derived_kind == "Property" {
                "Edm.String"
            } else {
                "N.Target"
            };
            let xml = format!(
                r#"<EntityType Name="Target"/><{owner_kind} Name="Base"><{base_kind} Name="Value" Type="{base_ty}"/></{owner_kind}><{owner_kind} Name="Derived" BaseType="N.Base"><{derived_kind} Name="Value" Type="{derived_ty}"/></{owner_kind}>"#
            );
            let json = format!(
                r#""Target":{{"$Kind":"EntityType"}},"Base":{{"$Kind":"{owner_kind}","Value":{{"$Kind":"{base_kind}","$Type":"{base_ty}"}}}},"Derived":{{"$Kind":"{owner_kind}","$BaseType":"N.Base","Value":{{"$Kind":"{derived_kind}","$Type":"{derived_ty}"}}}}"#
            );
            for document in documents(&xml, &json, "4.01") {
                assert!(Resolver::resolve_document(document.edmx.unwrap()).is_err());
            }
        }
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

#[test]
fn members_cannot_match_declaring_type_but_can_match_base_or_derived_names() {
    for kind in ["EntityType", "ComplexType"] {
        for (name, expected) in [("Value", true), ("Owner", false)] {
            valid(
                &format!(
                    r#"<{kind} Name="Owner"><Property Name="{name}" Type="Edm.String"/></{kind}>"#
                ),
                &format!(r#""Owner":{{"$Kind":"{kind}","{name}":{{}}}}"#),
                "4.01",
                expected,
            );
            valid(
                &format!(
                    r#"<EntityType Name="Target"/><{kind} Name="Owner"><NavigationProperty Name="{name}" Type="N.Target"/></{kind}>"#
                ),
                &format!(
                    r#""Target":{{"$Kind":"EntityType"}},"Owner":{{"$Kind":"{kind}","{name}":{{"$Kind":"NavigationProperty","$Type":"N.Target"}}}}"#
                ),
                "4.01",
                expected,
            );
        }
        valid(
            &format!(
                r#"<{kind} Name="Base"><Property Name="Derived" Type="Edm.String"/></{kind}><{kind} Name="Derived" BaseType="N.Base"><Property Name="Base" Type="Edm.String"/></{kind}>"#
            ),
            &format!(
                r#""Base":{{"$Kind":"{kind}","Derived":{{}}}},"Derived":{{"$Kind":"{kind}","$BaseType":"N.Base","Base":{{}}}}"#
            ),
            "4.01",
            true,
        );
        valid(
            &format!(
                r#"<EntityType Name="Target"/><{kind} Name="Base"><NavigationProperty Name="Derived" Type="N.Target"/></{kind}><{kind} Name="Derived" BaseType="N.Base"><NavigationProperty Name="Base" Type="N.Target"/></{kind}>"#
            ),
            &format!(
                r#""Target":{{"$Kind":"EntityType"}},"Base":{{"$Kind":"{kind}","Derived":{{"$Kind":"NavigationProperty","$Type":"N.Target"}}}},"Derived":{{"$Kind":"{kind}","$BaseType":"N.Base","Base":{{"$Kind":"NavigationProperty","$Type":"N.Target"}}}}"#
            ),
            "4.01",
            true,
        );
        valid(
            &format!(
                r#"<ComplexType Name="C"/><ComplexType Name="Specific" BaseType="N.C"/><{kind} Name="Base"><Property Name="Derived" Type="N.C"/></{kind}><{kind} Name="Derived" BaseType="N.Base"><Property Name="Derived" Type="N.Specific"/></{kind}>"#
            ),
            &format!(
                r#""C":{{"$Kind":"ComplexType"}},"Specific":{{"$Kind":"ComplexType","$BaseType":"N.C"}},"Base":{{"$Kind":"{kind}","Derived":{{"$Type":"N.C"}}}},"Derived":{{"$Kind":"{kind}","$BaseType":"N.Base","Derived":{{"$Type":"N.Specific"}}}}"#
            ),
            "4.01",
            false,
        );
    }
}

#[test]
fn structural_and_navigation_members_share_a_unique_scope() {
    for kind in ["EntityType", "ComplexType"] {
        valid(
            &format!(
                r#"<EntityType Name="Target"/><{kind} Name="Owner"><Property Name="Value" Type="Edm.String"/><NavigationProperty Name="Related" Type="N.Target"/></{kind}>"#
            ),
            &format!(
                r#""Target":{{"$Kind":"EntityType"}},"Owner":{{"$Kind":"{kind}","Value":{{}},"Related":{{"$Kind":"NavigationProperty","$Type":"N.Target"}}}}"#
            ),
            "4.01",
            true,
        );
        for (xml_members, json_members) in [
            (
                r#"<Property Name="1Invalid" Type="Edm.String"/>"#,
                r#""1Invalid":{}"#,
            ),
            (
                r#"<NavigationProperty Name="1Invalid" Type="N.Target"/>"#,
                r#""1Invalid":{"$Kind":"NavigationProperty","$Type":"N.Target"}"#,
            ),
            (
                r#"<Property Name="Value" Type="Edm.String"/><Property Name="Value" Type="Edm.Int32"/>"#,
                r#""Value":{},"Value":{"$Type":"Edm.Int32"}"#,
            ),
            (
                r#"<Property Name="Value" Type="Edm.String"/><NavigationProperty Name="Value" Type="N.Target"/>"#,
                r#""Value":{},"Value":{"$Kind":"NavigationProperty","$Type":"N.Target"}"#,
            ),
        ] {
            let xml = format!(
                r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><EntityType Name="Target"/><{kind} Name="Owner">{xml_members}</{kind}></Schema></edmx:DataServices></edmx:Edmx>"#
            );
            let json = format!(
                r#"{{"$Version":"4.01","N":{{"Target":{{"$Kind":"EntityType"}},"Owner":{{"$Kind":"{kind}",{json_members}}}}}}}"#
            );
            assert!(parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).is_err());
            assert!(parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).is_err());
        }
    }
}
