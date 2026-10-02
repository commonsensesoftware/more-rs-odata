#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

#[test]
fn property_path_context_depends_on_embedded_or_external_annotation() {
    for external in [false, true] {
        for (path, ty, embedded_valid, external_valid) in [
            ("Local", "Edm.String", true, false),
            ("RootValue", "Edm.String", false, true),
            ("Nested/Local", "Edm.String", false, true),
            ("Missing", "Edm.String", false, false),
            ("", "N.Inner", true, false),
            ("", "N.Outer", false, true),
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
            let xml = format!(
                r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{ty}"/><ComplexType Name="Inner"><Property Name="Local" Type="Edm.String">{inline_xml}</Property></ComplexType><ComplexType Name="Outer"><Property Name="RootValue" Type="Edm.String"/><Property Name="Nested" Type="N.Inner"/></ComplexType>{group_xml}</Schema></edmx:DataServices></edmx:Edmx>"#
            );
            let json = format!(
                r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"{ty}"}},"Inner":{{"$Kind":"ComplexType","Local":{{"$Nullable":true{inline_json}}}}},"Outer":{{"$Kind":"ComplexType","RootValue":{{"$Nullable":true}},"Nested":{{"$Type":"N.Inner","$Nullable":true}}}}{group_json}}}}}"#
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
                    if external {
                        external_valid
                    } else {
                        embedded_valid
                    },
                    "external={external}, {path}: {diagnostics:?}"
                );
                if !diagnostics.is_empty() && !path.is_empty() {
                    assert!(
                        diagnostics
                            .iter()
                            .any(|d| d.issue == validator::AnnotationIssue::InvalidExpressionPath),
                        "{diagnostics:?}"
                    );
                }
            }
        }
    }
}
