use csdl_edm::{conformance::ValidationProfile as P, parser};

fn xml(expression: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="T" Type="Edm.Int64"/><Annotation Term="N.T">{expression}</Annotation><ComplexType Name="Kept"/></Schema></edmx:DataServices></edmx:Edmx>"#
    )
}

#[test]
fn malformed_xml_comments_and_processing_instructions_cannot_be_skipped() {
    for (markup, valid) in [
        ("<!--ordinary comment-->", true),
        ("<?extension valid data?>", true),
        ("<!--invalid--comment-->", false),
        ("<!--\u{1}-->", false),
        ("<?extension \u{b}?>", false),
    ] {
        for skipped in [false, true] {
            let markup = if skipped {
                format!("<Future>{markup}</Future>")
            } else {
                markup.into()
            };
            let source = xml("<Int>1</Int>").replace(
                "<ComplexType Name=\"Kept\"/>",
                &format!("{markup}<ComplexType Name=\"Kept\"/>"),
            );
            for profile in [P::Strict, P::Tolerant] {
                let result = parser::from_xml_reader_with_report(source.as_bytes(), profile);
                assert_eq!(
                    result.is_ok(),
                    valid,
                    "{profile:?}, skipped={skipped}: {markup:?}: {result:?}"
                );
            }
        }
    }
}

#[test]
fn malformed_xml_character_data_cannot_be_recovered_or_skipped() {
    for body in [
        "\u{1}",
        "&#1;",
        "&#xFFFE;",
        "&undeclared;",
        "<![CDATA[\u{b}]]>",
    ] {
        for element in ["String", "Future"] {
            let source = xml(&format!("<{element}>{body}</{element}>"));
            for profile in [P::Strict, P::Tolerant] {
                assert!(
                    parser::from_xml_reader_with_report(source.as_bytes(), profile).is_err(),
                    "{profile:?}: {element}: {body:?}"
                );
            }
        }
        let source = xml("<Int>1</Int>").replace(
            "<ComplexType Name=\"Kept\"/>",
            &format!("<Future>{body}</Future><ComplexType Name=\"Kept\"/>"),
        );
        for profile in [P::Strict, P::Tolerant] {
            assert!(
                parser::from_xml_reader_with_report(source.as_bytes(), profile).is_err(),
                "Skipped extension: {profile:?}: {body:?}"
            );
        }
    }
    for body in ["&#9;&#10;&#13;", "&lt;&amp;&gt;", "<![CDATA[café 日本語]]>"] {
        let source = xml(&format!("<String>{body}</String>"));
        for profile in [P::Strict, P::Tolerant] {
            let report = parser::from_xml_reader_with_report(source.as_bytes(), profile).unwrap();
            assert!(report.diagnostics.is_empty());
            assert_eq!(
                report.document.edmx.unwrap().schemas[0].annotations.len(),
                1
            );
        }
    }
}

#[test]
fn unavailable_xml_operands_drop_whole_annotation_with_diagnostic() {
    for expression in [
        "<Future/>",
        "<Add><Int>1</Int><Future><Int>2</Int></Future></Add>",
        "<Collection><Future/></Collection>",
    ] {
        let source = xml(expression);
        let report = parser::from_xml_reader_with_report(source.as_bytes(), P::Tolerant).unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].location, "N.T");
        let schema = &report.document.edmx.unwrap().schemas[0];
        assert!(schema.annotations.is_empty());
        assert_eq!(schema.elements.len(), 2);
        assert!(parser::from_xml_reader_with_report(source.as_bytes(), P::Strict).is_err());
    }
}

#[test]
fn unavailable_json_operands_drop_annotation_and_its_controls() {
    for expression in [
        r#"{"$Future":1}"#,
        r#"{"$Add":[1,{"$Future":2}]}"#,
        r#"[{"$Future":2}]"#,
    ] {
        let source = format!(
            r##"{{"$Version":"4.01","N":{{"T":{{"$Kind":"Term","$Type":"Edm.Int64"}},"@N.T":{expression},"@N.T@type":"#Edm.Int64","@N.T@N.Note":"lost with parent","Kept":{{"$Kind":"ComplexType"}}}}}}"##
        );
        let report = parser::from_json_reader_with_report(source.as_bytes(), P::Tolerant).unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].location, "/N/@N.T");
        let schema = &report.document.edmx.unwrap().schemas[0];
        assert!(schema.annotations.is_empty());
        assert_eq!(schema.elements.len(), 2);
        assert!(parser::from_json_reader_with_report(source.as_bytes(), P::Strict).is_err());
    }
}

#[test]
fn known_expressions_and_unknown_terms_are_not_recovered_away() {
    let source = xml("<Add><Int>1</Int><Int>2</Int></Add>");
    for profile in [P::Tolerant, P::Strict] {
        let report = parser::from_xml_reader_with_report(source.as_bytes(), profile).unwrap();
        assert!(report.diagnostics.is_empty());
        assert_eq!(
            report.document.edmx.unwrap().schemas[0].annotations.len(),
            1
        );
        let report = parser::from_json_reader_with_report(
            br#"{"$Version":"4.01","N":{"@Unknown.Term":{"$Add":[1,2]}}}"#.as_slice(),
            profile,
        )
        .unwrap();
        assert!(report.diagnostics.is_empty());
        assert_eq!(
            report.document.edmx.unwrap().schemas[0].annotations.len(),
            1
        );
    }
}

#[test]
fn invalid_annotation_type_controls_do_not_prevent_tolerant_consumption() {
    for control in ["7", r##""#not a type""##] {
        let source = format!(r#"{{"$Version":"4.01","N":{{"@N.T":1,"@N.T@type":{control}}}}}"#);
        let report = parser::from_json_reader_with_report(source.as_bytes(), P::Tolerant).unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert!(
            report.document.edmx.unwrap().schemas[0]
                .annotations
                .is_empty()
        );
        assert!(parser::from_json_reader_with_report(source.as_bytes(), P::Strict).is_err());
    }
}

#[test]
fn invalid_annotations_recover_but_malformed_source_does_not() {
    let report =
        parser::from_xml_reader_with_report(xml("<Add><Int>1</Int></Add>").as_bytes(), P::Tolerant)
            .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    let source = xml("<Int>1</Int>").replace(
        "<Annotation Term=\"N.T\"><Int>1</Int></Annotation>",
        "<Annotation Term=\"N.T\" Int=\"invalid\"/>",
    );
    let report = parser::from_xml_reader_with_report(source.as_bytes(), P::Tolerant).unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    let report = parser::from_json_reader_with_report(
        br#"{"$Version":"4.01","N":{"@N.T":{"$Add":[1]}}}"#.as_slice(),
        P::Tolerant,
    )
    .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert!(
        parser::from_xml_reader_with_report(xml("<Future></Other>").as_bytes(), P::Tolerant)
            .is_err()
    );
    assert!(
        parser::from_json_reader_with_report(
            br#"{"$Version":"4.01","N":{"@N.T":{"$Future":}}}"#.as_slice(),
            P::Tolerant
        )
        .is_err()
    );
}
