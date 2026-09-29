use csdl_edm::edm::{PrimitiveType, ResolvedType, SchemaElement};
use csdl_edm::resolver::{ResolveError, Resolver};
use csdl_edm::{CsdlFormat, parser};

#[test]
fn omitted_json_property_type_defaults_to_string_and_survives_xml_roundtrip() {
    let json = r#"{"$Version":"4.01","Demo":{"Value":{"$Kind":"ComplexType","Text":{},"Texts":{"$Collection":true}}}}"#;
    let document = parser::from_json_reader(json.as_bytes()).unwrap();
    let mut xml = Vec::new();
    document.write_as(&mut xml, CsdlFormat::Xml).unwrap();
    for document in [document, parser::from_xml_reader(xml.as_slice()).unwrap()] {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let SchemaElement::ComplexType(value) = model.schemas[0].elements[0].as_ref() else {
            panic!()
        };
        assert_eq!(value.properties().len(), 2);
        for property in value.properties() {
            assert!(matches!(
                property.ty,
                ResolvedType::Primitive(PrimitiveType::String)
            ));
            assert_eq!(property.is_collection, property.name == "Texts");
        }
    }
}

#[test]
fn missing_required_xml_property_type_is_still_rejected() {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo"><ComplexType Name="Value"><Property Name="Text"/></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#;
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    assert!(matches!(
        Resolver::resolve_document(document.edmx.unwrap()),
        Err(ResolveError::MissingTypeName { .. })
    ));
}

#[test]
fn xml_to_json_preserves_integer_id_and_nullable_defaults() {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo"><EntityType Name="Item" HasStream="true"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/><Property Name="Text" Type="Edm.String"/><NavigationProperty Name="Other" Type="Demo.Item"/></EntityType></Schema></edmx:DataServices></edmx:Edmx>"#;
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let json = document.to_json_value();
    assert_eq!(json["Demo"]["Item"]["ID"]["$Type"], "Edm.Int32");
    assert_eq!(json["Demo"]["Item"]["Text"]["$Nullable"], true);
    assert_eq!(json["Demo"]["Item"]["Other"]["$Nullable"], true);
    let document = parser::from_json_reader(json.to_string().as_bytes()).unwrap();
    let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
    let SchemaElement::EntityType(item) = model.schemas[0].elements[0].as_ref() else {
        panic!()
    };
    assert!(matches!(
        item.properties()[0].ty,
        ResolvedType::Primitive(PrimitiveType::Int32)
    ));
    assert_eq!(item.properties()[1].nullable, Some(true));
    assert_eq!(item.navigation_properties()[0].nullable, Some(true));
}
