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
