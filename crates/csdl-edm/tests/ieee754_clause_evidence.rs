use csdl_edm::{
    CsdlFormat, conformance::ValidationProfile as P, parser, serialization::SerializationOptions,
};

#[test]
fn ieee754_options_leave_other_numeric_types_and_literal_strings_unchanged() {
    for (ty, value, expression) in [
        ("Byte", "255", "Int"),
        ("SByte", "-128", "Int"),
        ("Int16", "32767", "Int"),
        ("Int32", "2147483647", "Int"),
        ("Single", "1.25", "Float"),
        ("Double", "-1.25", "Float"),
    ] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Number" Type="Edm.{ty}" DefaultValue="{value}"/><Term Name="Text" Type="Edm.String" DefaultValue="{value}"/><Annotation Term="N.Number"><{expression}>{value}</{expression}></Annotation><Annotation Term="N.Text" String="{value}"/></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        let json = format!(
            r#"{{"$Version":"4.01","N":{{"Number":{{"$Kind":"Term","$Type":"Edm.{ty}","$DefaultValue":{value}}},"Text":{{"$Kind":"Term","$DefaultValue":"{value}"}},"@N.Number":{value},"@N.Text":"{value}"}}}}"#
        );
        for document in [
            parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).unwrap(),
            parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).unwrap(),
        ] {
            for options in [
                SerializationOptions::default(),
                SerializationOptions {
                    ieee754_compatible: false,
                    ..Default::default()
                },
                SerializationOptions {
                    ieee754_compatible: true,
                    ..Default::default()
                },
            ] {
                let mut bytes = Vec::new();
                document
                    .write_as_with_options(&mut bytes, CsdlFormat::Json, &options)
                    .unwrap();
                let output: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                for actual in [
                    &output["N"]["Number"]["$DefaultValue"],
                    &output["N"]["@N.Number"],
                ] {
                    assert!(actual.is_number(), "{ty}: {actual}");
                    assert_eq!(actual.as_f64(), Some(value.parse().unwrap()));
                }
                assert_eq!(output["N"]["Text"]["$DefaultValue"], value);
                assert_eq!(output["N"]["@N.Text"], value);
            }
        }
    }
}

#[test]
fn ieee754_output_preserves_typed_numbers_in_defaults_collections_and_records() {
    for (ty, value, expression) in [
        ("Edm.Int64", "9223372036854775807", "Int"),
        ("Edm.Int64", "-9223372036854775808", "Int"),
        ("Edm.Decimal", "12345678901234567890.123456789", "Decimal"),
        ("Edm.Decimal", "-0.000000000000000000001", "Decimal"),
    ] {
        let xml_scale = if ty == "Edm.Decimal" {
            r#" Scale="variable""#
        } else {
            ""
        };
        let json_scale = if ty == "Edm.Decimal" {
            r#", "$Scale":"variable""#
        } else {
            ""
        };
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><TypeDefinition Name="Number" UnderlyingType="{ty}"{xml_scale}/><ComplexType Name="Record"><Property Name="Value" Type="N.Number" DefaultValue="{value}"/></ComplexType><Term Name="Scalar" Type="N.Number" DefaultValue="{value}"/><Term Name="Items" Type="Collection(N.Number)"/><Term Name="Row" Type="N.Record"/><Annotation Term="N.Scalar"><{expression}>{value}</{expression}></Annotation><Annotation Term="N.Items"><Collection><{expression}>{value}</{expression}></Collection></Annotation><Annotation Term="N.Row"><Record><PropertyValue Property="Value"><{expression}>{value}</{expression}></PropertyValue></Record></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        let json = format!(
            r#"{{"$Version":"4.01","N":{{"Number":{{"$Kind":"TypeDefinition","$UnderlyingType":"{ty}"{json_scale}}},"Record":{{"$Kind":"ComplexType","Value":{{"$Type":"N.Number","$DefaultValue":"{value}"}}}},"Scalar":{{"$Kind":"Term","$Type":"N.Number","$DefaultValue":"{value}"}},"Items":{{"$Kind":"Term","$Type":"N.Number","$Collection":true}},"Row":{{"$Kind":"Term","$Type":"N.Record"}},"@N.Scalar":"{value}","@N.Items":["{value}"],"@N.Row":{{"Value":"{value}"}}}}}}"#
        );
        for document in [
            parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).unwrap(),
            parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).unwrap(),
        ] {
            for compatible in [false, true] {
                let mut bytes = Vec::new();
                document
                    .write_as_with_options(
                        &mut bytes,
                        CsdlFormat::Json,
                        &SerializationOptions {
                            ieee754_compatible: compatible,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let output: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                for actual in [
                    &output["N"]["Record"]["Value"]["$DefaultValue"],
                    &output["N"]["Scalar"]["$DefaultValue"],
                    &output["N"]["@N.Scalar"],
                    &output["N"]["@N.Items"][0],
                    &output["N"]["@N.Row"]["Value"],
                ] {
                    if compatible {
                        assert_eq!(actual.as_str(), Some(value), "{ty}: {actual}");
                    } else {
                        assert!(actual.is_number(), "{ty}: {actual}");
                        assert_eq!(actual.to_string(), value);
                    }
                }
                let parsed =
                    parser::from_json_reader_with_profile(bytes.as_slice(), P::Strict).unwrap();
                #[cfg(all(feature = "resolver", feature = "validator"))]
                {
                    let model =
                        csdl_edm::resolver::Resolver::resolve_document(parsed.edmx.unwrap())
                            .unwrap();
                    csdl_edm::validator::validate_document(&model).unwrap();
                    assert!(csdl_edm::validator::validate_annotations(&model).is_empty());
                }
                #[cfg(not(all(feature = "resolver", feature = "validator")))]
                let _ = parsed;
            }
        }
    }
}
