use csdl_edm::{conformance::ValidationProfile as P, parser};

fn source(reference: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Detail"><Property Name="Code" Type="Edm.Int32" Nullable="false"/></ComplexType><EntityType Name="Owner"><Key>{reference}</Key><Property Name="Id" Type="Edm.Int32" Nullable="false"/><Property Name="Detail" Type="N.Detail" Nullable="false"/></EntityType></Schema></edmx:DataServices></edmx:Edmx>"#
    )
}

#[test]
fn xml_property_references_require_name_and_allow_alias() {
    for reference in [
        r#"<PropertyRef Name="Id"/>"#,
        r#"<PropertyRef Name="Detail/Code" Alias="Code"/>"#,
    ] {
        let document =
            parser::from_xml_reader_with_profile(source(reference).as_bytes(), P::Strict).unwrap();
        let json = document.to_json_value();
        let expected = if reference.contains("Alias") {
            serde_json::json!([{"Code": "Detail/Code"}])
        } else {
            serde_json::json!(["Id"])
        };
        assert_eq!(json["N"]["Owner"]["$Key"], expected);
    }
    for reference in [r#"<PropertyRef/>"#, r#"<PropertyRef Alias="Code"/>"#] {
        assert!(
            parser::from_xml_reader_with_profile(source(reference).as_bytes(), P::Strict).is_err(),
            "{reference}"
        );
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn direct_and_aliased_xml_key_declarations_resolve_and_validate() {
    for reference in [
        r#"<PropertyRef Name="Id"/>"#,
        r#"<PropertyRef Name="Detail/Code" Alias="Code"/>"#,
    ] {
        let document =
            parser::from_xml_reader_with_profile(source(reference).as_bytes(), P::Strict).unwrap();
        let model = csdl_edm::resolver::Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        csdl_edm::validator::validate_document(&model).unwrap();
    }
}
