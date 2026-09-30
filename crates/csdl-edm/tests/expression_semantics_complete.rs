#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{edm::*, parser, resolver::Resolver, validator::*};

fn model(source: &str) -> DocumentModel {
    Resolver::resolve_document(
        parser::from_json_reader(source.as_bytes())
            .unwrap()
            .edmx
            .unwrap(),
    )
    .unwrap()
}
fn supplied(ty: PrimitiveType) -> ExpressionType {
    ExpressionType {
        ty: TermType::Primitive(ty),
        is_collection: false,
        is_ordered: false,
        nullable: false,
        facets: TypeFacets::default(),
    }
}

#[test]
fn url_results_require_external_types_and_client_functions_accept_supplied_signatures() {
    let m = model(
        r#"{"$Version":"4.01","N":{"Number":{"$Kind":"Term","$Type":"Edm.Int32"},"@N.Number":{"$UrlRef":"https://example.test/value"}}}"#,
    );
    let unknown = validate_annotations(&m);
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].issue, AnnotationIssue::UnknownExpressionType);
    let mut options = AnnotationValidationOptions::default();
    options.url_result_types.insert(
        "https://example.test/value".into(),
        supplied(PrimitiveType::Int32),
    );
    assert!(validate_annotations_with_options(&m, &options).is_empty());
    options.url_result_types.insert(
        "https://example.test/value".into(),
        supplied(PrimitiveType::String),
    );
    assert!(
        validate_annotations_with_options(&m, &options)
            .iter()
            .any(|d| d.issue == AnnotationIssue::IncompatibleValue)
    );
    let m = model(
        r#"{"$Version":"4.01","N":{"Number":{"$Kind":"Term","$Type":"Edm.Int32"},"@N.Number":{"$Apply":["a"],"$Function":"Client.measure"}}}"#,
    );
    assert!(!validate_annotations(&m).is_empty());
    options.client_functions.insert(
        "Client.measure".into(),
        vec![ClientFunctionSignature {
            parameters: vec![supplied(PrimitiveType::String)],
            return_type: supplied(PrimitiveType::Int32),
        }],
    );
    assert!(validate_annotations_with_options(&m, &options).is_empty());
}

