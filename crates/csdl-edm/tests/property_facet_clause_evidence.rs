use csdl_edm::{CsdlFormat, conformance::ValidationProfile as P, csdl::MaxLengthFacet, parser};

#[test]
fn legacy_xml_max_length_is_consumable_but_not_emitted_by_strict_401_authors() {
    for version in ["4.0", "4.01"] {
        for ty in ["Edm.String", "Edm.Binary"] {
            for (attribute, expected) in [
                ("", None),
                (r#" MaxLength="12""#, Some(MaxLengthFacet::Number(12))),
                (r#" MaxLength="max""#, Some(MaxLengthFacet::Max)),
            ] {
                let source = format!(
                    r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="{version}"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Owner"><Property Name="Value" Type="{ty}"{attribute}/></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
                );
                let rejected = version == "4.01" && expected == Some(MaxLengthFacet::Max);
                for profile in [P::Strict, P::Tolerant] {
                    let parsed = parser::from_xml_reader_with_profile(source.as_bytes(), profile);
                    assert_eq!(
                        parsed.is_err(),
                        rejected && profile == P::Strict,
                        "{version} {ty} {attribute} {profile:?}"
                    );
                    if parsed.is_err() {
                        continue;
                    }
                    let document = parsed.unwrap();
                    let csdl_edm::csdl::SchemaElement::ComplexType(owner) =
                        &document.edmx.as_ref().unwrap().schemas[0].elements[0]
                    else {
                        panic!("missing owner")
                    };
                    assert_eq!(owner.properties[0].max_length, expected);
                    let mut bytes = Vec::new();
                    let output =
                        document.write_as_with_profile(&mut bytes, CsdlFormat::Xml, P::Strict);
                    assert_eq!(output.is_err(), rejected);
                    if rejected {
                        assert!(
                            bytes.is_empty(),
                            "invalid author output must fail before writing"
                        );
                    } else {
                        let reparsed =
                            parser::from_xml_reader_with_profile(bytes.as_slice(), P::Strict)
                                .unwrap();
                        assert_eq!(reparsed, document);
                    }
                    #[cfg(all(feature = "resolver", feature = "validator"))]
                    {
                        let model =
                            csdl_edm::resolver::Resolver::resolve_document(document.edmx.unwrap())
                                .unwrap();
                        csdl_edm::validator::validate_document(&model).unwrap();
                        let csdl_edm::edm::SchemaElement::ComplexType(owner) =
                            model.schemas[0].elements[0].as_ref()
                        else {
                            panic!("missing resolved owner")
                        };
                        assert_eq!(
                            owner.properties.get().unwrap()[0].facets.max_length,
                            expected
                        );
                    }
                }
            }
        }
    }
}
