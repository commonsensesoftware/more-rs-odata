use csdl_edm::{CsdlDocument, CsdlFormat, conformance::ValidationProfile as P, parser};

fn document() -> CsdlDocument {
    let xml = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Z"><EnumType Name="Sequence"><Member Name="Zulu"/><Member Name="Alpha"/><Member Name="Middle"/><Member Name="Beta"/></EnumType><Function Name="Call"><Parameter Name="z" Type="Edm.String"/><Parameter Name="a" Type="Edm.Int32"/><ReturnType Type="Edm.String"/></Function><Term Name="Items" Type="Collection(Edm.String)"/><Annotation Term="Z.Items"><Collection><String>z</String><String>a</String><String>z</String></Collection></Annotation></Schema><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="A"><ComplexType Name="Last"/></Schema></edmx:DataServices></edmx:Edmx>"#;
    parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).unwrap()
}

#[test]
fn ordered_xml_members_parameters_and_items_survive_both_output_formats() {
    let original = document();
    for format in [CsdlFormat::Xml, CsdlFormat::Json] {
        let mut bytes = Vec::new();
        original.write_as(&mut bytes, format).unwrap();
        let reparsed = match format {
            CsdlFormat::Xml => parser::from_xml_reader_with_profile(bytes.as_slice(), P::Strict),
            CsdlFormat::Json => parser::from_json_reader_with_profile(bytes.as_slice(), P::Strict),
        }
        .unwrap();
        let schemas = &reparsed.edmx.as_ref().unwrap().schemas;
        assert_eq!(
            schemas
                .iter()
                .map(|s| s.namespace.as_str())
                .collect::<Vec<_>>(),
            ["Z", "A"]
        );
        let csdl_edm::csdl::SchemaElement::EnumType(enumeration) = &schemas[0].elements[0] else {
            panic!("enum order changed")
        };
        assert_eq!(
            enumeration
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["Zulu", "Alpha", "Middle", "Beta"]
        );
        let json = reparsed.to_json_value();
        assert_eq!(json["Z"]["Sequence"]["Zulu"], 0);
        assert_eq!(json["Z"]["Sequence"]["Alpha"], 1);
        assert_eq!(json["Z"]["Sequence"]["Middle"], 2);
        assert_eq!(json["Z"]["Sequence"]["Beta"], 3);
        assert_eq!(json["Z"]["Call"][0]["$Parameter"][0]["$Name"], "z");
        assert_eq!(json["Z"]["Call"][0]["$Parameter"][1]["$Name"], "a");
        assert_eq!(json["Z"]["@Z.Items"], serde_json::json!(["z", "a", "z"]));
    }
}

#[cfg(feature = "resolver")]
#[test]
fn resolved_enum_values_and_parameter_positions_follow_xml_order() {
    use csdl_edm::{edm::SchemaElement, resolver::Resolver};
    let model = Resolver::resolve_document(document().edmx.unwrap()).unwrap();
    let SchemaElement::EnumType(enumeration) = model.schemas[0].elements[0].as_ref() else {
        panic!("enum order changed")
    };
    assert_eq!(
        enumeration
            .members
            .iter()
            .map(|m| (m.name.as_str(), m.resolved_value))
            .collect::<Vec<_>>(),
        [("Zulu", 0), ("Alpha", 1), ("Middle", 2), ("Beta", 3)]
    );
    let SchemaElement::Function(function) = model.schemas[0].elements[1].as_ref() else {
        panic!("function order changed")
    };
    assert_eq!(
        function
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
}
