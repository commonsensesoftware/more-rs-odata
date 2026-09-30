#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{parser, resolver, validator};

fn issues(ty: &str, expression: &str, declarations: &str) -> Vec<validator::AnnotationDiagnostic> {
    let source = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N" Alias="Self"><Term Name="Value" Type="{ty}"/>{declarations}<Annotation Term="N.Value">{expression}</Annotation></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let model = resolver::Resolver::resolve_document(
        parser::from_xml_reader(source.as_bytes())
            .unwrap()
            .edmx
            .unwrap(),
    )
    .unwrap();
    validator::validate_annotations(&model)
}

#[test]
fn nested_records_and_facets_are_checked() {
    let declarations = r#"<ComplexType Name="R"><Property Name="Count" Type="Edm.Byte" Nullable="false"/><Property Name="Name" Type="Edm.String" MaxLength="3"/></ComplexType>"#;
    for expr in [
        r#"<Record><PropertyValue Property="Count" Int="256"/></Record>"#,
        "<Record/>",
        r#"<Record><PropertyValue Property="Count" Int="1"/><PropertyValue Property="Name" String="long"/></Record>"#,
        r#"<Record><PropertyValue Property="Count" Int="1"/><PropertyValue Property="Extra" Int="2"/></Record>"#,
    ] {
        assert!(!issues("N.R", expr, declarations).is_empty(), "{expr}");
    }
    assert!(
        issues(
            "N.R",
            r#"<Record Type="Self.R"><PropertyValue Property="Count" Int="1"/></Record>"#,
            declarations
        )
        .is_empty()
    );
}

#[test]
fn operators_and_conditionals_check_operands_and_results() {
    for (ty, expr, valid) in [
        ("Edm.Int64", "<Add><Int>1</Int><Int>2</Int></Add>", true),
        ("Edm.Boolean", "<Add><Int>1</Int><Int>2</Int></Add>", false),
        (
            "Edm.Int64",
            "<Add><String>x</String><Int>2</Int></Add>",
            false,
        ),
        ("Edm.Boolean", "<Not><String>x</String></Not>", false),
        (
            "Edm.Boolean",
            "<In><Int>1</Int><Collection><Int>2</Int></Collection></In>",
            true,
        ),
        (
            "Edm.Boolean",
            "<In><Int>1</Int><Collection><String>x</String></Collection></In>",
            false,
        ),
        (
            "Edm.String",
            "<If><Bool>true</Bool><String>yes</String><Int>1</Int></If>",
            false,
        ),
        ("Edm.Date", "<Date>2025-02-30</Date>", false),
        ("Edm.Date", "<Date>2024-02-29</Date>", true),
    ] {
        assert_eq!(issues(ty, expr, "").is_empty(), valid, "{expr}");
    }
}

#[test]
fn enum_members_functions_casts_and_labels_use_resolver_bindings() {
    let declarations = r#"<EnumType Name="Flags" IsFlags="true"><Member Name="A" Value="1"/><Member Name="B" Value="2"/></EnumType><Function Name="Twice"><Parameter Name="x" Type="Edm.Int64"/><ReturnType Type="Edm.Int64"/></Function>"#;
    for (ty, expr, valid) in [
        (
            "N.Flags",
            "<EnumMember>Self.Flags/A N.Flags/B</EnumMember>",
            true,
        ),
        ("N.Flags", "<EnumMember>N.Flags/Missing</EnumMember>", false),
        (
            "Edm.Int64",
            r#"<Apply Function="Self.Twice"><Int>2</Int></Apply>"#,
            true,
        ),
        (
            "Edm.Int64",
            r#"<Apply Function="N.Twice"><Bool>true</Bool></Apply>"#,
            false,
        ),
        (
            "Edm.String",
            r#"<Apply Function="odata.concat"><String>x</String><Int>1</Int></Apply>"#,
            true,
        ),
        (
            "Edm.String",
            r#"<Cast Type="Edm.String" MaxLength="2"><String>long</String></Cast>"#,
            false,
        ),
        (
            "Edm.Int64",
            r#"<Cast Type="N.Missing"><Int>1</Int></Cast>"#,
            false,
        ),
        (
            "Edm.Int64",
            r#"<Add><LabeledElement Name="X"><Int>2</Int></LabeledElement><LabeledElementReference>Self.X</LabeledElementReference></Add>"#,
            true,
        ),
        (
            "Edm.Int64",
            "<LabeledElementReference>N.Missing</LabeledElementReference>",
            false,
        ),
    ] {
        assert_eq!(issues(ty, expr, declarations).is_empty(), valid, "{expr}");
    }
}

#[test]
fn path_result_types_are_checked_without_reparsing_paths() {
    let declarations = r#"<ComplexType Name="R"><Property Name="Name" Type="Edm.String"/><Annotation Term="N.Value"><Path>Name</Path></Annotation></ComplexType>"#;
    assert!(!issues("Edm.Int64", "<Int>1</Int>", declarations).is_empty());
    assert!(issues("Edm.String", "<String>x</String>", declarations).is_empty());
}

#[test]
fn json_diagnostics_distinguish_strict_authoring_from_tolerant_consumption() {
    use csdl_edm::conformance::ValidationProfile;
    for (expression, valid) in [(r#"{"$Add":[1,2]}"#, true), (r#"{"$Add":[1,"bad"]}"#, false), (r#"{"$If":[true,1,"bad"]}"#, false)] {
        let source = format!(r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"Edm.Int64"}},"@N.Value":{expression}}}}}"#);
        let model = resolver::Resolver::resolve_document(parser::from_json_reader(source.as_bytes()).unwrap().edmx.unwrap()).unwrap();
        assert_eq!(validator::validate_document_with_profile(&model, ValidationProfile::Strict).is_valid(), valid);
        assert!(validator::validate_document_with_profile(&model, ValidationProfile::Tolerant).is_valid());
    }
}
