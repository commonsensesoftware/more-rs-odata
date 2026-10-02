use csdl_edm::{
    CsdlFormat,
    conformance::ValidationProfile as P,
    parser,
    serialization::{MetadataLevel, SerializationOptions},
};

#[test]
fn structured_controls_preserve_derived_types_and_inherited_nested_values() {
    for derived in [false, true] {
        let xml_type = if derived { r#" Type="A.Derived""# } else { "" };
        let json_type = if derived {
            r##""@type":"#A.Derived","##
        } else {
            ""
        };
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N" Alias="A"><ComplexType Name="Child"><Property Name="Amount" Type="Edm.Int64"/></ComplexType><ComplexType Name="Base"><Property Name="Amount" Type="Edm.Int64"/><Property Name="Children" Type="Collection(A.Child)" Nullable="false"/></ComplexType><ComplexType Name="Derived" BaseType="A.Base"/><Term Name="Row" Type="A.Base"/><Annotation Term="A.Row"><Record{xml_type}><PropertyValue Property="Amount" Int="7"/><PropertyValue Property="Children"><Collection><Record><PropertyValue Property="Amount" Int="9"/></Record></Collection></PropertyValue></Record></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        let json = format!(
            r#"{{"$Version":"4.01","N":{{"$Alias":"A","Child":{{"$Kind":"ComplexType","Amount":{{"$Type":"Edm.Int64"}}}},"Base":{{"$Kind":"ComplexType","Amount":{{"$Type":"Edm.Int64"}},"Children":{{"$Type":"A.Child","$Collection":true}}}},"Derived":{{"$Kind":"ComplexType","$BaseType":"A.Base"}},"Row":{{"$Kind":"Term","$Type":"A.Base"}},"@A.Row":{{{json_type}"Amount":7,"Children":[{{"Amount":9}}]}}}}}}"#
        );
        for document in [
            parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).unwrap(),
            parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).unwrap(),
        ] {
            for metadata in [
                MetadataLevel::Full,
                MetadataLevel::Minimal,
                MetadataLevel::None,
            ] {
                let mut bytes = Vec::new();
                document
                    .write_as_with_options(
                        &mut bytes,
                        CsdlFormat::Json,
                        &SerializationOptions {
                            metadata,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let output: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let row = &output["N"]["@A.Row"];
                if derived || metadata == MetadataLevel::Full {
                    assert_eq!(row["@type"], if derived { "#A.Derived" } else { "#A.Base" });
                } else {
                    assert!(row.get("@type").is_none());
                }
                assert_eq!(row["Amount"], 7);
                assert_eq!(row["Children"][0]["Amount"], 9);
                if metadata == MetadataLevel::Full {
                    assert_eq!(output["N"]["@A.Row@type"], row["@type"]);
                    assert_eq!(row["Amount@type"], "#Edm.Int64");
                    assert_eq!(row["Children@type"], "#Collection(A.Child)");
                    assert_eq!(row["Children"][0]["@type"], "#A.Child");
                    assert_eq!(row["Children"][0]["Amount@type"], "#Edm.Int64");
                } else {
                    assert!(row.get("Amount@type").is_none());
                    assert!(row.get("Children@type").is_none());
                    assert!(row["Children"][0].get("@type").is_none());
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

#[test]
fn full_metadata_emits_declared_scalar_and_collection_type_controls() {
    for (ty, expression, value, json_value, inferred) in [
        ("String", "String", "value", r#""value""#, true),
        ("Boolean", "Bool", "true", "true", true),
        ("Int32", "Int", "42", "42", true),
        ("Double", "Float", "1.25", "1.25", true),
        ("Int64", "Int", "42", "42", false),
        ("Decimal", "Decimal", "42", "42", false),
        ("Single", "Float", "1.25", "1.25", false),
        ("Date", "Date", "2026-10-02", r#""2026-10-02""#, false),
    ] {
        for collection in [false, true] {
            let xml_type = if collection {
                format!("Collection(Edm.{ty})")
            } else {
                format!("Edm.{ty}")
            };
            let xml_expression = format!("<{expression}>{value}</{expression}>");
            let xml_expression = if collection {
                format!("<Collection>{xml_expression}</Collection>")
            } else {
                xml_expression
            };
            let json_expression = if collection {
                format!("[{json_value}]")
            } else {
                json_value.into()
            };
            let xml = format!(
                r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{xml_type}"/><Annotation Term="N.Value">{xml_expression}</Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
            );
            let json = format!(
                r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"Edm.{ty}","$Collection":{collection}}},"@N.Value":{json_expression}}}}}"#
            );
            for document in [
                parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).unwrap(),
                parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).unwrap(),
            ] {
                for metadata in [
                    MetadataLevel::Full,
                    MetadataLevel::Minimal,
                    MetadataLevel::None,
                ] {
                    let mut bytes = Vec::new();
                    document
                        .write_as_with_options(
                            &mut bytes,
                            CsdlFormat::Json,
                            &SerializationOptions {
                                metadata,
                                ..Default::default()
                            },
                        )
                        .unwrap();
                    let output: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    let control = output["N"].get("@N.Value@type");
                    if metadata == MetadataLevel::Full && (collection || !inferred) {
                        assert_eq!(
                            control.and_then(|v| v.as_str()),
                            Some(format!("#{xml_type}").as_str())
                        );
                    } else {
                        assert!(control.is_none(), "{ty} {collection} {metadata:?}");
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
}
