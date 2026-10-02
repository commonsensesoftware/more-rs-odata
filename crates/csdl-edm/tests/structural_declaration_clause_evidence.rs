use csdl_edm::{CsdlDocument, conformance::ValidationProfile as P, parser};

fn xml(body: &str) -> std::io::Result<CsdlDocument> {
    let source = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict)
}

fn json(body: &str) -> std::io::Result<CsdlDocument> {
    let source = format!(r#"{{"$Version":"4.01","N":{{{body}}}}}"#);
    parser::from_json_reader_with_profile(source.as_bytes(), P::Strict)
}

fn validate(document: CsdlDocument) {
    #[cfg(all(feature = "resolver", feature = "validator"))]
    {
        let model = csdl_edm::resolver::Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        csdl_edm::validator::validate_document(&model).unwrap();
    }
    #[cfg(not(all(feature = "resolver", feature = "validator")))]
    let _ = document;
}

#[test]
fn complex_declarations_require_name_or_kind_and_retain_optional_content() {
    assert!(xml("<ComplexType/>").is_err());
    assert!(xml(r#"<ComplexType Abstract="true" OpenType="true"/>"#).is_err());
    assert!(json(r#""Owner":{}"#).is_err());
    for kind in ["null", "true", "1", "[]", r#""complextype""#] {
        assert!(json(&format!(r#""Owner":{{"$Kind":{kind}}}"#)).is_err());
    }
    for document in [
        xml(r#"<ComplexType Name="Owner"/>"#).unwrap(),
        json(r#""Owner":{"$Kind":"ComplexType"}"#).unwrap(),
    ] {
        let csdl_edm::csdl::SchemaElement::ComplexType(owner) =
            &document.edmx.as_ref().unwrap().schemas[0].elements[0]
        else {
            panic!("missing complex type")
        };
        assert_eq!(owner.name, "Owner");
        assert_eq!(
            (&owner.base_type, owner.abstract_, owner.open_type),
            (&None, None, None)
        );
        assert!(
            owner.properties.is_empty()
                && owner.navigation_properties.is_empty()
                && owner.annotations.is_empty()
        );
        validate(document);
    }
    for enabled in [false, true] {
        for document in [
            xml(&format!(r#"<Term Name="Note" Type="Edm.String"/><EntityType Name="Target"/><ComplexType Name="Base" Abstract="true"/><ComplexType Name="Owner" BaseType="N.Base" Abstract="{enabled}" OpenType="{enabled}"><Property Name="Text" Type="Edm.String"/><NavigationProperty Name="Related" Type="N.Target"/><Annotation Term="N.Note" String="retained"/></ComplexType>"#)).unwrap(),
            json(&format!(r#""Note":{{"$Kind":"Term","$Type":"Edm.String"}},"Target":{{"$Kind":"EntityType"}},"Base":{{"$Kind":"ComplexType","$Abstract":true}},"Owner":{{"$Kind":"ComplexType","$BaseType":"N.Base","$Abstract":{enabled},"$OpenType":{enabled},"Text":{{"$Type":"Edm.String"}},"Related":{{"$Kind":"NavigationProperty","$Type":"N.Target"}},"@N.Note":"retained"}}"#)).unwrap(),
        ] {
            let owner = document.edmx.as_ref().unwrap().schemas[0].elements.iter().find_map(|e| match e {
                csdl_edm::csdl::SchemaElement::ComplexType(t) if t.name == "Owner" => Some(t),
                _ => None,
            }).unwrap();
            assert_eq!(owner.base_type.as_deref(), Some("N.Base"));
            assert_eq!((owner.abstract_, owner.open_type), (Some(enabled), Some(enabled)));
            assert_eq!(owner.properties.len(), 1);
            assert_eq!(owner.properties[0].name, "Text");
            assert_eq!(owner.navigation_properties.len(), 1);
            assert_eq!(owner.navigation_properties[0].name, "Related");
            assert_eq!(owner.annotations.len(), 1);
            assert_eq!(document.to_json_value()["N"]["Owner"]["@N.Note"], "retained");
            validate(document);
        }
    }
}

#[test]
fn entity_declarations_require_xml_name_and_json_entity_kind() {
    assert!(xml("<EntityType/>").is_err());
    assert!(xml(r#"<EntityType Abstract="true" OpenType="true" HasStream="true"/>"#).is_err());
    assert!(json(r#""Owner":{}"#).is_err());
    for kind in ["null", "true", "1", "[]", r#""entitytype""#] {
        assert!(json(&format!(r#""Owner":{{"$Kind":{kind}}}"#)).is_err());
    }
    for document in [
        xml(r#"<EntityType Name="Owner"/>"#).unwrap(),
        json(r#""Owner":{"$Kind":"EntityType"}"#).unwrap(),
    ] {
        let csdl_edm::csdl::SchemaElement::EntityType(owner) =
            &document.edmx.as_ref().unwrap().schemas[0].elements[0]
        else {
            panic!("entity kind was not retained")
        };
        assert_eq!(owner.name, "Owner");
        assert_eq!(owner.base_type, None);
        assert_eq!(
            (owner.abstract_, owner.open_type, owner.has_stream),
            (None, None, None)
        );
        validate(document);
    }
}

#[test]
fn entity_declarations_retain_optional_base_and_boolean_attributes() {
    for enabled in [false, true] {
        for document in [
            xml(&format!(r#"<EntityType Name="Base" Abstract="true" HasStream="{enabled}"/><EntityType Name="Owner" BaseType="N.Base" Abstract="{enabled}" OpenType="{enabled}" HasStream="{enabled}"/>"#)).unwrap(),
            json(&format!(r#""Base":{{"$Kind":"EntityType","$Abstract":true,"$HasStream":{enabled}}},"Owner":{{"$Kind":"EntityType","$BaseType":"N.Base","$Abstract":{enabled},"$OpenType":{enabled},"$HasStream":{enabled}}}"#)).unwrap(),
        ] {
            let owner = document.edmx.as_ref().unwrap().schemas[0].elements.iter().find_map(|element| {
                match element {
                    csdl_edm::csdl::SchemaElement::EntityType(owner) if owner.name == "Owner" => Some(owner),
                    _ => None,
                }
            }).unwrap();
            assert_eq!(owner.base_type.as_deref(), Some("N.Base"));
            assert_eq!((owner.abstract_, owner.open_type, owner.has_stream), (Some(enabled), Some(enabled), Some(enabled)));
            validate(document);
        }
    }
}

#[test]
fn xml_properties_require_name_and_type_and_retain_optional_facets() {
    for property in [
        "<Property/>",
        r#"<Property Name="Value"/>"#,
        r#"<Property Type="Edm.String"/>"#,
    ] {
        assert!(
            xml(&format!(
                r#"<ComplexType Name="Owner">{property}</ComplexType>"#
            ))
            .is_err()
        );
    }
    for (attributes, expected) in [
        (r#"Type="Edm.String""#, serde_json::json!({})),
        (
            r#"Type="Edm.String" Nullable="false" MaxLength="12" Unicode="false" DefaultValue="abc""#,
            serde_json::json!({"$Nullable": false, "$MaxLength": 12, "$Unicode": false, "$DefaultValue": "abc"}),
        ),
        (
            r#"Type="Edm.Decimal" Precision="8" Scale="2" DefaultValue="12.34""#,
            serde_json::json!({"$Precision": 8, "$Scale": 2, "$DefaultValue": "12.34"}),
        ),
        (
            r#"Type="Edm.GeographyPoint" SRID="4326""#,
            serde_json::json!({"$SRID": 4326}),
        ),
    ] {
        let document = xml(&format!(
            r#"<ComplexType Name="Owner"><Property Name="Value" {attributes}/></ComplexType>"#
        ))
        .unwrap();
        let value = document.to_json_value();
        for (name, expected) in expected.as_object().unwrap() {
            assert_eq!(
                &value["N"]["Owner"]["Value"][name], expected,
                "{attributes}: {name}"
            );
        }
        validate(document);
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn structural_properties_resolve_only_supported_scalar_and_collection_types() {
    let xml_types = r#"<ComplexType Name="Detail"/><EntityType Name="Entity"/><EnumType Name="Choice"><Member Name="One"/></EnumType><TypeDefinition Name="Code" UnderlyingType="Edm.String"/>"#;
    let json_types = r#""Detail":{"$Kind":"ComplexType"},"Entity":{"$Kind":"EntityType"},"Choice":{"$Kind":"EnumType","One":0},"Code":{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.String"}"#;
    for owner in ["EntityType", "ComplexType"] {
        for (ty, expected) in [
            ("Edm.String", true),
            ("N.Detail", true),
            ("N.Choice", true),
            ("N.Code", true),
            ("N.Entity", false),
            ("N.Missing", false),
            ("Missing.Detail", false),
        ] {
            for collection in [false, true] {
                let xml_type = if collection {
                    format!("Collection({ty})")
                } else {
                    ty.into()
                };
                for document in [
                    xml(&format!(r#"{xml_types}<{owner} Name="Owner"><Property Name="Value" Type="{xml_type}" Nullable="false"/></{owner}>"#)).unwrap(),
                    json(&format!(r#"{json_types},"Owner":{{"$Kind":"{owner}","Value":{{"$Type":"{ty}","$Collection":{collection},"$Nullable":false}}}}"#)).unwrap(),
                ] {
                    let result = csdl_edm::resolver::Resolver::resolve_document(document.edmx.unwrap())
                        .map_err(|e| format!("{e:?}"))
                        .and_then(|model| {
                            csdl_edm::validator::validate_document(&model).map_err(|e| format!("{e:?}"))?;
                            let properties = model.schemas[0].elements.iter().find_map(|element| match element.as_ref() {
                                csdl_edm::edm::SchemaElement::EntityType(t) if t.name == "Owner" => Some(t.properties.get().unwrap()),
                                csdl_edm::edm::SchemaElement::ComplexType(t) if t.name == "Owner" => Some(t.properties.get().unwrap()),
                                _ => None,
                            }).unwrap();
                            assert_eq!(properties[0].is_collection, collection);
                            Ok(())
                        });
                    assert_eq!(result.is_ok(), expected, "{owner} {xml_type}: {result:?}");
                }
            }
        }
    }
}

#[test]
fn json_property_collections_require_boolean_control_and_item_type() {
    for (control, collection) in [
        ("", false),
        (r#", "$Collection":false"#, false),
        (r#", "$Collection":true"#, true),
    ] {
        let document = json(&format!(
            r#""Owner":{{"$Kind":"ComplexType","Value":{{"$Type":"Edm.Int32"{control}}}}}"#
        ))
        .unwrap();
        let csdl_edm::csdl::SchemaElement::ComplexType(owner) =
            &document.edmx.as_ref().unwrap().schemas[0].elements[0]
        else {
            panic!("owner kind changed")
        };
        assert_eq!(owner.properties[0].type_name.as_deref(), Some("Edm.Int32"));
        assert_eq!(owner.properties[0].is_collection, collection);
        validate(document);
    }
    for (ty, control) in [
        ("Collection(Edm.Int32)", "true"),
        ("Collection(Edm.Int32)", "false"),
        ("Edm.Int32", r#""true""#),
        ("Edm.Int32", "1"),
        ("Edm.Int32", "null"),
        ("Edm.Int32", "[]"),
    ] {
        assert!(json(&format!(r#""Owner":{{"$Kind":"ComplexType","Value":{{"$Type":"{ty}","$Collection":{control}}}}}"#)).is_err(), "{ty} {control}");
    }
}
