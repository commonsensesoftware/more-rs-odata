#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

fn check(xml: &str, json: &str, expected: bool) {
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
        assert_eq!(result.is_ok(), expected, "{result:?}\n{xml}\n{json}");
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
