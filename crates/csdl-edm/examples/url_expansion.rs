use csdl_edm::csdl::*;
use csdl_edm::path_expansion::{PathExpander, SpawnedPath};
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::validate_document;

fn main() {
    let schema = build_demo_schema();
    let document = Edmx {
        entity_container: None,
        version: Some("4.01".to_owned()),
        references: vec![],
        schemas: vec![schema],
    };

    let document_model = Resolver::resolve_document(document).expect("resolve document");
    validate_document(&document_model).expect("validate document");

    let model = document_model
        .schemas
        .first()
        .expect("document must contain at least one schema");
    let expander = PathExpander::new(3);
    let spawned_paths = expander.collect_paths(model.as_ref());
    print_spawned_paths(&spawned_paths, 3);
}

#[test]
fn demo_paths_survive_xml_and_json_serialization() {
    let document = CsdlDocument {
        edmx: Some(Edmx {
            entity_container: None,
            version: Some("4.01".into()),
            references: vec![],
            schemas: vec![build_demo_schema()],
        }),
    };
    let paths = |document: CsdlDocument| {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        PathExpander::new(3)
            .collect_paths(&model.schemas[0])
            .into_iter()
            .map(|p| {
                (
                    p.segments.join("/"),
                    p.terminal_type.name.clone(),
                    p.is_collection,
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = paths(document.clone());
    assert!(expected.iter().any(|p| p.0 == "customers" && p.2));
    assert!(expected.iter().any(|p| p.0 == "me" && !p.2));
    for format in [csdl_edm::CsdlFormat::Xml, csdl_edm::CsdlFormat::Json] {
        let mut bytes = Vec::new();
        document.write_as(&mut bytes, format).unwrap();
        let parsed = match format {
            csdl_edm::CsdlFormat::Xml => csdl_edm::parser::from_xml_reader(bytes.as_slice()),
            csdl_edm::CsdlFormat::Json => csdl_edm::parser::from_json_reader(bytes.as_slice()),
        }
        .unwrap();
        assert_eq!(paths(parsed), expected);
    }
}

fn build_demo_schema() -> Schema {
    Schema {
        external_annotations: Vec::new(),
        namespace: "Demo".to_owned(),
        alias: Some("D".to_owned()),
        annotations: vec![],
        elements: vec![
            SchemaElement::EnumType(EnumType {
                name: "Color".to_owned(),
                underlying_type: None,
                is_flags: None,
                members: vec![
                    EnumMember {
                        name: "Red".to_owned(),
                        value: Some(1),
                        annotations: vec![],
                    },
                    EnumMember {
                        name: "Green".to_owned(),
                        value: Some(2),
                        annotations: vec![],
                    },
                    EnumMember {
                        name: "Blue".to_owned(),
                        value: Some(3),
                        annotations: vec![],
                    },
                ],
                annotations: vec![],
            }),
            SchemaElement::TypeDefinition(TypeDefinition {
                name: "CustomerCode".to_owned(),
                underlying_type: "Edm.String".to_owned(),
                max_length: None,
                precision: None,
                scale: None,
                srid: None,
                unicode: None,
                annotations: vec![],
            }),
            SchemaElement::Term(Term {
                name: "DisplayName".to_owned(),
                type_name: Some("Edm.String".to_owned()),
                is_collection: false,
                base_term: None,
                default_value: None,
                applies_to: vec![],
                nullable: None,
                max_length: None,
                precision: None,
                scale: None,
                srid: None,
                unicode: None,
                annotations: vec![],
            }),
            SchemaElement::ComplexType(ComplexType {
                name: "Address".to_owned(),
                base_type: None,
                abstract_: Some(false),
                open_type: None,
                properties: vec![Property {
                    name: "street".to_owned(),
                    type_name: Some("Edm.String".to_owned()),
                    is_collection: false,
                    nullable: None,
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: vec![],
                }],
                navigation_properties: vec![],
                annotations: vec![],
            }),
            SchemaElement::EntityType(EntityType {
                name: "Customer".to_owned(),
                base_type: None,
                abstract_: Some(false),
                open_type: None,
                has_stream: None,
                key: Some(Key {
                    property_refs: vec![PropertyRef {
                        alias: None,
                        name: "id".to_owned(),
                    }],
                }),
                properties: vec![
                    Property {
                        name: "id".to_owned(),
                        type_name: Some("Edm.Int32".to_owned()),
                        is_collection: false,
                        nullable: Some(false),
                        max_length: None,
                        precision: None,
                        scale: None,
                        srid: None,
                        unicode: None,
                        default_value: None,
                        annotations: vec![],
                    },
                    Property {
                        name: "name".to_owned(),
                        type_name: Some("Edm.String".to_owned()),
                        is_collection: false,
                        nullable: None,
                        max_length: None,
                        precision: None,
                        scale: None,
                        srid: None,
                        unicode: None,
                        default_value: None,
                        annotations: vec![],
                    },
                    Property {
                        name: "address".to_owned(),
                        type_name: Some("D.Address".to_owned()),
                        is_collection: false,
                        nullable: None,
                        max_length: None,
                        precision: None,
                        scale: None,
                        srid: None,
                        unicode: None,
                        default_value: None,
                        annotations: vec![],
                    },
                ],
                navigation_properties: vec![NavigationProperty {
                    name: "orders".to_owned(),
                    type_name: Some("D.Order".to_owned()),
                    is_collection: true,
                    nullable: None,
                    partner: None,
                    contains_target: None,
                    on_delete: None,
                    referential_constraints: vec![],
                    annotations: vec![],
                }],
                annotations: vec![],
            }),
            SchemaElement::EntityType(EntityType {
                name: "Order".to_owned(),
                base_type: None,
                abstract_: Some(false),
                open_type: None,
                has_stream: None,
                key: Some(Key {
                    property_refs: vec![PropertyRef {
                        alias: None,
                        name: "id".to_owned(),
                    }],
                }),
                properties: vec![Property {
                    name: "id".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: vec![],
                }],
                navigation_properties: vec![NavigationProperty {
                    name: "customer".to_owned(),
                    type_name: Some("Customer".to_owned()),
                    is_collection: false,
                    nullable: None,
                    partner: None,
                    contains_target: None,
                    on_delete: None,
                    referential_constraints: vec![],
                    annotations: vec![],
                }],
                annotations: vec![],
            }),
            SchemaElement::EntityContainer(EntityContainer {
                name: "Default".to_owned(),
                extends: None,
                entity_sets: vec![
                    EntitySet {
                        name: "customers".to_owned(),
                        entity_type: Some("Customer".to_owned()),
                        include_in_service_document: None,
                        navigation_property_bindings: vec![],
                        annotations: vec![],
                    },
                    EntitySet {
                        name: "orders".to_owned(),
                        entity_type: Some("Order".to_owned()),
                        include_in_service_document: None,
                        navigation_property_bindings: vec![],
                        annotations: vec![],
                    },
                ],
                singletons: vec![Singleton {
                    name: "me".to_owned(),
                    type_name: Some("Customer".to_owned()),
                    nullable: None,
                    include_in_service_document: None,
                    navigation_property_bindings: vec![],
                    annotations: vec![],
                }],
                function_imports: vec![],
                action_imports: vec![],
                annotations: vec![],
            }),
        ],
    }
}

fn print_spawned_paths(paths: &[SpawnedPath], max_key_segments: usize) {
    println!("\nSpawned URL paths (key segments < {}):", max_key_segments);

    if paths.is_empty() {
        println!("  <no spawned paths>");
        return;
    }

    for path in paths {
        println!(
            "  {} -> {} ({})",
            path.segments.join("/"),
            path.terminal_type.name,
            if path.is_collection {
                "collection"
            } else {
                "single"
            }
        );
    }
}
