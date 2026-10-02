use csdl_edm::{conformance::ValidationProfile, parser};

#[test]
fn xml_declaration_text_rules_apply_to_decoded_references() {
    for (text, valid) in [
        (" \t\n", true),
        ("&#32;&#9;&#10;&#13;", true),
        ("unexpected", false),
        ("&#65;", false),
        ("&amp;", false),
        ("&#xA0;", false),
    ] {
        for nested in [false, true] {
            let body = if nested {
                format!("<ComplexType Name=\"Owner\">{text}</ComplexType>")
            } else {
                text.to_owned()
            };
            let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{body}</Schema></edmx:DataServices></edmx:Edmx>"#);
            let result = parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict);
            assert_eq!(result.is_ok(), valid, "nested={nested}: {text:?}: {result:?}");
        }
    }
}

fn strict(json: &str) -> bool {
    parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict).is_ok()
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn paired_formats_preserve_parameter_order_and_effective_collection_defaults() {
    use csdl_edm::{CsdlFormat, edm::SchemaElement, resolver::Resolver};
    let json = r#"{"$Version":"4.01","N":{"F":[{"$Kind":"Function","$Parameter":[{"$Name":"z","$Type":"Edm.Int32","$Collection":true},{"$Name":"a","$Type":"Edm.Int32","$Collection":true,"$Nullable":false}],"$ReturnType":{"$Type":"Edm.Int32","$Collection":true}}]}}"#;
    let doc = parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict).unwrap();
    let model = Resolver::resolve_document(doc.edmx.clone().unwrap()).unwrap();
    let SchemaElement::Function(f) = model.schemas[0].elements[0].as_ref() else { panic!() };
    assert_eq!(f.parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["z", "a"]);
    assert!(!f.parameters[0].nullable_was_authored);
    assert!(f.parameters[1].nullable_was_authored);
    for format in [CsdlFormat::Xml, CsdlFormat::Json] {
        let mut output = Vec::new();
        doc.write_as_with_profile(&mut output, format, ValidationProfile::Strict).unwrap();
        let parsed = match format {
            CsdlFormat::Xml => parser::from_xml_reader(output.as_slice()),
            CsdlFormat::Json => parser::from_json_reader(output.as_slice()),
        }.unwrap();
        let model = Resolver::resolve_document(parsed.edmx.unwrap()).unwrap();
        let SchemaElement::Function(f) = model.schemas[0].elements[0].as_ref() else { panic!() };
        assert_eq!(f.parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["z", "a"]);
        assert!(f.parameters.iter().all(|p| p.nullable == Some(false)));
        assert_eq!(f.return_type.as_ref().unwrap().nullable, Some(false));
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn xml_entity_collection_returns_reject_only_authored_nullability() {
    use csdl_edm::{resolver::Resolver, validator};
    for nullable in ["", " Nullable=\"false\"", " Nullable=\"true\""] {
        let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><EntityType Name="E"/><Function Name="F"><ReturnType Type="Collection(N.E)"{nullable}/></Function></Schema></edmx:DataServices></edmx:Edmx>"#);
        let doc = parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict).unwrap();
        let model = Resolver::resolve_document(doc.edmx.unwrap()).unwrap();
        assert_eq!(validator::validate_document(&model).is_ok(), nullable.is_empty());
    }
}

#[test]
fn json_qualification_is_checked_without_treating_literal_strings_as_references() {
    for reference in ["N.R", "Self.R"] {
        let source = format!(
            r#"{{"$Version":"4.01","N":{{"$Alias":"Self","R":{{"$Kind":"ComplexType"}},"T":{{"$Kind":"Term","$Type":"{reference}"}},"@Self.Text":"N.R"}}}}"#
        );
        assert_eq!(strict(&source), reference == "Self.R");
        assert!(
            parser::from_json_reader_with_profile(source.as_bytes(), ValidationProfile::Tolerant)
                .is_ok()
        );
    }
    assert!(strict(
        r#"{"$Version":"4.01","$EntityContainer":"N.C","N":{"$Alias":"Self","C":{"$Kind":"EntityContainer"}}}"#
    ));
    assert!(!strict(
        r#"{"$Version":"4.01","$Reference":{"v.json":{"$Include":[{"$Namespace":"Vocabulary","$Alias":"V"}]}},"N":{"@Vocabulary.Term":true}}"#
    ));
    assert!(strict(
        r#"{"$Version":"4.01","$Reference":{"v.json":{"$Include":[{"$Namespace":"Vocabulary","$Alias":"V"}]}},"N":{"@V.Term":true}}"#
    ));
}

