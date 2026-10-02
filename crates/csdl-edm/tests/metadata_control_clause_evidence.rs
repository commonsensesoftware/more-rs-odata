use csdl_edm::{
    CsdlFormat,
    conformance::ValidationProfile as P,
    parser,
    serialization::{MetadataLevel, SerializationOptions},
};

#[test]
fn dynamic_primitive_controls_preserve_noninferable_value_types() {
    for (kind, ty, literal, json_value) in [
        ("Binary", "Binary", "AQI", r#""AQI""#),
        (
            "DateTimeOffset",
            "DateTimeOffset",
            "2026-10-02T12:00:00Z",
            r#""2026-10-02T12:00:00Z""#,
        ),
        ("Duration", "Duration", "PT1S", r#""PT1S""#),
        (
            "Guid",
            "Guid",
            "01234567-89ab-cdef-0123-456789abcdef",
            r#""01234567-89ab-cdef-0123-456789abcdef""#,
        ),
        ("TimeOfDay", "TimeOfDay", "12:34:56", r#""12:34:56""#),
        (
            "Int",
            "Int64",
            "9223372036854775807",
            r#""9223372036854775807""#,
        ),
        ("Float", "Double", "INF", r#""INF""#),
        ("Float", "Double", "-INF", r#""-INF""#),
        ("Float", "Double", "NaN", r#""NaN""#),
    ] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Open" OpenType="true"/><Term Name="Row" Type="N.Open"/><Annotation Term="N.Row"><Record><PropertyValue Property="Value"><{kind}>{literal}</{kind}></PropertyValue></Record></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        let json = format!(
            r##"{{"$Version":"4.01","N":{{"Open":{{"$Kind":"ComplexType","$OpenType":true}},"Row":{{"$Kind":"Term","$Type":"N.Open"}},"@N.Row":{{"Value":{json_value},"Value@type":"#Edm.{ty}"}}}}}}"##
        );
        for document in [
            parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).unwrap(),
            parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).unwrap(),
        ] {
            for metadata in [MetadataLevel::Full, MetadataLevel::Minimal] {
                let mut bytes = Vec::new();
                document
                    .write_as_with_options(
                        &mut bytes,
                        CsdlFormat::Json,
                        &SerializationOptions {
                            metadata,
                            ieee754_compatible: true,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let output: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    output["N"]["@N.Row"]["Value@type"],
                    format!("#Edm.{ty}"),
                    "{kind} {literal}"
                );
                let parsed =
                    parser::from_json_reader_with_profile(bytes.as_slice(), P::Strict).unwrap();
                #[cfg(all(feature = "resolver", feature = "validator"))]
                {
                    let model =
                        csdl_edm::resolver::Resolver::resolve_document(parsed.edmx.unwrap())
                            .unwrap();
                    let issues = csdl_edm::validator::validate_annotations(&model);
                    assert!(issues.is_empty(), "{kind} {literal}: {issues:?}");
                }
                #[cfg(not(all(feature = "resolver", feature = "validator")))]
                let _ = parsed;
            }
        }
    }
}

#[test]
fn full_and_minimal_metadata_type_dynamic_open_record_constants() {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Open" OpenType="true"/><Term Name="Row" Type="N.Open"/><Annotation Term="N.Row"><Record><PropertyValue Property="Text" String="value"/><PropertyValue Property="Enabled" Bool="true"/><PropertyValue Property="Date" Date="2026-10-02"/><PropertyValue Property="Amount" Decimal="1.25"/><PropertyValue Property="Dates"><Collection><Date>2026-10-02</Date></Collection></PropertyValue></Record></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#;
    let json = r##"{"$Version":"4.01","N":{"Open":{"$Kind":"ComplexType","$OpenType":true},"Row":{"$Kind":"Term","$Type":"N.Open"},"@N.Row":{"Text":"value","Enabled":true,"Date":"2026-10-02","Date@type":"#Edm.Date","Amount":1.25,"Amount@type":"#Edm.Decimal","Dates":["2026-10-02"],"Dates@type":"#Collection(Edm.Date)"}}}"##;
    for document in [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).unwrap(),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict).unwrap(),
    ] {
        for metadata in [MetadataLevel::Full, MetadataLevel::Minimal] {
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
            let row = &output["N"]["@N.Row"];
            assert_eq!(row["Text"], "value");
            assert_eq!(row["Enabled"], true);
            assert!(row.get("Text@type").is_none());
            assert!(row.get("Enabled@type").is_none());
            assert_eq!(row["Date@type"], "#Edm.Date");
            assert_eq!(row["Amount@type"], "#Edm.Decimal");
            assert_eq!(row["Dates@type"], "#Collection(Edm.Date)");
            let parsed =
                parser::from_json_reader_with_profile(bytes.as_slice(), P::Strict).unwrap();
            #[cfg(all(feature = "resolver", feature = "validator"))]
            {
                let model =
                    csdl_edm::resolver::Resolver::resolve_document(parsed.edmx.unwrap()).unwrap();
                assert!(csdl_edm::validator::validate_annotations(&model).is_empty());
            }
            #[cfg(not(all(feature = "resolver", feature = "validator")))]
            let _ = parsed;
        }
    }
}

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
        ("Double", "Float", "INF", r#""INF""#, false),
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
