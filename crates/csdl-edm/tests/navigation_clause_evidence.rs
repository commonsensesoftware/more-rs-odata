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
fn navigation_targets_must_be_entity_types_in_scope() {
    for (ty, valid) in [
        ("N.Target", true),
        ("Edm.EntityType", true),
        ("N.Value", false),
        ("Edm.String", false),
        ("Missing.Target", false),
    ] {
        for collection in [true, false] {
            let xml_ty = if collection {
                format!("Collection({ty})")
            } else {
                ty.into()
            };
            check(
                &format!(
                    r#"<EntityType Name="Source"><NavigationProperty Name="Related" Type="{xml_ty}"/></EntityType><EntityType Name="Target"/><ComplexType Name="Value"/>"#
                ),
                &format!(
                    r#""Source":{{"$Kind":"EntityType","Related":{{"$Kind":"NavigationProperty","$Type":"{ty}","$Collection":{collection}}}}},"Target":{{"$Kind":"EntityType"}},"Value":{{"$Kind":"ComplexType"}}"#
                ),
                valid,
            );
        }
    }
}

#[test]
fn collection_navigation_must_not_author_nullable() {
    for nullable in [None, Some(false), Some(true)] {
        let xml_nullable = nullable
            .map(|b| format!(r#" Nullable="{b}""#))
            .unwrap_or_default();
        let json_nullable = nullable
            .map(|b| format!(r#", "$Nullable":{b}"#))
            .unwrap_or_default();
        check(
            &format!(
                r#"<EntityType Name="Source"><NavigationProperty Name="Related" Type="Collection(N.Target)"{xml_nullable}/></EntityType><EntityType Name="Target"/>"#
            ),
            &format!(
                r#""Source":{{"$Kind":"EntityType","Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$Collection":true{json_nullable}}}}},"Target":{{"$Kind":"EntityType"}}"#
            ),
            nullable.is_none(),
        );
    }
}

#[test]
fn complex_navigation_cannot_specify_partner() {
    for partner in [false, true] {
        let xml_partner = if partner { r#" Partner="Back""# } else { "" };
        let json_partner = if partner {
            r#", "$Partner":"Back""#
        } else {
            ""
        };
        check(
            &format!(
                r#"<ComplexType Name="C"><NavigationProperty Name="Related" Type="N.Target"{xml_partner}/></ComplexType><EntityType Name="Target"><NavigationProperty Name="Back" Type="N.Target"/></EntityType>"#
            ),
            &format!(
                r#""C":{{"$Kind":"ComplexType","Related":{{"$Kind":"NavigationProperty","$Type":"N.Target"{json_partner}}}}},"Target":{{"$Kind":"EntityType","Back":{{"$Kind":"NavigationProperty","$Type":"N.Target"}}}}"#
            ),
            !partner,
        );
    }
}

#[test]
fn referential_constraint_nullability_matches_navigation_or_principal() {
    for navigation in [true, false] {
        for principal in [true, false] {
            for dependent in [true, false] {
                check(
                    &format!(
                        r#"<EntityType Name="Source"><Property Name="ForeignKey" Type="Edm.Int32" Nullable="{dependent}"/><NavigationProperty Name="Related" Type="N.Target" Nullable="{navigation}"><ReferentialConstraint Property="ForeignKey" ReferencedProperty="Code"/></NavigationProperty></EntityType><EntityType Name="Target"><Property Name="Code" Type="Edm.Int32" Nullable="{principal}"/></EntityType>"#
                    ),
                    &format!(
                        r#""Source":{{"$Kind":"EntityType","ForeignKey":{{"$Type":"Edm.Int32","$Nullable":{dependent}}},"Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$Nullable":{navigation},"$ReferentialConstraint":{{"ForeignKey":"Code"}}}}}},"Target":{{"$Kind":"EntityType","Code":{{"$Type":"Edm.Int32","$Nullable":{principal}}}}}"#
                    ),
                    dependent == (navigation || principal),
                );
            }
        }
    }
}

#[test]
fn partner_declarations_are_reciprocal_when_present() {
    for partner in [None, Some("Related"), Some("Other")] {
        let xml_partner = partner
            .map(|p| format!(r#" Partner="{p}""#))
            .unwrap_or_default();
        let json_partner = partner
            .map(|p| format!(r#", "$Partner":"{p}""#))
            .unwrap_or_default();
        check(
            &format!(
                r#"<EntityType Name="Source"><NavigationProperty Name="Related" Type="N.Target" Partner="Back"/><NavigationProperty Name="Other" Type="N.Target"/></EntityType><EntityType Name="Target"><NavigationProperty Name="Back" Type="N.Source"{xml_partner}/></EntityType>"#
            ),
            &format!(
                r#""Source":{{"$Kind":"EntityType","Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$Partner":"Back"}},"Other":{{"$Kind":"NavigationProperty","$Type":"N.Target"}}}},"Target":{{"$Kind":"EntityType","Back":{{"$Kind":"NavigationProperty","$Type":"N.Source"{json_partner}}}}}"#
            ),
            partner != Some("Other"),
        );
    }
}

#[test]
fn partner_paths_allow_complex_and_derived_types_but_not_navigation_traversal() {
    for (path, valid) in [
        ("Back", true),
        ("N.Derived/Back", true),
        ("Details/Back", true),
        ("Details/N.Special/DerivedBack", true),
        ("Next/Back", false),
        ("Missing/Back", false),
        ("Code", false),
        ("N.Other/Back", false),
    ] {
        check(
            &format!(
                r#"<EntityType Name="Source"><NavigationProperty Name="Related" Type="N.Target" Partner="{path}"/></EntityType><EntityType Name="Target"><Property Name="Code" Type="Edm.String"/><Property Name="Details" Type="N.C"/><NavigationProperty Name="Back" Type="N.Source"/><NavigationProperty Name="Next" Type="N.Target"/></EntityType><EntityType Name="Derived" BaseType="N.Target"/><EntityType Name="Other"><NavigationProperty Name="Back" Type="N.Source"/></EntityType><ComplexType Name="C"><NavigationProperty Name="Back" Type="N.Source"/></ComplexType><ComplexType Name="Special" BaseType="N.C"><NavigationProperty Name="DerivedBack" Type="N.Source"/></ComplexType>"#
            ),
            &format!(
                r#""Source":{{"$Kind":"EntityType","Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$Partner":"{path}"}}}},"Target":{{"$Kind":"EntityType","Code":{{}},"Details":{{"$Type":"N.C"}},"Back":{{"$Kind":"NavigationProperty","$Type":"N.Source"}},"Next":{{"$Kind":"NavigationProperty","$Type":"N.Target"}}}},"Derived":{{"$Kind":"EntityType","$BaseType":"N.Target"}},"Other":{{"$Kind":"EntityType","Back":{{"$Kind":"NavigationProperty","$Type":"N.Source"}}}},"C":{{"$Kind":"ComplexType","Back":{{"$Kind":"NavigationProperty","$Type":"N.Source"}}}},"Special":{{"$Kind":"ComplexType","$BaseType":"N.C","DerivedBack":{{"$Kind":"NavigationProperty","$Type":"N.Source"}}}}"#
            ),
            valid,
        );
    }
}

#[test]
fn partner_target_must_be_source_type_or_its_base() {
    for (target, valid) in [("N.Source", true), ("N.Base", true), ("N.Target", false)] {
        check(
            &format!(
                r#"<EntityType Name="Base"/><EntityType Name="Source" BaseType="N.Base"><NavigationProperty Name="Related" Type="N.Target" Partner="Back"/></EntityType><EntityType Name="Target"><NavigationProperty Name="Back" Type="{target}"/></EntityType>"#
            ),
            &format!(
                r#""Base":{{"$Kind":"EntityType"}},"Source":{{"$Kind":"EntityType","$BaseType":"N.Base","Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$Partner":"Back"}}}},"Target":{{"$Kind":"EntityType","Back":{{"$Kind":"NavigationProperty","$Type":"{target}"}}}}"#
            ),
            valid,
        );
    }
}

#[test]
fn constraint_paths_resolve_nested_properties_on_the_correct_owner() {
    for (dependent, principal, valid) in [
        ("Details/Code", "Details/Code", true),
        ("Missing/Code", "Details/Code", false),
        ("Details/Code", "Missing/Code", false),
        ("Related/Details/Code", "Details/Code", false),
        ("Details/Code", "Next/Details/Code", false),
    ] {
        check(
            &format!(
                r#"<ComplexType Name="C"><Property Name="Code" Type="Edm.Int32" Nullable="false"/></ComplexType><EntityType Name="Source"><Property Name="Details" Type="N.C" Nullable="false"/><NavigationProperty Name="Related" Type="N.Target" Nullable="false"><ReferentialConstraint Property="{dependent}" ReferencedProperty="{principal}"/></NavigationProperty></EntityType><EntityType Name="Target"><Property Name="Details" Type="N.C" Nullable="false"/><NavigationProperty Name="Next" Type="N.Target"/></EntityType>"#
            ),
            &format!(
                r#""C":{{"$Kind":"ComplexType","Code":{{"$Type":"Edm.Int32"}}}},"Source":{{"$Kind":"EntityType","Details":{{"$Type":"N.C"}},"Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$ReferentialConstraint":{{"{dependent}":"{principal}"}}}}}},"Target":{{"$Kind":"EntityType","Details":{{"$Type":"N.C"}},"Next":{{"$Kind":"NavigationProperty","$Type":"N.Target"}}}}"#
            ),
            valid,
        );
    }
}

#[test]
fn constraint_types_must_match_unless_both_are_complex() {
    for (dependent, principal, valid) in [
        ("Edm.Int32", "Edm.Int32", true),
        ("Edm.String", "Edm.Int32", false),
        ("N.First", "N.Second", true),
        ("N.First", "Edm.String", false),
    ] {
        for owner in ["EntityType", "ComplexType"] {
            check(
                &format!(
                    r#"<ComplexType Name="First"/><ComplexType Name="Second"/><{owner} Name="Source"><Property Name="Value" Type="{dependent}" Nullable="false"/><NavigationProperty Name="Related" Type="N.Target" Nullable="false"><ReferentialConstraint Property="Value" ReferencedProperty="Value"/></NavigationProperty></{owner}><EntityType Name="Target"><Property Name="Value" Type="{principal}" Nullable="false"/></EntityType>"#
                ),
                &format!(
                    r#""First":{{"$Kind":"ComplexType"}},"Second":{{"$Kind":"ComplexType"}},"Source":{{"$Kind":"{owner}","Value":{{"$Type":"{dependent}"}},"Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$ReferentialConstraint":{{"Value":"Value"}}}}}},"Target":{{"$Kind":"EntityType","Value":{{"$Type":"{principal}"}}}}"#
                ),
                valid,
            );
        }
    }
}
