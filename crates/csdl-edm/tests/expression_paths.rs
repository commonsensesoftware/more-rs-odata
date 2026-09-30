#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{CsdlFormat, parser, resolver::Resolver, validator};

fn check(path: &str, ty: &str, collection: bool) -> Vec<validator::AnnotationDiagnostic> {
    let source = serde_json::json!({
        "$Version":"4.01",
        "Org.OData.Core.V1":{"$Alias":"Core","Ordered":{"$Kind":"Term","$Type":"Edm.Boolean"}},
        "N":{
            "Value":{"$Kind":"Term","$Type":ty,"$Collection":collection},
            "Address":{"$Kind":"ComplexType","Street":{"$Type":"Edm.String"},"Tags":{"$Type":"Edm.String","$Collection":true}},
            "E":{"$Kind":"EntityType","$HasStream":true,"$Key":["ID"],"ID":{"$Type":"Edm.Int32","$Nullable":false},"Name":{"$Type":"Edm.String"},"Addresses":{"$Type":"N.Address","$Collection":true,"$Nullable":false,"@Core.Ordered":true},"Unordered":{"$Type":"Edm.String","$Collection":true,"$Nullable":false},"Nav":{"$Kind":"NavigationProperty","$Type":"N.E","$Collection":true},"@N.Value":{"$Path":path}},
            "C":{"$Kind":"EntityContainer","Items":{"$Type":"N.E","$Collection":true}}
        }
    });
    let doc = parser::from_json_reader(source.to_string().as_bytes()).unwrap();
    let mut result = None;
    for format in [CsdlFormat::Json, CsdlFormat::Xml] {
        let mut bytes = Vec::new();
        doc.write_as(&mut bytes, format).unwrap();
        let parsed = match format {
            CsdlFormat::Json => parser::from_json_reader(bytes.as_slice()),
            CsdlFormat::Xml => parser::from_xml_reader(bytes.as_slice()),
        }
        .unwrap();
        let model = Resolver::resolve_document(parsed.edmx.unwrap()).unwrap();
        let diagnostics = validator::validate_annotations(&model);
        if let Some(previous) = &result {
            assert_eq!(*previous, diagnostics.is_empty(), "{path}: {diagnostics:?}");
        }
        result = Some(diagnostics.is_empty());
        if !diagnostics.is_empty() {
            return diagnostics;
        }
    }
    Vec::new()
}

#[test]
fn instance_selectors_counts_and_empty_paths_have_correct_multiplicity() {
    for (path, ty, collection) in [
        ("", "N.E", false),
        ("Addresses/$count", "Edm.Int64", false),
        ("Addresses/0/Street", "Edm.String", false),
        ("Addresses/-1/Street", "Edm.String", false),
        ("Addresses/Street", "Edm.String", true),
        ("Addresses/0/Tags", "Edm.String", true),
        ("Nav(1)/Name", "Edm.String", false),
        ("Nav(ID=ID)/Name", "Edm.String", false),
        ("/N.C/Items(1)/Name", "Edm.String", false),
        ("@odata.mediaContentType", "Edm.String", false),
    ] {
        assert!(
            check(path, ty, collection).is_empty(),
            "{path}: {:?}",
            check(path, ty, collection)
        );
    }
}

#[test]
fn malformed_or_semantically_invalid_selectors_are_diagnosed() {
    for path in [
        "Addresses/Tags",
        "Unordered/0",
        "Name/$count",
        "Addresses/$count/Name",
        "Nav('wrong')/Name",
        "Nav(ID=Name)/Name",
        "Nav(Other=1)/Name",
        "Nav(ID=1,ID=2)/Name",
        "Nav(1",
        "Addresses//Street",
        "@odata.unknown",
    ] {
        assert!(!check(path, "Edm.String", false).is_empty(), "{path}");
    }
}

#[test]
fn operation_and_external_property_paths_use_the_specified_starting_context() {
    let source = r#"{"$Version":"4.01","N":{"Value":{"$Kind":"Term","$Type":"Edm.String"},"Inner":{"$Kind":"ComplexType","Local":{"$Type":"Edm.String"}},"Outer":{"$Kind":"ComplexType","RootValue":{"$Type":"Edm.String"},"Nested":{"$Type":"N.Inner"}},"F":[{"$Kind":"Function","$Parameter":[{"$Name":"p","$Type":"N.Outer","@N.Value":{"$Path":"p/RootValue"}}],"$ReturnType":{"$Type":"N.Outer","@N.Value":{"$Path":"$ReturnType/RootValue"}}}],"$Annotations":{"N.Outer/Nested/Local":{"@N.Value":{"$Path":"RootValue"}}}}}"#;
    let doc = parser::from_json_reader(source.as_bytes()).unwrap();
    let model = Resolver::resolve_document(doc.edmx.unwrap()).unwrap();
    assert!(
        validator::validate_annotations(&model).is_empty(),
        "{:?}",
        validator::validate_annotations(&model)
    );
}

#[test]
fn model_paths_allow_multiple_collections_and_navigation_annotation_suffixes() {
    let source = r#"{"$Version":"4.01","N":{"Value":{"$Kind":"Term","$Type":"Edm.PropertyPath"},"Text":{"$Kind":"Term","$Type":"Edm.String"},"Annotation":{"$Kind":"Term","$Type":"Edm.AnnotationPath"},"E":{"$Kind":"EntityType","Name":{"$Type":"Edm.String"},"Nav":{"$Kind":"NavigationProperty","$Type":"N.E","$Collection":true,"@N.Text":"label"},"@N.Value":{"$PropertyPath":"Nav/Nav/Name"},"@N.Annotation":{"$AnnotationPath":"Nav@N.Text"}}}}"#;
    let doc = parser::from_json_reader(source.as_bytes()).unwrap();
    let model = Resolver::resolve_document(doc.edmx.unwrap()).unwrap();
    assert!(
        validator::validate_annotations(&model).is_empty(),
        "{:?}",
        validator::validate_annotations(&model)
    );
}

#[test]
fn new_path_syntax_is_version_gated_in_both_sources() {
    use csdl_edm::conformance::ValidationProfile;
    for path in ["Names/$count", "Names/0", "Items(1)/Name", "/N.C/Items"] {
        for version in ["4.0", "4.01"] {
            let json =
                serde_json::json!({"$Version":version,"N":{"@N.T":{"$Path":path}}}).to_string();
            let xml = format!(
                r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="{version}"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Annotation Term="N.T"><Path>{path}</Path></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
            );
            assert_eq!(
                parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict)
                    .is_ok(),
                version == "4.01"
            );
            assert_eq!(
                parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict)
                    .is_ok(),
                version == "4.01"
            );
        }
    }
}
