use csdl_edm::{
    CsdlFormat, parser,
    serialization::{MetadataLevel, SerializationOptions},
};

fn output(source: &str, options: &SerializationOptions) -> serde_json::Value {
    let doc = parser::from_json_reader(source.as_bytes()).unwrap();
    let mut bytes = Vec::new();
    doc.write_as_with_options(&mut bytes, CsdlFormat::Json, options)
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn arbitrary_decimal_text_survives_numeric_and_ieee754_output() {
    let literal = "123456789012345678901234567890.1234567890123456789";
    let source = format!(
        r#"{{"$Version":"4.01","N":{{"T":{{"$Kind":"Term","$Type":"Edm.Decimal"}},"@N.T":{literal}}}}}"#
    );
    for compatible in [false, true] {
        let options = SerializationOptions {
            ieee754_compatible: compatible,
            ..Default::default()
        };
        let value = output(&source, &options);
        if compatible {
            assert_eq!(value["N"]["@N.T"].as_str(), Some(literal));
        } else {
            assert_eq!(value["N"]["@N.T"].to_string(), literal);
        }
    }
}

#[test]
fn full_metadata_types_nested_records_and_primitive_values() {
    let source = r#"{"$Version":"4.01","N":{"R":{"$Kind":"ComplexType","Money":{"$Type":"Edm.Decimal","$Scale":"variable"}},"T":{"$Kind":"Term","$Type":"N.R"},"@N.T":{"Money":"12345678901234567890.123456789"}}}"#;
    let options = SerializationOptions {
        metadata: MetadataLevel::Full,
        ..Default::default()
    };
    let value = output(source, &options);
    assert_eq!(value["N"]["@N.T"]["@type"], "#N.R");
    assert_eq!(value["N"]["@N.T"]["Money@type"], "#Edm.Decimal");
    assert_eq!(
        value["N"]["@N.T"]["Money"].to_string(),
        "12345678901234567890.123456789"
    );
    let parsed = parser::from_json_reader(value.to_string().as_bytes()).unwrap();
    #[cfg(all(feature = "resolver", feature = "validator"))]
    {
        let model =
            csdl_edm::resolver::Resolver::resolve_document(parsed.edmx.clone().unwrap()).unwrap();
        assert!(
            csdl_edm::validator::validate_annotations(&model).is_empty(),
            "{:?} {:?}",
            csdl_edm::validator::validate_annotations(&model),
            model.schemas[0].annotations
        );
    }
    let mut xml = Vec::new();
    parsed
        .write_as_with_options(&mut xml, CsdlFormat::Xml, &options)
        .unwrap();
    assert!(
        String::from_utf8(xml)
            .unwrap()
            .contains("12345678901234567890.123456789")
    );
}

#[test]
fn xml_base_rebases_to_absolute_reference_identity() {
    let source = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01" xml:base="../vocab/"><edmx:Reference Uri="types.xml" xml:base="v1/"><edmx:Include Namespace="Types"/></edmx:Reference><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"/></edmx:DataServices></edmx:Edmx>"#;
    let doc = parser::from_xml_reader(source.as_bytes()).unwrap();
    let mut bytes = Vec::new();
    let options = SerializationOptions {
        source_uri: Some("https://example.test/service/model.xml".into()),
        ..Default::default()
    };
    doc.write_as_with_options(&mut bytes, CsdlFormat::Json, &options)
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        value["$Reference"]
            .get("https://example.test/vocab/v1/types.xml")
            .is_some()
    );
}

#[test]
fn aliases_are_normalized_without_rewriting_literal_strings() {
    let value = output(
        r#"{"$Version":"4.01","N":{"$Alias":"Self","T":{"$Kind":"Term","$Type":"Edm.String"},"@N.T":"N.T"}}"#,
        &SerializationOptions::default(),
    );
    assert_eq!(value["N"]["@Self.T"], "N.T");
    assert!(value["N"].get("@N.T").is_none());
}

#[test]
fn collection_controls_and_defaults_follow_output_options() {
    let value = output(
        r#"{"$Version":"4.01","N":{"T":{"$Kind":"Term","$Type":"Edm.Int64","$Collection":true},"D":{"$Kind":"Term","$Type":"Edm.Decimal","$DefaultValue":"12345678901234567890.123"},"@N.T":["9223372036854775807"]}}"#,
        &SerializationOptions {
            metadata: MetadataLevel::Full,
            ieee754_compatible: true,
            ..Default::default()
        },
    );
    assert_eq!(value["N"]["@N.T@type"], "#Collection(Edm.Int64)");
    assert_eq!(value["N"]["D"]["$DefaultValue"], "12345678901234567890.123");
    assert_eq!(value["N"]["@N.T"][0], "9223372036854775807");
    let doc = parser::from_json_reader(value.to_string().as_bytes()).unwrap();
    #[cfg(all(feature = "resolver", feature = "validator"))]
    {
        let model = csdl_edm::resolver::Resolver::resolve_document(doc.edmx.unwrap()).unwrap();
        assert!(csdl_edm::validator::validate_annotations(&model).is_empty());
    }
    #[cfg(not(all(feature = "resolver", feature = "validator")))]
    let _ = doc;
}

