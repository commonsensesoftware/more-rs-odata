#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

#[test]
fn property_path_context_depends_on_embedded_or_external_annotation() {
    for entity in [false, true] {
        for navigation in [false, true] {
            for external in [false, true] {
                for (path, ty, embedded_valid, external_valid) in [
                    (
                        if navigation { "Local/Name" } else { "Local" },
                        "Edm.String",
                        true,
                        false,
                    ),
                    ("RootValue", "Edm.String", false, true),
                    (
                        if navigation {
                            "Nested/Local/Name"
                        } else {
                            "Nested/Local"
                        },
                        "Edm.String",
                        false,
                        true,
                    ),
                    ("Missing", "Edm.String", false, false),
                    ("", "N.Inner", true, false),
                    ("", "N.Outer", false, true),
                    ("N.InnerDerived/DerivedValue", "Edm.String", true, !entity),
                    ("N.OuterDerived/DerivedValue", "Edm.String", !entity, true),
                    ("N.Missing/DerivedValue", "Edm.String", false, false),
                    ("@N.Text", "Edm.String", true, true),
                    ("@N.Missing", "Edm.String", false, false),
                ] {
                    let annotation_xml =
                        format!(r#"<Annotation Term="N.Value"><Path>{path}</Path></Annotation>"#);
                    let annotation_json = format!(r#", "@N.Value":{{"$Path":"{path}"}}"#);
                    let (inline_xml, inline_json, group_xml, group_json) = if external {
                        (
                            String::new(),
                            String::new(),
                            format!(
                                r#"<Annotations Target="N.Outer/Nested/Local">{annotation_xml}</Annotations>"#
                            ),
                            format!(
                                r#", "$Annotations":{{"N.Outer/Nested/Local":{{"@N.Value":{{"$Path":"{path}"}}}}}}"#
                            ),
                        )
                    } else {
                        (
                            annotation_xml,
                            annotation_json,
                            String::new(),
                            String::new(),
                        )
                    };
                    let (member_xml, member_json) = if navigation {
                        (
                            format!(
                                r#"<NavigationProperty Name="Local" Type="N.Target">{inline_xml}</NavigationProperty>"#
                            ),
                            format!(
                                r#""$Kind":"NavigationProperty","$Type":"N.Target","$Nullable":true{inline_json}"#
                            ),
                        )
                    } else {
                        (
                            format!(
                                r#"<Property Name="Local" Type="Edm.String">{inline_xml}</Property>"#
                            ),
                            format!(r#""$Nullable":true{inline_json}"#),
                        )
                    };
                    let xml = format!(
                        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{ty}"/><Term Name="Text" Type="Edm.String"/><EntityType Name="Target" Abstract="true"><Property Name="Name" Type="Edm.String"/></EntityType><ComplexType Name="Inner">{member_xml}</ComplexType><ComplexType Name="Outer"><Property Name="RootValue" Type="Edm.String"/><Property Name="Nested" Type="N.Inner"/></ComplexType><ComplexType Name="InnerDerived" BaseType="N.Inner"><Property Name="DerivedValue" Type="Edm.String"/></ComplexType><ComplexType Name="OuterDerived" BaseType="N.Outer"><Property Name="DerivedValue" Type="Edm.String"/></ComplexType>{group_xml}</Schema></edmx:DataServices></edmx:Edmx>"#
                    );
                    let json = format!(
                        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"{ty}"}},"Text":{{"$Kind":"Term"}},"Target":{{"$Kind":"EntityType","$Abstract":true,"Name":{{"$Nullable":true}}}},"Inner":{{"$Kind":"ComplexType","Local":{{{member_json}}}}},"Outer":{{"$Kind":"ComplexType","RootValue":{{"$Nullable":true}},"Nested":{{"$Type":"N.Inner","$Nullable":true}}}},"InnerDerived":{{"$Kind":"ComplexType","$BaseType":"N.Inner","DerivedValue":{{"$Nullable":true}}}},"OuterDerived":{{"$Kind":"ComplexType","$BaseType":"N.Outer","DerivedValue":{{"$Nullable":true}}}}{group_json}}}}}"#
                    );
                    let xml = if entity {
                        xml.replace("<ComplexType Name=\"Inner\">", "<EntityType Name=\"Inner\" Abstract=\"true\">")
                    .replace(&format!("{member_xml}</ComplexType>"), &format!("{member_xml}</EntityType>"))
                    .replace("<ComplexType Name=\"InnerDerived\" BaseType=\"N.Inner\"><Property Name=\"DerivedValue\" Type=\"Edm.String\"/></ComplexType>", "<EntityType Name=\"InnerDerived\" BaseType=\"N.Inner\" Abstract=\"true\"><Property Name=\"DerivedValue\" Type=\"Edm.String\"/></EntityType>")
                    .replace("<Property Name=\"Nested\" Type=\"N.Inner\"/>", "<NavigationProperty Name=\"Nested\" Type=\"N.Inner\"/>")
                    } else {
                        xml
                    };
                    let json = if entity {
                        json.replace(
                            r#""Inner":{"$Kind":"ComplexType""#,
                            r#""Inner":{"$Kind":"EntityType","$Abstract":true"#,
                        )
                        .replace(
                            r#""InnerDerived":{"$Kind":"ComplexType""#,
                            r#""InnerDerived":{"$Kind":"EntityType","$Abstract":true"#,
                        )
                        .replace(
                            r#""Nested":{"$Type":"N.Inner""#,
                            r#""Nested":{"$Kind":"NavigationProperty","$Type":"N.Inner""#,
                        )
                    } else {
                        json
                    };
                    for parsed in [
                        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                    ] {
                        let model =
                            Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                        validator::validate_document(&model).unwrap();
                        let diagnostics = validator::validate_annotations(&model);
                        assert_eq!(
                            diagnostics.is_empty(),
                            if external {
                                external_valid
                            } else {
                                embedded_valid
                            },
                            "external={external}, {path}: {diagnostics:?}"
                        );
                        if !diagnostics.is_empty() && !path.is_empty() {
                            assert!(
                                diagnostics.iter().any(|d| d.issue
                                    == validator::AnnotationIssue::InvalidExpressionPath),
                                "{diagnostics:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