#[test]
fn implicit_json_owners_reject_misplaced_known_members() {
    for source in [
        r#"{"$Version":"4.01","N":{"C":{"$Kind":"ComplexType","P":{"$IsBound":true}}}}"#,
        r#"{"$Version":"4.01","N":{"F":[{"$Kind":"Function","$ReturnType":{"$OpenType":true}}]}}"#,
        r#"{"$Version":"4.01","N":{"$Collection":true}}"#,
        r#"{"$Version":"4.01","N":{"C":{"$Kind":"ComplexType","$Parameter":[]}}}"#,
        r#"{"$Version":"4.01","N":{"C":{"$Kind":"ComplexType","P":42}}}"#,
        r#"{"$Version":"4.01","N":{"C":{"$Kind":"ComplexType","P":{"$Kind":"Term"}}}}"#,
        r#"{"$Version":"4.01","N":{"C":{"$Kind":"ComplexType","P":{"$MaxLength":"10"}}}}"#,
        r#"{"$Version":"4.01","N":{"C":{"$Kind":"ComplexType","P":{"$Precision":true}}}}"#,
        r#"{"$Version":"4.01","N":{"E":{"$Kind":"EntityType","$Key":[{"A":"One","B":"Two"}]}}}"#,
        r#"{"$Version":"4.01","N":{"E":{"$Kind":"EntityType","Nav":{"$Kind":"NavigationProperty","$Type":"N.E","$ReferentialConstraint":{"P":42}}}}}"#,
        r#"{"$Version":"4.01","N":{"E":{"$Kind":"EntityType","Nav":{"$Kind":"NavigationProperty","$Type":"N.E","$OnDelete":"Maybe"}}}}"#,
    ] {
        assert!(!strict(source), "{source}");
    }
}

#[test]
fn local_binding_targets_must_omit_the_container_prefix() {
    for target in ["Items", "Self.C/Items", "N.C/Items", "Self.Other/Items"] {
        let source = format!(
            r#"{{"$Version":"4.01","N":{{"$Alias":"Self","C":{{"$Kind":"EntityContainer","Items":{{"$Collection":true,"$Type":"Self.E","$NavigationPropertyBinding":{{"Nav":"{target}"}}}}}}}}}}"#
        );
        assert_eq!(
            strict(&source),
            matches!(target, "Items" | "Self.Other/Items"),
            "{target}"
        );
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn return_collection_nullability_preserves_authored_and_defaulted_values() {
    use csdl_edm::{edm::SchemaElement, resolver::Resolver, validator};
    for (nullable, valid) in [
        ("", true),
        (",\"$Nullable\":false", false),
        (",\"$Nullable\":true", false),
    ] {
        let source = format!(
            r#"{{"$Version":"4.01","N":{{"E":{{"$Kind":"EntityType"}},"F":[{{"$Kind":"Function","$ReturnType":{{"$Type":"N.E","$Collection":true{nullable}}}}}]}}}}"#
        );
        let doc = parser::from_json_reader(source.as_bytes()).unwrap();
        let model = Resolver::resolve_document(doc.edmx.clone().unwrap()).unwrap();
        assert_eq!(validator::validate_document(&model).is_ok(), valid);
        let SchemaElement::Function(f) = model.schemas[0].elements[1].as_ref() else {
            panic!()
        };
        let r = f.return_type.as_ref().unwrap();
        assert_eq!(r.nullable_was_authored, !nullable.is_empty());
        if nullable.is_empty() {
            assert_eq!(r.nullable, Some(false));
            assert!(
                doc.to_json_value()["N"]["F"][0]["$ReturnType"]
                    .get("$Nullable")
                    .is_none()
            );
        }
    }
}
