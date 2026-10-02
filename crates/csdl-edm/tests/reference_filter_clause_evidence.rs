#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{
    conformance::ValidationProfile as P, expr::CsdlAnnotationExpression as E, parser,
    resolver::Resolver,
};

fn xml(references: &str, schemas: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01">{references}<edmx:DataServices>{schemas}</edmx:DataServices></edmx:Edmx>"#
    )
}

#[test]
fn annotation_import_filters_apply_independently_and_in_combination() {
    let mut groups_xml = String::new();
    let mut groups_json = Vec::new();
    for target in ["Root", "Other"] {
        let mut annotations_xml = String::new();
        let mut annotations_json = Vec::new();
        for term in ["V", "W"] {
            for qualifier in [None, Some("Phone"), Some("Tablet")] {
                let marker = format!("{target}:{term}:{}", qualifier.unwrap_or("none"));
                let xml_qualifier = qualifier
                    .map(|q| format!(r#" Qualifier="{q}""#))
                    .unwrap_or_default();
                let json_qualifier = qualifier.map(|q| format!("#{q}")).unwrap_or_default();
                annotations_xml.push_str(&format!(
                    r#"<Annotation Term="{term}.Label"{xml_qualifier} String="{marker}"/>"#
                ));
                annotations_json.push(format!(r#""@{term}.Label{json_qualifier}":"{marker}""#));
            }
        }
        groups_xml.push_str(&format!(
            r#"<Annotations Target="{target}.Item">{annotations_xml}</Annotations>"#
        ));
        groups_json.push(format!(
            r#""{target}.Item":{{{}}}"#,
            annotations_json.join(",")
        ));
    }
    let external_xml = xml(
        r#"<edmx:Reference Uri="root"><edmx:Include Namespace="Root"/><edmx:Include Namespace="Other"/></edmx:Reference>"#,
        &format!(
            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="V"><Term Name="Label" Type="Edm.String"/>{groups_xml}</Schema><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="W"><Term Name="Label" Type="Edm.String"/></Schema>"#
        ),
    );
    let external_json = format!(
        r#"{{"$Version":"4.01","$Reference":{{"root":{{"$Include":[{{"$Namespace":"Root"}},{{"$Namespace":"Other"}}]}}}},"V":{{"Label":{{"$Kind":"Term"}},"$Annotations":{{{}}}}},"W":{{"Label":{{"$Kind":"Term"}}}}}}"#,
        groups_json.join(",")
    );
    for qualifier in [None, Some("Phone"), Some("Missing")] {
        for target in [None, Some("Root"), Some("Missing")] {
            let xml_qualifier = qualifier
                .map(|q| format!(r#" Qualifier="{q}""#))
                .unwrap_or_default();
            let xml_target = target
                .map(|t| format!(r#" TargetNamespace="{t}""#))
                .unwrap_or_default();
            let json_qualifier = qualifier
                .map(|q| format!(r#", "$Qualifier":"{q}""#))
                .unwrap_or_default();
            let json_target = target
                .map(|t| format!(r#", "$TargetNamespace":"{t}""#))
                .unwrap_or_default();
            let root_xml = xml(
                &format!(
                    r#"<edmx:Reference Uri="annotations"><edmx:IncludeAnnotations TermNamespace="V"{xml_qualifier}{xml_target}/></edmx:Reference>"#
                ),
                r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Root"><ComplexType Name="Item"/></Schema><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Other"><ComplexType Name="Item"/></Schema>"#,
            );
            let root_json = format!(
                r#"{{"$Version":"4.01","$Reference":{{"annotations":{{"$IncludeAnnotations":[{{"$TermNamespace":"V"{json_qualifier}{json_target}}}]}}}},"Root":{{"Item":{{"$Kind":"ComplexType"}}}},"Other":{{"Item":{{"$Kind":"ComplexType"}}}}}}"#
            );
            for (root, external) in [
                (
                    parser::from_xml_reader_with_profile(root_xml.as_bytes(), P::Strict).unwrap(),
                    parser::from_xml_reader_with_profile(external_xml.as_bytes(), P::Strict)
                        .unwrap(),
                ),
                (
                    parser::from_json_reader_with_profile(root_json.as_bytes(), P::Strict).unwrap(),
                    parser::from_json_reader_with_profile(external_json.as_bytes(), P::Strict)
                        .unwrap(),
                ),
            ] {
                let model = Resolver::resolve_with_loader(
                    root,
                    "https://example.test/root",
                    &mut |uri: &str| {
                        assert_eq!(uri, "https://example.test/annotations");
                        Ok(external.clone())
                    },
                )
                .unwrap();
                csdl_edm::validator::validate_document(&model).unwrap();
                let mut actual = model
                    .all_schemas()
                    .flat_map(|s| s.annotations.get().into_iter().flatten())
                    .map(|a| {
                        assert!(a.term.is_some());
                        let Some(E::String(marker)) = a.expression.as_ref() else {
                            panic!("lost annotation value")
                        };
                        marker.clone()
                    })
                    .collect::<Vec<_>>();
                let mut expected = Vec::new();
                for t in ["Root", "Other"] {
                    for q in [None, Some("Phone"), Some("Tablet")] {
                        if target.is_none_or(|filter| filter == t)
                            && qualifier.is_none_or(|filter| Some(filter) == q)
                        {
                            expected.push(format!("{t}:V:{}", q.unwrap_or("none")));
                        }
                    }
                }
                actual.sort();
                expected.sort();
                assert_eq!(actual, expected, "{qualifier:?} {target:?}");
            }
        }
    }
}
