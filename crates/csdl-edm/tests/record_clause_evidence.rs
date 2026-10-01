#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

#[test]
fn base_term_annotations_supply_inherited_record_members() {
    for base_present in [false, true] {
        for own_present in [false, true] {
            let xml_base = if base_present {
                r#"<Annotation Term="N.BaseTerm"><Record><PropertyValue Property="BaseValue" String="base"/></Record></Annotation>"#
            } else {
                ""
            };
            let json_base = if base_present {
                r#", "@N.BaseTerm":{"BaseValue":"base"}"#
            } else {
                ""
            };
            let xml_own = if own_present {
                r#"<PropertyValue Property="OwnValue" String="own"/>"#
            } else {
                ""
            };
            let json_own = if own_present {
                r#""OwnValue":"own""#
            } else {
                ""
            };
            let xml = format!(
                r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Base"><Property Name="BaseValue" Type="Edm.String" Nullable="false"/></ComplexType><ComplexType Name="Derived" BaseType="N.Base"><Property Name="OwnValue" Type="Edm.String" Nullable="false"/></ComplexType><Term Name="BaseTerm" Type="N.Base"/><Term Name="DerivedTerm" Type="N.Derived" BaseTerm="N.BaseTerm"/><ComplexType Name="Owner">{xml_base}<Annotation Term="N.DerivedTerm"><Record>{xml_own}</Record></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
            );
            let json = format!(
                r#"{{"$Version":"4.01","N":{{"Base":{{"$Kind":"ComplexType","BaseValue":{{}}}},"Derived":{{"$Kind":"ComplexType","$BaseType":"N.Base","OwnValue":{{}}}},"BaseTerm":{{"$Kind":"Term","$Type":"N.Base"}},"DerivedTerm":{{"$Kind":"Term","$Type":"N.Derived","$BaseTerm":"N.BaseTerm"}},"Owner":{{"$Kind":"ComplexType"{json_base},"@N.DerivedTerm":{{{json_own}}}}}}}}}"#
            );
            for parsed in [
                parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
            ] {
                let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                validator::validate_document(&model).unwrap();
                let diagnostics = validator::validate_annotations(&model);
                assert_eq!(
                    diagnostics.is_empty(),
                    base_present && own_present,
                    "{diagnostics:?}: base={base_present}, own={own_present}"
                );
            }
        }
    }
}

#[test]
fn record_type_is_inferred_or_resolves_to_a_compatible_structured_type() {
    for (ty, valid) in [
        (None, true),
        (Some("N.Value"), true),
        (Some("N.Derived"), true),
        (Some("Edm.String"), false),
        (Some("Missing.Value"), false),
        (Some("N.Owner"), false),
    ] {
        let xml_type = ty.map(|t| format!(r#" Type="{t}""#)).unwrap_or_default();
        let json_type = ty
            .map(|t| format!(r##""@type":"#{t}","##))
            .unwrap_or_default();
        check(
            &format!(
                r#"<Record{xml_type}><PropertyValue Property="Required" String="value"/></Record>"#
            ),
            &format!(r#"{{{json_type}"Required":"value"}}"#),
            valid,
        );
    }
}

#[test]
fn record_navigation_members_are_required_only_when_single_and_nonnullable() {
    for collection in [false, true] {
        for nullable in [false, true] {
            if collection && nullable {
                continue;
            }
            for supplied in [false, true] {
                let target = if collection {
                    "Collection(N.Target)"
                } else {
                    "N.Target"
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
                let xml_value = if !supplied {
                    ""
                } else if collection {
                    r#"<PropertyValue Property="Related"><Collection><Record/></Collection></PropertyValue>"#
                } else {
                    r#"<PropertyValue Property="Related"><Record/></PropertyValue>"#
                };
                let json_value = if !supplied {
                    ""
                } else if collection {
                    r#""Related":[{}]"#
                } else {
                    r#""Related":{}"#
                };
                let xml = format!(
                    r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><EntityType Name="Target"/><ComplexType Name="Value"><NavigationProperty Name="Related" Type="{target}"{xml_nullable}/></ComplexType><Term Name="Term" Type="N.Value"/><ComplexType Name="Owner"><Annotation Term="N.Term"><Record>{xml_value}</Record></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
                );
                let json = format!(
                    r#"{{"$Version":"4.01","N":{{"Target":{{"$Kind":"EntityType"}},"Value":{{"$Kind":"ComplexType","Related":{{"$Kind":"NavigationProperty","$Type":"N.Target","$Collection":{collection}{json_nullable}}}}},"Term":{{"$Kind":"Term","$Type":"N.Value"}},"Owner":{{"$Kind":"ComplexType","@N.Term":{{{json_value}}}}}}}}}"#
                );
                for parsed in [
                    parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                    parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                ] {
                    let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                    validator::validate_document(&model).unwrap();
                    let diagnostics = validator::validate_annotations(&model);
                    assert_eq!(
                        diagnostics.is_empty(),
                        supplied || collection || nullable,
                        "{diagnostics:?}: collection={collection}, nullable={nullable}, supplied={supplied}"
                    );
                }
            }
        }
    }
}

fn check(xml_record: &str, json_record: &str, valid: bool) {
    check_kind(xml_record, json_record, "ComplexType", valid);
}

fn check_kind(xml_record: &str, json_record: &str, kind: &str, valid: bool) {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><{kind} Name="Value"><Property Name="Required" Type="Edm.String" Nullable="false"/><Property Name="Optional" Type="Edm.String"/><Property Name="Defaulted" Type="Edm.String" Nullable="false" DefaultValue="default"/></{kind}><{kind} Name="Derived" BaseType="N.Value"/><Term Name="Term" Type="N.Value"/><ComplexType Name="Owner"><Annotation Term="N.Term">{xml_record}</Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(
        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"{kind}","Required":{{}},"Optional":{{"$Nullable":true}},"Defaulted":{{"$DefaultValue":"default"}}}},"Derived":{{"$Kind":"{kind}","$BaseType":"N.Value"}},"Term":{{"$Kind":"Term","$Type":"N.Value"}},"Owner":{{"$Kind":"ComplexType","@N.Term":{json_record}}}}}}}"#
    );
    for parsed in [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
    ] {
        let result = parsed
            .map_err(|e| e.to_string())
            .and_then(|d| Resolver::resolve_document(d.edmx.unwrap()).map_err(|e| format!("{e:?}")))
            .and_then(|m| {
                validator::validate_document(&m).map_err(|e| format!("{e:?}"))?;
                let diagnostics = validator::validate_annotations(&m);
                if diagnostics.is_empty() {
                    Ok(())
                } else {
                    Err(format!("{diagnostics:?}"))
                }
            });
        assert_eq!(
            result.is_ok(),
            valid,
            "{result:?}\n{xml_record}\n{json_record}"
        );
    }
}

