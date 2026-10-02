use csdl_edm::{conformance::ValidationProfile as P, parser};

#[test]
fn json_operation_annotations_survive_parsing_without_resolving() {
    for kind in ["Action", "Function"] {
        let source = format!(
            r#"{{"$Version":"4.01","N":{{"F":[{{"$Kind":"{kind}","$ReturnType":{{}},"@N.Label":"kept","@N.Path":{{"$Path":"$ReturnType"}}}}]}}}}"#
        );
        for profile in [P::Strict, P::Tolerant] {
            let document =
                parser::from_json_reader_with_profile(source.as_bytes(), profile).unwrap();
            let output = document.to_json_value();
            assert_eq!(output["N"]["F"][0]["@N.Label"], "kept");
            assert_eq!(output["N"]["F"][0]["@N.Path"]["$Path"], "$ReturnType");
        }
    }
}

fn xml(body: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.String"/><ComplexType Name="Owner"/>{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    )
}

#[test]
fn xml_annotations_require_a_term_and_allow_an_optional_qualifier() {
    for (attributes, valid) in [
        (r#"Term="N.Value""#, true),
        (r#"Term="N.Value" Qualifier="View""#, true),
        ("", false),
        (r#"Term="""#, false),
        (r#"Qualifier="View""#, false),
    ] {
        let source = xml(&format!(
            r#"<Annotations Target="N.Owner"><Annotation {attributes} String="value"/></Annotations>"#
        ));
        assert_eq!(
            parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict).is_ok(),
            valid,
            "{source}"
        );
    }
}

#[test]
fn xml_group_qualifier_cannot_be_redeclared_on_a_child_annotation() {
    for parent in [None, Some("View")] {
        for child in [None, Some("View"), Some("Other")] {
            let attribute = |value: Option<&str>| {
                value
                    .map(|q| format!(r#" Qualifier="{q}""#))
                    .unwrap_or_default()
            };
            let source = xml(&format!(
                r#"<Annotations Target="N.Owner"{}><Annotation Term="N.Value"{} String="value"/></Annotations>"#,
                attribute(parent),
                attribute(child)
            ));
            let parsed = parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict);
            assert_eq!(
                parsed.is_ok(),
                parent.is_none() || child.is_none(),
                "{source}"
            );
            if let Ok(document) = parsed {
                let annotation = &document.edmx.as_ref().unwrap().schemas[0].external_annotations
                    [0]
                .annotations[0];
                assert_eq!(annotation.qualifier.as_deref(), child.or(parent));
            }
        }
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn annotation_terms_and_targets_respect_reference_include_scope() {
    use csdl_edm::{
        resolver::Resolver,
        validator::{self, AnnotationIssue},
    };
    let external_xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Public"><Term Name="Value" Type="Edm.String"/><ComplexType Name="Owner"><Property Name="Detail" Type="Hidden.Detail"/><Annotation Term="Public.Value" String="value"/></ComplexType><ComplexType Name="Derived" BaseType="Public.Owner"/><Function Name="F"><Parameter Name="arg" Type="Public.Owner"/><ReturnType Type="Edm.String"/></Function><Function Name="F"><Parameter Name="arg" Type="Hidden.Detail"/><ReturnType Type="Edm.String"/></Function><Function Name="G"><Parameter Name="arg" Type="Collection(Public.Owner)"/><ReturnType Type="Edm.String"/></Function><Function Name="G"><Parameter Name="arg" Type="Collection(Hidden.Detail)"/><ReturnType Type="Edm.String"/></Function><Function Name="H"><Parameter Name="arg" Type="Edm.String"/><ReturnType Type="Edm.String"/></Function></Schema><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Hidden"><Term Name="Value" Type="Edm.String"/><ComplexType Name="Detail"/><ComplexType Name="Derived" BaseType="Public.Owner"/></Schema></edmx:DataServices></edmx:Edmx>"#;
    let external_json = r#"{"$Version":"4.01","Public":{"Value":{"$Kind":"Term"},"Owner":{"$Kind":"ComplexType","Detail":{"$Type":"Hidden.Detail"},"@Public.Value":"value"},"Derived":{"$Kind":"ComplexType","$BaseType":"Public.Owner"},"F":[{"$Kind":"Function","$Parameter":[{"$Name":"arg","$Type":"Public.Owner"}],"$ReturnType":{}},{"$Kind":"Function","$Parameter":[{"$Name":"arg","$Type":"Hidden.Detail"}],"$ReturnType":{}}],"G":[{"$Kind":"Function","$Parameter":[{"$Name":"arg","$Type":"Public.Owner","$Collection":true}],"$ReturnType":{}},{"$Kind":"Function","$Parameter":[{"$Name":"arg","$Type":"Hidden.Detail","$Collection":true}],"$ReturnType":{}}],"H":[{"$Kind":"Function","$Parameter":[{"$Name":"arg","$Type":"Edm.String"}],"$ReturnType":{}}]},"Hidden":{"Value":{"$Kind":"Term"},"Detail":{"$Kind":"ComplexType"},"Derived":{"$Kind":"ComplexType","$BaseType":"Public.Owner"}}}"#;
    for (term, term_known) in [
        ("Public.Value", true),
        ("P.Value", true),
        ("Hidden.Value", false),
        ("Missing.Value", false),
    ] {
        for (target, target_known) in [
            ("N.Owner", true),
            ("Public.Owner", true),
            ("P.Owner", true),
            ("Hidden.Detail", false),
            ("Missing.Owner", false),
            ("P.Owner/P.Derived", true),
            ("P.Owner/Hidden.Derived", false),
            ("P.Owner/Missing.Derived", false),
            ("P.Owner/@P.Value", true),
            ("P.Owner/@Hidden.Value", false),
            ("P.Owner/@Missing.Value", false),
            ("P.F(P.Owner)/arg", true),
            ("P.F(Hidden.Detail)/arg", false),
            ("P.F(Missing.Detail)/arg", false),
            ("P.G(Collection(P.Owner))/arg", true),
            ("P.G(Collection(Hidden.Detail))/arg", false),
            ("P.H(Edm.String)/arg", true),
        ] {
            let root_xml = format!(
                r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:Reference Uri="types"><edmx:Include Namespace="Public" Alias="P"/></edmx:Reference><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Owner"/><Annotations Target="{target}"><Annotation Term="{term}" Qualifier="Test" String="value"/></Annotations></Schema></edmx:DataServices></edmx:Edmx>"#
            );
            // JSON references use the declared alias; XML permits either spelling.
            let json_target = target.replace("Public.", "P.");
            let json_term = term.replace("Public.", "P.");
            let root_json = format!(
                r#"{{"$Version":"4.01","$Reference":{{"types":{{"$Include":[{{"$Namespace":"Public","$Alias":"P"}}]}}}},"N":{{"Owner":{{"$Kind":"ComplexType"}},"$Annotations":{{"{json_target}":{{"@{json_term}#Test":"value"}}}}}}}}"#
            );
            for (root, external) in [
                (
                    parser::from_xml_reader_with_profile(root_xml.as_bytes(), P::Strict),
                    parser::from_xml_reader_with_profile(external_xml.as_bytes(), P::Strict),
                ),
                (
                    parser::from_json_reader_with_profile(root_json.as_bytes(), P::Strict),
                    parser::from_json_reader_with_profile(external_json.as_bytes(), P::Strict),
                ),
            ] {
                let external = external.unwrap();
                let model = Resolver::resolve_with_loader(
                    root.unwrap(),
                    "https://example.test/root",
                    &mut |uri: &str| {
                        assert_eq!(uri, "https://example.test/types");
                        Ok(external.clone())
                    },
                )
                .unwrap();
                // Hidden is needed by Public.Owner, but must not leak into the root scope.
                assert!(
                    model
                        .referenced_schemas
                        .iter()
                        .any(|s| s.namespace == "Hidden")
                );
                let diagnostics = validator::validate_annotations(&model);
                assert_eq!(
                    diagnostics
                        .iter()
                        .any(|d| d.issue == AnnotationIssue::UnknownTerm),
                    !term_known,
                    "{term}, {target}: {diagnostics:?}"
                );
                assert_eq!(
                    diagnostics
                        .iter()
                        .any(|d| d.issue == AnnotationIssue::UnresolvedTarget),
                    !target_known,
                    "{term}, {target}: {diagnostics:?}"
                );
                assert_eq!(
                    diagnostics.is_empty(),
                    term_known && target_known,
                    "{term}, {target}: {diagnostics:?}"
                );
            }
        }
    }
}