#[test]
fn base_term_only_supplies_members_present_in_its_record() {
    for (record, valid) in [("{}", false), (r#"{"B":"b"}"#, true)] {
        let json = format!(
            r#"{{"$Version":"4.01","N":{{"Base":{{"$Kind":"ComplexType","A":{{"$Type":"Edm.String","$Nullable":false}}}},"Derived":{{"$Kind":"ComplexType","$BaseType":"N.Base","B":{{"$Type":"Edm.String","$Nullable":false}}}},"T":{{"$Kind":"Term","$Type":"N.Base"}},"D":{{"$Kind":"Term","$Type":"N.Derived","$BaseTerm":"N.T"}},"@N.T":{{"A":"a"}},"@N.D":{record}}}}}"#
        );
        let m = model(&json);
        assert_eq!(
            validate_annotations(&m).is_empty(),
            valid,
            "{:?}",
            validate_annotations(&m)
        );
    }
}

#[test]
fn numeric_enum_literals_obey_underlying_ranges_and_flag_signs() {
    for (value, flags, valid) in [
        ("3", true, true),
        ("128", true, false),
        ("-1", true, false),
        ("-1", false, true),
        ("A,2", true, true),
        ("A,2", false, false),
    ] {
        let json = serde_json::json!({"$Version":"4.01","N":{"E":{"$Kind":"EnumType","$UnderlyingType":"Edm.SByte","$IsFlags":flags,"A":1},"T":{"$Kind":"Term","$Type":"N.E"},"@N.T":value}});
        assert_eq!(
            validate_annotations(&model(&json.to_string())).is_empty(),
            valid,
            "{value}"
        );
    }
}

#[test]
fn canonical_collection_functions_and_numeric_promotion() {
    for (function, args, ty, valid) in [
        ("odata.length", "[[1,2]]", "Edm.Int32", true),
        ("odata.hassubset", "[[1,2],[2]]", "Edm.Boolean", true),
        ("odata.hassubsequence", "[[1,2],[2]]", "Edm.Boolean", true),
        ("odata.contains", "[[1,2],[2]]", "Edm.Boolean", true),
        ("odata.indexof", "[[1,2],[2]]", "Edm.Int32", true),
        ("odata.hassubset", "[1,2]", "Edm.Boolean", false),
        ("odata.case", "[true,1,false,2]", "Edm.Int64", true),
        ("odata.cast", "[1,\"Edm.Double\"]", "Edm.Double", true),
    ] {
        let source = format!(
            r#"{{"$Version":"4.01","N":{{"T":{{"$Kind":"Term","$Type":"{ty}"}},"@N.T":{{"$Apply":{args},"$Function":"{function}"}}}}}}"#
        );
        assert_eq!(
            validate_annotations(&model(&source)).is_empty(),
            valid,
            "{function}"
        );
    }
    let m = model(
        r#"{"$Version":"4.01","N":{"T":{"$Kind":"Term","$Type":"Edm.Single"},"C":{"$Kind":"ComplexType","A":{"$Type":"Edm.Single"},"B":{"$Type":"Edm.Int32"},"@N.T":{"$Add":[{"$Path":"A"},{"$Path":"B"}]}}}}"#,
    );
    assert!(
        validate_annotations(&m).is_empty(),
        "{:?}",
        validate_annotations(&m)
    );
}

#[test]
fn xml_and_json_literal_spelling_rules_are_retained() {
    let json =
        model(r#"{"$Version":"4.01","N":{"T":{"$Kind":"Term","$Type":"Edm.Int64"},"@N.T":"42"}}"#);
    assert!(validate_annotations(&json).is_empty());
    for (expression, valid) in [("<String>42</String>", false), ("<Int>42</Int>", true)] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="T" Type="Edm.Int64"/><Annotation Term="N.T">{expression}</Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        let m = Resolver::resolve_document(
            parser::from_xml_reader(xml.as_bytes())
                .unwrap()
                .edmx
                .unwrap(),
        )
        .unwrap();
        assert_eq!(validate_annotations(&m).is_empty(), valid);
    }
}

#[test]
fn spatial_signatures_and_type_tests_check_types_without_evaluating_values() {
    for (function, args, ty, valid) in [
        (
            "odata.geo.distance",
            r#"[{"$Path":"Point"},{"$Path":"Point"}]"#,
            "Edm.Double",
            true,
        ),
        (
            "odata.geo.intersects",
            r#"[{"$Path":"Point"},{"$Path":"Polygon"}]"#,
            "Edm.Boolean",
            true,
        ),
        (
            "odata.geo.intersects",
            r#"[{"$Path":"Polygon"},{"$Path":"Point"}]"#,
            "Edm.Boolean",
            false,
        ),
        (
            "odata.geo.length",
            r#"[{"$Path":"Line"}]"#,
            "Edm.Double",
            true,
        ),
        ("odata.isof", r#"["text","Edm.Int32"]"#, "Edm.Boolean", true),
    ] {
        let source = format!(
            r#"{{"$Version":"4.01","N":{{"T":{{"$Kind":"Term","$Type":"{ty}"}},"C":{{"$Kind":"ComplexType","Point":{{"$Type":"Edm.GeographyPoint"}},"Polygon":{{"$Type":"Edm.GeographyPolygon"}},"Line":{{"$Type":"Edm.GeographyLineString"}},"@N.T":{{"$Apply":{args},"$Function":"{function}"}}}}}}}}"#
        );
        assert_eq!(
            validate_annotations(&model(&source)).is_empty(),
            valid,
            "{function}"
        );
    }
}