#[test]
fn same_container_binding_targets_are_shortened() {
    let value = output(
        r#"{"$Version":"4.01","N":{"E":{"$Kind":"EntityType","$Key":["Id"],"Id":{"$Type":"Edm.Int32"},"Other":{"$Kind":"NavigationProperty","$Type":"N.E"}},"C":{"$Kind":"EntityContainer","Es":{"$Collection":true,"$Type":"N.E","$NavigationPropertyBinding":{"Other":"N.C/Es"}}}}}"#,
        &SerializationOptions::default(),
    );
    assert_eq!(
        value["N"]["C"]["Es"]["$NavigationPropertyBinding"]["Other"],
        "Es"
    );
}

#[test]
fn json_term_defaults_and_numeric_enum_members_convert_to_xml() {
    let doc = parser::from_json_reader(br#"{"$Version":"4.01","N":{"Text":{"$Kind":"Term"},"Bits":{"$Kind":"EnumType","$IsFlags":true,"A":1,"B":2},"T":{"$Kind":"Term","$Type":"N.Bits"},"@N.T":"3"}}"#.as_slice()).unwrap();
    let mut bytes = Vec::new();
    doc.write_as_with_options(&mut bytes, CsdlFormat::Xml, &Default::default())
        .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("N.Bits/A N.Bits/B"));
    let parsed = parser::from_xml_reader(text.as_bytes()).unwrap();
    #[cfg(all(feature = "resolver", feature = "validator"))]
    {
        let model = csdl_edm::resolver::Resolver::resolve_document(parsed.edmx.unwrap()).unwrap();
        assert!(csdl_edm::validator::validate_annotations(&model).is_empty());
    }
    #[cfg(not(all(feature = "resolver", feature = "validator")))]
    let _ = parsed;
}

#[test]
fn unknown_xml_collection_nullability_is_not_invented() {
    let doc = parser::from_xml_reader(br#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="C"><Property Name="Values" Type="Collection(Edm.String)"/></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#.as_slice()).unwrap();
    let mut bytes = Vec::new();
    assert!(
        doc.write_as_with_options(&mut bytes, CsdlFormat::Json, &Default::default())
            .is_err()
    );
    assert!(bytes.is_empty());
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn checked_output_uses_referenced_types_and_loads_each_document_once() {
    use csdl_edm::{
        CsdlDocument, resolver::DocumentLoader, validator::AnnotationValidationOptions,
    };
    struct Loader(usize);
    impl DocumentLoader for Loader {
        fn load(&mut self, uri: &str) -> Result<CsdlDocument, String> {
            assert_eq!(uri, "https://example.test/types.json");
            self.0 += 1;
            parser::from_json_reader(br#"{"$Version":"4.01","Types":{"$Alias":"Local","R":{"$Kind":"ComplexType","Value":{"$Type":"Edm.Decimal","$Scale":"variable"}},"T":{"$Kind":"Term","$Type":"Local.R"}}}"#.as_slice()).map_err(|e| e.to_string())
        }
    }
    for format in [CsdlFormat::Xml, CsdlFormat::Json] {
        let doc = parser::from_json_reader(br#"{"$Version":"4.01","$Reference":{"types.json":{"$Include":[{"$Namespace":"Types","$Alias":"External"}]}},"N":{"@External.T":{"Value":"12345678901234567890.123"}}}"#.as_slice()).unwrap();
        let mut loader = Loader(0);
        let mut bytes = Vec::new();
        doc.write_as_checked_with_loader(
            &mut bytes,
            format,
            &SerializationOptions::default(),
            "https://example.test/model.json",
            &mut loader,
            &AnnotationValidationOptions::default(),
        )
        .unwrap();
        assert_eq!(loader.0, 1);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("12345678901234567890.123")
        );
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn checked_output_does_not_write_invalid_graphs() {
    struct Loader;
    impl csdl_edm::resolver::DocumentLoader for Loader {
        fn load(&mut self, _: &str) -> Result<csdl_edm::CsdlDocument, String> {
            panic!("no references");
        }
    }
    let doc = parser::from_json_reader(br#"{"$Version":"4.01","N":{"T":{"$Kind":"Term","$Type":"Edm.Int32"},"@N.T":"not an integer"}}"#.as_slice()).unwrap();
    let mut bytes = Vec::new();
    assert!(
        doc.write_as_checked_with_loader(
            &mut bytes,
            CsdlFormat::Json,
            &SerializationOptions::default(),
            "https://example.test/model.json",
            &mut Loader,
            &Default::default()
        )
        .is_err()
    );
    assert!(bytes.is_empty());
}
