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
fn reference_uris_use_document_identity_and_xml_base_precedence() {
    for (reference, expected) in [
        ("types", "https://example.test/models/types"),
        ("../types", "https://example.test/types"),
        ("./vocab/../types", "https://example.test/models/types"),
        ("/vocab/types", "https://example.test/vocab/types"),
        ("https://other.test/types", "https://other.test/types"),
        ("urn:example:types", "urn:example:types"),
    ] {
        let source_xml = xml(
            &format!(
                r#"<edmx:Reference Uri="{reference}"><edmx:Include Namespace="Types"/></edmx:Reference>"#
            ),
            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Root"/>"#,
        );
        let source_json = format!(
            r#"{{"$Version":"4.01","$Reference":{{"{reference}":{{"$Include":[{{"$Namespace":"Types"}}]}}}},"Root":{{}}}}"#
        );
        for document in [
            parser::from_xml_reader_with_profile(source_xml.as_bytes(), P::Strict).unwrap(),
            parser::from_json_reader_with_profile(source_json.as_bytes(), P::Strict).unwrap(),
        ] {
            resolve_reference(document, expected);
        }
    }
    for (document_base, reference_base, reference, expected) in [
        ("../vocab/", "", "types", "https://example.test/vocab/types"),
        (
            "../vocab/",
            "v1/",
            "types",
            "https://example.test/vocab/v1/types",
        ),
        (
            "../vocab/",
            "https://other.test/v2/",
            "types",
            "https://other.test/v2/types",
        ),
        (
            "../vocab/",
            "v1/",
            "https://third.test/types",
            "https://third.test/types",
        ),
    ] {
        let source = xml(&format!(r#"<edmx:Reference xml:base="{reference_base}" Uri="{reference}"><edmx:Include Namespace="Types"/></edmx:Reference>"#), r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Root"/>"#).replace(r#"Version="4.01""#, &format!(r#"Version="4.01" xml:base="{document_base}""#));
        resolve_reference(
            parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict).unwrap(),
            expected,
        );
    }

    fn resolve_reference(document: csdl_edm::CsdlDocument, expected: &str) {
        let external = parser::from_json_reader_with_profile(
            br#"{"$Version":"4.01","Types":{}}"#.as_slice(),
            P::Strict,
        )
        .unwrap();
        let mut calls = 0;
        let model = Resolver::resolve_with_loader(
            document,
            "https://example.test/models/root",
            &mut |uri: &str| {
                calls += 1;
                assert_eq!(uri, expected);
                Ok(external.clone())
            },
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(model.referenced_schemas.len(), 1);
        assert_eq!(model.referenced_schemas[0].namespace, "Types");
        csdl_edm::validator::validate_document(&model).unwrap();
    }
}

#[test]
fn schema_namespaces_cannot_be_split_across_reference_documents() {
    for split in [false, true] {
        for json_format in [false, true] {
            let parse = |source: String| {
                if json_format {
                    parser::from_json_reader_with_profile(source.as_bytes(), P::Strict).unwrap()
                } else {
                    parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict).unwrap()
                }
            };
            let root = parse(if json_format {
                r#"{"$Version":"4.01","$Reference":{"left":{"$Include":[{"$Namespace":"Left"}]},"right":{"$Include":[{"$Namespace":"Right"}]}},"Root":{}}"#.into()
            } else {
                xml(
                    r#"<edmx:Reference Uri="left"><edmx:Include Namespace="Left"/></edmx:Reference><edmx:Reference Uri="right"><edmx:Include Namespace="Right"/></edmx:Reference>"#,
                    r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Root"/>"#,
                )
            });
            let branch = |namespace: &str, reference: &str, member: &str| {
                parse(if json_format {
                    format!(
                        r#"{{"$Version":"4.01","$Reference":{{"{reference}":{{"$Include":[{{"$Namespace":"Shared"}}]}}}},"{namespace}":{{"Owner":{{"$Kind":"ComplexType","Value":{{"$Type":"Shared.{member}"}}}}}}}}"#
                    )
                } else {
                    xml(
                        &format!(
                            r#"<edmx:Reference Uri="{reference}"><edmx:Include Namespace="Shared"/></edmx:Reference>"#
                        ),
                        &format!(
                            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="{namespace}"><ComplexType Name="Owner"><Property Name="Value" Type="Shared.{member}"/></ComplexType></Schema>"#
                        ),
                    )
                })
            };
            let shared = |member: &str| {
                parse(if json_format {
                    format!(
                        r#"{{"$Version":"4.01","Shared":{{"{member}":{{"$Kind":"ComplexType"}}}}}}"#
                    )
                } else {
                    xml(
                        "",
                        &format!(
                            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Shared"><ComplexType Name="{member}"/></Schema>"#
                        ),
                    )
                })
            };
            let left = branch("Left", "shared", "First");
            let right = branch(
                "Right",
                if split { "other" } else { "shared" },
                if split { "Second" } else { "First" },
            );
            let first = shared("First");
            let second = shared("Second");
            let result = Resolver::resolve_with_loader(
                root,
                "https://example.test/root",
                &mut |uri: &str| {
                    Ok(match uri {
                        "https://example.test/left" => left.clone(),
                        "https://example.test/right" => right.clone(),
                        "https://example.test/shared" => first.clone(),
                        "https://example.test/other" => second.clone(),
                        _ => panic!("unexpected reference {uri}"),
                    })
                },
            );
            if split {
                assert!(
                    format!("{:?}", result.unwrap_err()).contains("defined by multiple documents")
                );
            } else {
                let model = result.unwrap();
                csdl_edm::validator::validate_document(&model).unwrap();
                assert_eq!(
                    model
                        .all_schemas()
                        .filter(|s| s.namespace == "Shared")
                        .count(),
                    1
                );
            }
        }
    }
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
