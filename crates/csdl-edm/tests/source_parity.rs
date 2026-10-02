use csdl_edm::{conformance::ValidationProfile, parser};

#[test]
fn json_cast_and_isof_boolean_controls_require_booleans() {
    for operator in ["Cast", "IsOf"] {
        for control in ["Nullable", "Unicode", "Collection"] {
            for (value, valid) in [("true", true), ("false", true), (r#""true""#, false), (r#""false""#, false), ("1", false), ("null", false)] {
                let json = format!(r#"{{"$Version":"4.01","N":{{"@N.Value":{{"${operator}":"value","$Type":"Edm.String","${control}":{value}}}}}}}"#);
                let result = parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict);
                assert_eq!(result.is_ok(), valid, "{operator}, {control}={value}: {result:?}");
            }
        }
    }
}

#[test]
fn json_cast_and_isof_numeric_facets_require_numbers() {
    for operator in ["Cast", "IsOf"] {
        for facet in ["MaxLength", "Precision", "Scale", "SRID"] {
            for (value, valid) in [("12", true), (r#""12""#, false), ("-1", false), ("1.5", false), ("null", false), ("true", false)] {
                let json = format!(r#"{{"$Version":"4.01","N":{{"@N.Value":{{"${operator}":1,"$Type":"Edm.Decimal","${facet}":{value}}}}}}}"#);
                let result = parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict);
                assert_eq!(result.is_ok(), valid, "{operator}, {facet}={value}: {result:?}");
            }
        }
    }
}

#[test]
fn cast_and_isof_symbolic_scale_values_are_case_sensitive() {
    for operator in ["Cast", "IsOf"] {
        for (scale, valid) in [("variable", true), ("floating", true), ("Variable", false), ("VARIABLE", false), ("Floating", false), ("FLOATING", false)] {
            let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Annotation Term="N.Value"><{operator} Type="Edm.Decimal" Scale="{scale}"><Decimal>1</Decimal></{operator}></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#);
            let json = format!(r#"{{"$Version":"4.01","N":{{"@N.Value":{{"${operator}":1,"$Type":"Edm.Decimal","$Scale":"{scale}"}}}}}}"#);
            for result in [
                parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict),
                parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict),
            ] {
                assert_eq!(result.is_ok(), valid, "{operator}, {scale}: {result:?}");
            }
        }
    }
}

#[test]
fn cast_and_isof_precision_require_nonnegative_integer_syntax() {
    for operator in ["Cast", "IsOf"] {
        for (precision, json_precision, valid) in [
            ("0", "0", true), ("12", "12", true), ("+12", "12", true), (" -0 ", "0", true),
            ("-1", "-1", false), ("1.5", "1.5", false),
            ("invalid", r#""invalid""#, false), ("", r#""""#, false),
        ] {
            let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Annotation Term="N.Value"><{operator} Type="Edm.Decimal" Precision="{precision}"><Decimal>1</Decimal></{operator}></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#);
            let json = format!(r#"{{"$Version":"4.01","N":{{"@N.Value":{{"${operator}":1,"$Type":"Edm.Decimal","$Precision":{json_precision}}}}}}}"#);
            for result in [
                parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict),
                parser::from_json_reader_with_profile(json.as_bytes(), ValidationProfile::Strict),
            ] {
                assert_eq!(result.is_ok(), valid, "{operator}, {precision:?}: {result:?}");
            }
        }
    }
}

#[test]
fn strict_xml_limits_inline_url_references_to_expression_value_locations() {
    for (body, valid) in [
        (r#"<Annotation Term="N.Value"><Collection UrlRef="value"/></Annotation>"#, false),
        (r#"<ComplexType Name="Owner"><Property Name="P" UrlRef="value"/></ComplexType>"#, false),
        (r#"<Annotation Term="N.Value" UrlRef="value"/>"#, true),
        (r#"<Annotation Term="N.Value"><LabeledElement Name="Label" UrlRef="value"/></Annotation>"#, true),
        (r#"<Annotation Term="N.Value"><Record><PropertyValue Property="P" UrlRef="value"/></Record></Annotation>"#, true),
    ] {
        let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{body}</Schema></edmx:DataServices></edmx:Edmx>"#);
        let result = parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict);
        assert_eq!(result.is_ok(), valid, "{body}: {result:?}");
        if valid {
            let expanded = xml.replace(" UrlRef=\"value\"/>", "><UrlRef><String>value</String></UrlRef></Annotation>");
            if body.starts_with("<Annotation Term=\"N.Value\" UrlRef=") {
                let expanded = parser::from_xml_reader_with_profile(expanded.as_bytes(), ValidationProfile::Strict).unwrap();
                assert_eq!(result.unwrap(), expanded);
            }
        }
    }
}

#[test]
fn strict_xml_limits_inline_paths_to_expression_value_locations() {
    for attribute in ["Path", "PropertyPath", "NavigationPropertyPath", "AnnotationPath", "ModelElementPath"] {
        for (body, valid) in [
            (format!(r#"<Annotation Term="N.Value"><Collection {attribute}="P"/></Annotation>"#), false),
            (format!(r#"<ComplexType Name="Owner"><Property Name="P" {attribute}="P"/></ComplexType>"#), false),
            (format!(r#"<Annotation Term="N.Value" {attribute}="P"/>"#), true),
            (format!(r#"<Annotation Term="N.Value"><LabeledElement Name="Label" {attribute}="P"/></Annotation>"#), true),
            (format!(r#"<Annotation Term="N.Value"><Record><PropertyValue Property="P" {attribute}="P"/></Record></Annotation>"#), true),
        ] {
            let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{body}</Schema></edmx:DataServices></edmx:Edmx>"#);
            let result = parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict);
            assert_eq!(result.is_ok(), valid, "{body}: {result:?}");
        }
    }
}

#[test]
fn strict_xml_limits_inline_constants_to_expression_value_locations() {
    for (body, valid) in [
        (r#"<Annotation Term="N.Value"><Collection String="ignored"><String>kept</String></Collection></Annotation>"#, false),
        (r#"<Annotation Term="N.Value"><Not Bool="true"><Bool>false</Bool></Not></Annotation>"#, false),
        (r#"<ComplexType Name="Owner"><Property Name="P" Type="Edm.Int32" Int="1"/></ComplexType>"#, false),
        (r#"<Annotation Term="N.Value" String="value"/>"#, true),
        (r#"<Annotation Term="N.Value"><LabeledElement Name="Label" String="value"/></Annotation>"#, true),
        (r#"<Annotation Term="N.Value"><Record><PropertyValue Property="P" String="value"/></Record></Annotation>"#, true),
    ] {
        let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{body}</Schema></edmx:DataServices></edmx:Edmx>"#);
        let result = parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict);
        assert_eq!(result.is_ok(), valid, "{body}: {result:?}");
    }
}

#[test]
fn strict_xml_checks_known_attributes_on_expression_elements() {
    for (expression, valid) in [
        (r#"<Collection Name="Wrong"><Int>1</Int></Collection>"#, false),
        (r#"<Record Function="N.Wrong"/>"#, false),
        (r#"<String Nullable="true">value</String>"#, false),
        (r#"<Apply Function="odata.concat" Type="Edm.String"><String>a</String><String>b</String></Apply>"#, false),
        (r#"<Cast Type="Edm.String" Nullable="true"><String>a</String></Cast>"#, true),
        (r#"<Record Type="N.R"><PropertyValue Property="P" String="value"/></Record>"#, true),
        (r#"<LabeledElement Name="Label" Path="P"/>"#, true),
        (r#"<String FutureAttribute="ignored">value</String>"#, true),
    ] {
        let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Annotation Term="N.Value">{expression}</Annotation></Schema></edmx:DataServices></edmx:Edmx>"#);
        let result = parser::from_xml_reader_with_profile(xml.as_bytes(), ValidationProfile::Strict);
        assert_eq!(result.is_ok(), valid, "{expression}: {result:?}");
    }
}

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
