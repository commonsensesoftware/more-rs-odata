use csdl_edm::{conformance::ValidationProfile as P, parser};

#[test]
fn xml_collection_nullability_is_required_for_401_authors_and_preserved_for_consumers() {
    for version in ["4.0", "4.01"] {
        for collection in [false, true] {
            for nullable in [None, Some(false), Some(true)] {
                let ty = if collection {
                    "Collection(Edm.String)"
                } else {
                    "Edm.String"
                };
                let attribute = nullable
                    .map(|v| format!(r#" Nullable="{v}""#))
                    .unwrap_or_default();
                let source = format!(
                    r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="{version}"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Owner"><Property Name="Value" Type="{ty}"{attribute}/></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
                );
                for profile in [P::Strict, P::Tolerant] {
                    let result = parser::from_xml_reader_with_profile(source.as_bytes(), profile);
                    let rejected = profile == P::Strict
                        && version == "4.01"
                        && collection
                        && nullable.is_none();
                    assert_eq!(
                        result.is_err(),
                        rejected,
                        "{version} {collection} {nullable:?} {profile:?}"
                    );
                    if rejected {
                        continue;
                    }
                    let document = result.unwrap();
                    let csdl_edm::csdl::SchemaElement::ComplexType(owner) =
                        &document.edmx.as_ref().unwrap().schemas[0].elements[0]
                    else {
                        panic!("owner was lost")
                    };
                    assert_eq!(owner.properties[0].nullable, nullable);
                    assert_eq!(owner.properties[0].is_collection, collection);
                    #[cfg(all(feature = "resolver", feature = "validator"))]
                    {
                        let model =
                            csdl_edm::resolver::Resolver::resolve_document(document.edmx.unwrap())
                                .unwrap();
                        csdl_edm::validator::validate_document(&model).unwrap();
                        let csdl_edm::edm::SchemaElement::ComplexType(owner) =
                            model.schemas[0].elements[0].as_ref()
                        else {
                            panic!("resolved owner was lost")
                        };
                        let property = &owner.properties.get().unwrap()[0];
                        assert_eq!(property.nullable, nullable);
                        assert_eq!(property.is_collection, collection);
                    }
                }
            }
        }
    }
}
