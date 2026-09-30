use csdl_edm::{CsdlFormat, parser};

fn xml(expression: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Demo"><Annotation Term="Demo.Value">{expression}</Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
    )
}

fn roundtrip(expression: &str) {
    let original = parser::from_xml_reader(xml(expression).as_bytes()).unwrap();
    let json = original.to_json_value().to_string();
    let from_json = parser::from_json_reader(json.as_bytes()).unwrap();
    assert_eq!(original, from_json, "{json}");
    let mut bytes = Vec::new();
    from_json.write_as(&mut bytes, CsdlFormat::Xml).unwrap();
    assert_eq!(original, parser::from_xml_reader(bytes.as_slice()).unwrap());
}

#[test]
fn every_added_operator_and_model_path_roundtrips() {
    for op in ["Has", "In", "Add", "Sub", "Mul", "Div", "DivBy", "Mod"] {
        roundtrip(&format!("<{op}><Int>6</Int><Int>2</Int></{op}>"));
    }
    roundtrip("<Neg><Int>6</Int></Neg>");
    roundtrip("<ModelElementPath>/Demo.Type</ModelElementPath>");
    roundtrip("<LabeledElementReference>Demo.Label</LabeledElementReference>");
    roundtrip(r#"<LabeledElement Name="Label" Path="Price"/>"#);
}

#[test]
fn dynamic_annotations_and_type_facets_roundtrip() {
    roundtrip(
        r#"<Cast Type="Collection(Edm.Decimal)" Nullable="false" Precision="10" Scale="2"><Collection><Int>2</Int></Collection><Annotation Term="Demo.Note" String="typed"/></Cast>"#,
    );
    roundtrip(r#"<Add><Int>1</Int><Int>2</Int><Annotation Term="Demo.Note" String="sum"/></Add>"#);
    roundtrip(r#"<Null><Annotation Term="Demo.Note" String="empty"/></Null>"#);
    roundtrip(
        r#"<Add><Int>1</Int><Int>2</Int><Annotation Term="Demo.Note" String="sum"><Annotation Term="Demo.Detail" String="nested"/></Annotation></Add>"#,
    );
    roundtrip(
        r#"<Record><PropertyValue Property="P" Int="1"><Annotation Term="Demo.Note" String="one"><Annotation Term="Demo.Detail" String="nested"/></Annotation></PropertyValue><Annotation Term="Demo.Note" String="record"><Annotation Term="Demo.Detail" String="nested"/></Annotation></Record>"#,
    );
}

#[test]
fn annotations_on_annotations_roundtrip() {
    let source = xml(r#"<Int>2</Int><Annotation Term="Demo.Note" String="explanation"/>"#);
    let original = parser::from_xml_reader(source.as_bytes()).unwrap();
    assert_eq!(
        original.edmx.as_ref().unwrap().schemas[0].annotations[0]
            .annotations
            .len(),
        1
    );
    let json = original.to_json_value().to_string();
    assert!(json.contains("@Demo.Value@Demo.Note"));
    assert_eq!(original, parser::from_json_reader(json.as_bytes()).unwrap());
    let mut bytes = Vec::new();
    original.write_as(&mut bytes, CsdlFormat::Xml).unwrap();
    assert_eq!(original, parser::from_xml_reader(bytes.as_slice()).unwrap());
}

#[test]
fn malformed_expressions_do_not_fabricate_or_discard_values() {
    for expr in [
        "<Int/>",
        "<Bool/>",
        "<Float/>",
        "<Add><Int>1</Int></Add>",
        "<Neg/>",
        "<Int>1</Int><Int>2</Int>",
        r#"<Record><PropertyValue Property="P" Int="1"><Int>2</Int></PropertyValue></Record>"#,
    ] {
        assert!(
            parser::from_xml_reader(xml(expr).as_bytes()).is_err(),
            "{expr}"
        );
    }
    for expr in [
        r#"{"$Add":[1]}"#,
        r#"{"$Not":true,"$Neg":1}"#,
        r#"{"$Path":123}"#,
        r#"{"$Apply":[]}"#,
        r#"{"$LabeledElement":1}"#,
    ] {
        let json = format!(r#"{{"$Version":"4.01","Demo":{{"@Demo.Value":{expr}}}}}"#);
        assert!(parser::from_json_reader(json.as_bytes()).is_err(), "{expr}");
    }
}
