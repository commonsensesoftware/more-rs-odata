use csdl_edm::{conformance::ValidationProfile as P, parser};

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
