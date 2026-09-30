use csdl_edm::{CsdlFormat, conformance::ValidationProfile, parser};

fn xml(version: &str, body: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="{version}"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    )
}

#[test]
fn strict_source_and_output_enforce_declared_version() {
    for body in [
        r#"<Term Name="T" Type="Edm.Untyped"/>"#,
        r#"<Term Name="T" Type="Edm.String" Unicode="true"/>"#,
        r#"<Term Name="T" Type="Edm.Decimal" Scale="floating"/>"#,
        r#"<Annotation Term="N.T"><Add><Int>1</Int><Int>2</Int></Add></Annotation>"#,
        r#"<Annotation Term="N.T"><Path>/N.Container/Items</Path></Annotation>"#,
    ] {
        let old = xml("4.0", body);
        assert!(
            parser::from_xml_reader_with_profile(old.as_bytes(), ValidationProfile::Strict)
                .is_err(),
            "{body}"
        );
        let doc = parser::from_xml_reader(old.as_bytes()).unwrap();
        let mut output = Vec::new();
        assert!(
            doc.write_as_with_profile(&mut output, CsdlFormat::Json, ValidationProfile::Strict)
                .is_err()
        );
        assert!(output.is_empty());
        assert!(
            parser::from_xml_reader_with_profile(
                xml("4.01", body).as_bytes(),
                ValidationProfile::Strict
            )
            .is_ok(),
            "{body}"
        );
    }
}

#[test]
fn strict_401_requires_collection_nullability_and_rejects_max_length_max() {
    for body in [
        r#"<ComplexType Name="C"><Property Name="P" Type="Collection(Edm.String)"/></ComplexType>"#,
        r#"<Term Name="T" Type="Edm.String" MaxLength="max"/>"#,
    ] {
        assert!(
            parser::from_xml_reader_with_profile(
                xml("4.01", body).as_bytes(),
                ValidationProfile::Strict
            )
            .is_err()
        );
        assert!(
            parser::from_xml_reader_with_profile(
                xml("4.0", body).as_bytes(),
                ValidationProfile::Strict
            )
            .is_ok()
        );
        assert!(
            parser::from_xml_reader_with_profile(
                xml("4.01", body).as_bytes(),
                ValidationProfile::Tolerant
            )
            .is_ok()
        );
    }
}

#[test]
fn guarded_output_rejects_loss_of_base_uri_and_container_selection() {
    let source = r#"{"$Version":"4.01","$EntityContainer":"N.A","N":{"A":{"$Kind":"EntityContainer"},"B":{"$Kind":"EntityContainer"}}}"#;
    let doc = parser::from_json_reader(source.as_bytes()).unwrap();
    assert!(
        doc.write_as_with_profile(Vec::new(), CsdlFormat::Xml, ValidationProfile::Strict)
            .is_err()
    );
    let mut doc =
        parser::from_xml_reader(xml("4.01", r#"<Term Name="T" Type="Edm.String"/>"#).as_bytes())
            .unwrap();
    doc.edmx.as_mut().unwrap().base_uri = Some("https://example.test/".into());
    assert!(doc.try_to_json_value().is_err());
}

#[test]
fn enum_and_nonfinite_values_have_correct_target_lexical_forms() {
    let source = xml(
        "4.01",
        r#"<EnumType Name="Flags" IsFlags="true"><Member Name="A" Value="1"/><Member Name="B" Value="2"/></EnumType><Term Name="T" Type="N.Flags"/><Term Name="F" Type="Edm.Double"/><Annotation Term="N.T" EnumMember="N.Flags/A N.Flags/B"/><Annotation Term="N.F" Float="INF"/>"#,
    );
    let doc = parser::from_xml_reader(source.as_bytes()).unwrap();
    let json = doc.try_to_json_value().unwrap();
    assert_eq!(json["N"]["@N.T"], "A,B");
    assert_eq!(json["N"]["@N.F"], "INF");
    let doc = parser::from_json_reader(json.to_string().as_bytes()).unwrap();
    let mut output = Vec::new();
    doc.write_as_with_profile(&mut output, CsdlFormat::Xml, ValidationProfile::Strict)
        .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("EnumMember=\"N.Flags/A N.Flags/B\""));
    assert!(text.contains("Float=\"INF\""));
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn referenced_schemas_keep_their_source_versions() {
    use csdl_edm::{resolver::Resolver, validator};
    let root = parser::from_json_reader(br#"{"$Version":"4.0","$Reference":{"new.json":{"$Include":[{"$Namespace":"New"}]}},"Old":{}}"#.as_slice()).unwrap();
    let external = parser::from_json_reader(
        br#"{"$Version":"4.01","New":{"Loose":{"$Kind":"EntityType"}}}"#.as_slice(),
    )
    .unwrap();
    let model =
        Resolver::resolve_with_loader(root, "https://example.test/root.json", &mut |_: &str| {
            Ok(external.clone())
        })
        .unwrap();
    assert_eq!(model.schema_versions["New"], "4.01");
    assert!(validator::validate_document(&model).is_ok());
}
