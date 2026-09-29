use csdl_edm::{
    edm::AnnotationTarget,
    parser,
    resolver::Resolver,
    validator::{AnnotationIssue, validate_annotations, validate_document},
};

fn documents(body: &str) -> Vec<csdl_edm::CsdlDocument> {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo" Alias="D">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let json = document.to_json_value().to_string();
    let mut xml_again = Vec::new();
    document
        .write_as(&mut xml_again, csdl_edm::CsdlFormat::Xml)
        .unwrap();
    vec![
        document,
        parser::from_json_reader(json.as_bytes()).unwrap(),
        parser::from_xml_reader(xml_again.as_slice()).unwrap(),
    ]
}

fn issues(body: &str, expected: AnnotationIssue) {
    for document in documents(body) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let diagnostics = validate_annotations(&model);
        assert!(
            diagnostics.iter().any(|d| d.issue == expected),
            "{diagnostics:?}"
        );
        validate_document(&model).unwrap();
    }
}

#[test]
fn external_groups_roundtrip_resolve_aliases_and_inherit_qualifiers() {
    for document in documents(
        r#"<Term Name="Label" Type="Edm.String" AppliesTo="Property"/><ComplexType Name="Value"><Property Name="Name" Type="Edm.String"/></ComplexType><Annotations Target="D.Value/Name" Qualifier="Tablet"><Annotation Term="D.Label" String="Display"/></Annotations>"#,
    ) {
        assert_eq!(
            document.edmx.as_ref().unwrap().schemas[0]
                .external_annotations
                .len(),
            1
        );
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        assert!(
            validate_annotations(&model).is_empty(),
            "{:?}",
            validate_annotations(&model)
        );
        let annotation = &model.schemas[0].annotations.get().unwrap()[0];
        assert!(annotation.term.is_some());
        assert_eq!(annotation.qualifier.as_deref(), Some("Tablet"));
        assert!(matches!(
            annotation.targets[0].last(),
            Some(AnnotationTarget::Property(_))
        ));
    }
}

#[test]
fn diagnoses_duplicates_applicability_unknown_targets_and_base_terms() {
    issues(
        r#"<Term Name="Label" Type="Edm.String"/><ComplexType Name="Value"><Annotation Term="D.Label" String="One"/></ComplexType><Annotations Target="Demo.Value"><Annotation Term="Demo.Label" String="Two"/></Annotations>"#,
        AnnotationIssue::Duplicate,
    );
    issues(
        r#"<Term Name="Label" Type="Edm.String" AppliesTo="Property"/><ComplexType Name="Value"><Annotation Term="D.Label" String="Wrong target"/></ComplexType>"#,
        AnnotationIssue::NotApplicable,
    );
    issues(
        r#"<Term Name="Label" Type="Edm.String"/><Annotations Target="Demo.Missing"><Annotation Term="D.Label" String="Missing"/></Annotations>"#,
        AnnotationIssue::UnresolvedTarget,
    );
    issues(
        r#"<Term Name="Base" Type="Edm.String"/><Term Name="Derived" Type="Edm.String" BaseTerm="D.Base"/><ComplexType Name="Value"><Annotation Term="D.Derived" String="Only derived"/></ComplexType>"#,
        AnnotationIssue::MissingBaseTerm,
    );
    issues(
        r#"<ComplexType Name="Value"><Annotation Term="External.Unknown" String="Preserved"/></ComplexType>"#,
        AnnotationIssue::UnknownTerm,
    );
}

#[test]
fn operation_signature_targets_resolve_parameters_and_returns() {
    for document in documents(
        r#"<Term Name="Label" Type="Edm.String" AppliesTo="Parameter ReturnType"/><Function Name="F"><Parameter Name="x" Type="Edm.String"/><ReturnType Type="Edm.String"/></Function><Function Name="F"><Parameter Name="y" Type="Edm.Int32"/><ReturnType Type="Edm.String"/></Function><Annotations Target="D.F(Edm.String)/x"><Annotation Term="D.Label" String="Input"/></Annotations><Annotations Target="D.F/$ReturnType"><Annotation Term="D.Label" String="Result"/></Annotations>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let annotations = model.schemas[0].annotations.get().unwrap();
        assert_eq!(annotations[0].targets.len(), 1);
        assert_eq!(annotations[1].targets.len(), 2);
        assert!(
            validate_annotations(&model).is_empty(),
            "{:?}",
            validate_annotations(&model)
        );
    }
}

#[test]
fn resolves_expression_paths_and_checks_value_shapes() {
    let body = r#"<Term Name="Display" Type="Edm.String"/><ComplexType Name="Value"><Property Name="Name" Type="Edm.String"/><Annotation Term="D.Display" Path="Name"/></ComplexType>"#;
    for document in documents(body) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let annotation = &model.schemas[0].annotations.get().unwrap()[0];
        assert!(matches!(
            annotation.expression_paths[0].segments.last(),
            Some(AnnotationTarget::Property(_))
        ));
        assert!(validate_annotations(&model).is_empty());
    }
    issues(
        &body.replace(r#"Path="Name""#, r#"Path="Missing""#),
        AnnotationIssue::InvalidExpressionPath,
    );
    issues(
        &body.replace(r#"Path="Name""#, r#"Bool="true""#),
        AnnotationIssue::IncompatibleValue,
    );
}

#[test]
fn resolves_annotations_on_record_and_property_value_expressions() {
    for document in documents(
        r#"<Term Name="RecordLabel" Type="Edm.String" AppliesTo="Record"/><Term Name="ValueLabel" Type="Edm.String" AppliesTo="PropertyValue"/><ComplexType Name="Value"><Property Name="Name" Type="Edm.String"/></ComplexType><Term Name="Data" Type="Demo.Value"/><ComplexType Name="Owner"><Annotation Term="D.Data"><Record><Annotation Term="D.RecordLabel" String="Record"/><PropertyValue Property="Name" String="Data"><Annotation Term="D.ValueLabel" String="Value"/></PropertyValue></Record></Annotation></ComplexType>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        assert_eq!(model.schemas[0].annotations.get().unwrap().len(), 3);
        assert!(
            validate_annotations(&model).is_empty(),
            "{:?}",
            validate_annotations(&model)
        );
    }
}