#[test]
fn entity_records_accept_context_and_compatible_explicit_types() {
    for (ty, valid) in [
        (None, true),
        (Some("N.Value"), true),
        (Some("N.Derived"), true),
        (Some("N.Owner"), false),
        (Some("Edm.String"), false),
        (Some("Missing.Entity"), false),
    ] {
        let xml_type = ty.map(|t| format!(r#" Type="{t}""#)).unwrap_or_default();
        let json_type = ty
            .map(|t| format!(r##""@type":"#{t}","##))
            .unwrap_or_default();
        check_kind(
            &format!(
                r#"<Record{xml_type}><PropertyValue Property="Required" String="value"/></Record>"#
            ),
            &format!(r#"{{{json_type}"Required":"value"}}"#),
            "EntityType",
            valid,
        );
    }
}

#[test]
fn records_require_nonnullable_members_without_defaults() {
    for (xml, json, valid) in [
        (
            r#"<Record><PropertyValue Property="Required" String="value"/></Record>"#,
            r#"{"Required":"value"}"#,
            true,
        ),
        ("<Record/>", "{}", false),
        (
            r#"<Record><PropertyValue Property="Required"><Null/></PropertyValue></Record>"#,
            r#"{"Required":null}"#,
            false,
        ),
        (
            r#"<Record><PropertyValue Property="Required" Int="1"/></Record>"#,
            r#"{"Required":1}"#,
            false,
        ),
        (
            r#"<Record><PropertyValue Property="Required" String="value"/><PropertyValue Property="Required" String="other"/></Record>"#,
            r#"{"Required":"value","Required":"other"}"#,
            false,
        ),
        (
            r#"<Record><PropertyValue Property="Required" String="value"/><PropertyValue Property="Unknown" String="other"/></Record>"#,
            r#"{"Required":"value","Unknown":"other"}"#,
            false,
        ),
    ] {
        check(xml, json, valid);
    }
}

#[test]
fn property_values_require_property_and_one_expression() {
    check(
        r#"<Record><PropertyValue Property="Required"><String>value</String></PropertyValue></Record>"#,
        r#"{"Required":"value"}"#,
        true,
    );
    for value in [
        r#"<PropertyValue String="value"/>"#,
        r#"<PropertyValue Property="Required"/>"#,
        r#"<PropertyValue Property="Required"><String>a</String><String>b</String></PropertyValue>"#,
    ] {
        check(
            &format!("<Record>{value}</Record>"),
            r#"{"Required":17}"#,
            false,
        );
    }
}
