#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

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
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Value"><Property Name="Required" Type="Edm.String" Nullable="false"/><Property Name="Optional" Type="Edm.String"/><Property Name="Defaulted" Type="Edm.String" Nullable="false" DefaultValue="default"/></ComplexType><ComplexType Name="Derived" BaseType="N.Value"/><Term Name="Term" Type="N.Value"/><ComplexType Name="Owner"><Annotation Term="N.Term">{xml_record}</Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(
        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"ComplexType","Required":{{}},"Optional":{{"$Nullable":true}},"Defaulted":{{"$DefaultValue":"default"}}}},"Derived":{{"$Kind":"ComplexType","$BaseType":"N.Value"}},"Term":{{"$Kind":"Term","$Type":"N.Value"}},"Owner":{{"$Kind":"ComplexType","@N.Term":{json_record}}}}}}}"#
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
