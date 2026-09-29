use std::fs;
use std::path::{Path, PathBuf};

use csdl_edm::CsdlDocument;
use csdl_edm::csdl;
use csdl_edm::edm::{EntityContainerElement, OnDeleteAction};
use csdl_edm::resolver::{ResolveError, Resolver};
use csdl_edm::validator::{ValidationError, validate_document};

#[derive(Debug, Clone, Copy)]
enum ExpectedOutcome {
    Pass,
    ValidateInvalidNavigation(&'static str),
    ResolveUnknownType(&'static str),
    ResolveMissingType {
        element_kind: &'static str,
        element_name: &'static str,
    },
    ValidateUnknownContainerTarget {
        source_kind: &'static str,
        source: &'static str,
        target: &'static str,
    },
}

#[test]
fn parses_all_input_fixtures() {
    for path in fixture_paths() {
        let parsed = CsdlDocument::from_path(&path)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
        assert!(
            parsed.edmx.is_some(),
            "fixture has no Edmx root: {}",
            path.display()
        );
    }
}
#[test]
fn validates_navigation_property_binding_path_and_target_semantics() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Category".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: vec![csdl::NavigationProperty {
                    name: "Products".to_owned(),
                    type_name: Some("Demo.Product".to_owned()),
                    is_collection: true,
                    nullable: None,
                    partner: None,
                    contains_target: Some(false),
                    on_delete: None,
                    referential_constraints: Vec::new(),
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Product".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: vec![csdl::NavigationProperty {
                    name: "Supplier".to_owned(),
                    type_name: Some("Demo.Supplier".to_owned()),
                    is_collection: false,
                    nullable: None,
                    partner: None,
                    contains_target: Some(false),
                    on_delete: None,
                    referential_constraints: Vec::new(),
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Supplier".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityContainer(csdl::EntityContainer {
                name: "Container".to_owned(),
                extends: None,
                entity_sets: vec![
                    csdl::EntitySet {
                        name: "Categories".to_owned(),
                        entity_type: Some("Demo.Category".to_owned()),
                        include_in_service_document: None,
                        navigation_property_bindings: vec![csdl::NavigationPropertyBinding {
                            path: "Products/Supplier".to_owned(),
                            target: "Products/Supplier".to_owned(),
                        }],
                        annotations: Vec::new(),
                    },
                    csdl::EntitySet {
                        name: "Products".to_owned(),
                        entity_type: Some("Demo.Product".to_owned()),
                        include_in_service_document: None,
                        navigation_property_bindings: Vec::new(),
                        annotations: Vec::new(),
                    },
                ],
                singletons: vec![csdl::Singleton {
                    name: "MainSupplier".to_owned(),
                    type_name: Some("Demo.Supplier".to_owned()),
                    nullable: None,
                    include_in_service_document: None,
                    navigation_property_bindings: Vec::new(),
                    annotations: Vec::new(),
                }],
                function_imports: Vec::new(),
                action_imports: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document =
        Resolver::resolve_edmx_document(edmx).expect("resolver should build binding structures");
    let errors = validate_document(&document)
        .expect_err("validation should detect invalid navigation property binding semantics");

    assert!(errors.iter().any(|error| {
                    matches!(
                        error,
                        ValidationError::InvalidNavigationPropertyBinding {
                            source_kind,
                            source,
                            attribute,
                            reason,
                            ..
                        } if *source_kind == "EntitySet.NavigationPropertyBinding"
                            && source == "Categories"
                            && *attribute == "Path"
                            && *reason == "Only containment navigation segments are allowed before the final segment"
                    )
                }));

    assert!(errors.iter().any(|error| {
        matches!(
            error,
            ValidationError::InvalidNavigationPropertyBinding {
                source_kind,
                source,
                attribute,
                reason,
                ..
            } if *source_kind == "EntitySet.NavigationPropertyBinding"
                && source == "Categories"
                && *attribute == "Target"
                && *reason == "Target paths with additional segments must start from a singleton"
        )
    }));
}

#[test]
fn resolve_and_validate_fixture_expectations() {
    let expectations: [(&str, ExpectedOutcome); 16] = [
        ("enum_sample.csdl.json", ExpectedOutcome::Pass),
        ("enum_sample.csdl.xml", ExpectedOutcome::Pass),
        ("example89.csdl.json", ExpectedOutcome::Pass),
        ("example89.csdl.xml", ExpectedOutcome::Pass),
        (
            "extras_sample.csdl.json",
            ExpectedOutcome::ValidateInvalidNavigation(
                "Referential constraints require a single-valued navigation",
            ),
        ),
        (
            "extras_sample.csdl.xml",
            ExpectedOutcome::ValidateInvalidNavigation(
                "Referential constraints require a single-valued navigation",
            ),
        ),
        (
            "fields_sample.csdl.json",
            ExpectedOutcome::ResolveUnknownType("FieldsDemo.BaseService"),
        ),
        (
            "fields_sample.csdl.xml",
            ExpectedOutcome::ResolveUnknownType("FieldsDemo.BaseService"),
        ),
        (
            "import_sample.csdl.json",
            ExpectedOutcome::ValidateUnknownContainerTarget {
                source_kind: "ActionImport.EntitySet",
                source: "MaintenanceMode",
                target: "Maintenance",
            },
        ),
        (
            "import_sample.csdl.xml",
            ExpectedOutcome::ValidateUnknownContainerTarget {
                source_kind: "ActionImport.EntitySet",
                source: "MaintenanceMode",
                target: "Maintenance",
            },
        ),
        ("record_sample.csdl.json", ExpectedOutcome::Pass),
        (
            "record_sample.csdl.xml",
            ExpectedOutcome::ResolveMissingType {
                element_kind: "Property",
                element_name: "ISBN",
            },
        ),
        (
            "reporting_line.csdl.json",
            ExpectedOutcome::ResolveUnknownType("self.EmployeeInfo"),
        ),
        (
            "reporting_line.csdl.xml",
            ExpectedOutcome::ResolveUnknownType("self.EmployeeInfo"),
        ),
        (
            "types_sample.csdl.json",
            ExpectedOutcome::ResolveUnknownType("self.Counter"),
        ),
        (
            "types_sample.csdl.xml",
            ExpectedOutcome::ResolveUnknownType("TypesDemo.Counter"),
        ),
    ];

    for (fixture_name, expected) in expectations {
        let path = fixture_path(fixture_name);

        let parsed = CsdlDocument::from_path(&path)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
        let edmx = parsed
            .edmx
            .unwrap_or_else(|| panic!("missing Edmx root in {}", path.display()));

        match Resolver::resolve_edmx_document(edmx) {
            Ok(model) => match validate_document(&model) {
                Ok(()) => {
                    assert!(
                        matches!(expected, ExpectedOutcome::Pass),
                        "{} resolved+validated successfully, expected {:?}",
                        fixture_name,
                        expected
                    );
                }
                Err(errors) => {
                    assert_expected_validation(fixture_name, expected, &errors);
                }
            },
            Err(error) => {
                assert_expected_resolve_error(fixture_name, expected, error);
            }
        }
    }
}

#[test]
fn resolves_entity_container_extends_and_inherits_elements() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Thing".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityContainer(csdl::EntityContainer {
                name: "DerivedContainer".to_owned(),
                extends: Some("Demo.BaseContainer".to_owned()),
                entity_sets: Vec::new(),
                singletons: vec![csdl::Singleton {
                    name: "PrimaryThing".to_owned(),
                    type_name: Some("Demo.Thing".to_owned()),
                    nullable: None,
                    include_in_service_document: None,
                    navigation_property_bindings: Vec::new(),
                    annotations: Vec::new(),
                }],
                function_imports: Vec::new(),
                action_imports: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityContainer(csdl::EntityContainer {
                name: "BaseContainer".to_owned(),
                extends: None,
                entity_sets: vec![csdl::EntitySet {
                    name: "Things".to_owned(),
                    entity_type: Some("Demo.Thing".to_owned()),
                    include_in_service_document: None,
                    navigation_property_bindings: Vec::new(),
                    annotations: Vec::new(),
                }],
                singletons: Vec::new(),
                function_imports: Vec::new(),
                action_imports: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx)
        .expect("resolver should support EntityContainer.Extends inheritance");
    let model = document.schemas.first().expect("resolved schema expected");
    let container = model
        .entity_container
        .as_ref()
        .expect("resolved entity container expected");

    assert_eq!(container.name, "DerivedContainer");
    assert_eq!(container.elements.len(), 2);

    let mut names = container
        .elements
        .iter()
        .map(|element| match element.as_ref() {
            EntityContainerElement::EntitySet(set) => set.name.clone(),
            EntityContainerElement::Singleton(singleton) => singleton.name.clone(),
            EntityContainerElement::FunctionImport(import_) => import_.name.clone(),
            EntityContainerElement::ActionImport(import_) => import_.name.clone(),
        })
        .collect::<Vec<_>>();
    names.sort();

    assert_eq!(names, vec!["PrimaryThing".to_owned(), "Things".to_owned()]);
}

#[test]
fn resolves_container_imports_and_navigation_bindings() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Thing".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Function(csdl::Function {
                name: "ListThings".to_owned(),
                is_bound: Some(false),
                is_composable: Some(false),
                entity_set_path: None,
                parameters: Vec::new(),
                return_type: Some(csdl::ReturnType {
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: None,
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    annotations: Vec::new(),
                }),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Action(csdl::Action {
                name: "Reset".to_owned(),
                is_bound: Some(false),
                entity_set_path: None,
                parameters: Vec::new(),
                return_type: None,
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityContainer(csdl::EntityContainer {
                name: "Container".to_owned(),
                extends: None,
                entity_sets: vec![csdl::EntitySet {
                    name: "Things".to_owned(),
                    entity_type: Some("Demo.Thing".to_owned()),
                    include_in_service_document: None,
                    navigation_property_bindings: vec![csdl::NavigationPropertyBinding {
                        path: "Nav".to_owned(),
                        target: "Things".to_owned(),
                    }],
                    annotations: Vec::new(),
                }],
                singletons: vec![csdl::Singleton {
                    name: "MainThing".to_owned(),
                    type_name: Some("Demo.Thing".to_owned()),
                    nullable: None,
                    include_in_service_document: None,
                    navigation_property_bindings: vec![csdl::NavigationPropertyBinding {
                        path: "Nav".to_owned(),
                        target: "Things".to_owned(),
                    }],
                    annotations: Vec::new(),
                }],
                function_imports: vec![csdl::FunctionImport {
                    name: "ListThings".to_owned(),
                    entity_set: Some("Things".to_owned()),
                    function: Some("Demo.ListThings".to_owned()),
                    include_in_service_document: None,
                    annotations: Vec::new(),
                }],
                action_imports: vec![csdl::ActionImport {
                    name: "Reset".to_owned(),
                    action: Some("Demo.Reset".to_owned()),
                    entity_set: None,
                    include_in_service_document: None,
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx)
        .expect("resolver should support container imports and navigation property bindings");
    let model = document.schemas.first().expect("resolved schema expected");
    let container = model
        .entity_container
        .as_ref()
        .expect("resolved entity container expected");

    assert_eq!(container.elements.len(), 4);

    let mut seen_names = Vec::new();
    for element in &container.elements {
        match element.as_ref() {
            EntityContainerElement::EntitySet(set) => {
                seen_names.push(set.name.clone());
                assert_eq!(set.navigation_property_bindings().len(), 1);
            }
            EntityContainerElement::Singleton(singleton) => {
                seen_names.push(singleton.name.clone());
                assert_eq!(singleton.navigation_property_bindings().len(), 1);
            }
            EntityContainerElement::FunctionImport(import_) => {
                seen_names.push(import_.name.clone());
                assert_eq!(import_.function, "Demo.ListThings");
            }
            EntityContainerElement::ActionImport(import_) => {
                seen_names.push(import_.name.clone());
                assert_eq!(import_.action, "Demo.Reset");
            }
        }
    }

    seen_names.sort();
    assert_eq!(
        seen_names,
        vec![
            "ListThings".to_owned(),
            "MainThing".to_owned(),
            "Reset".to_owned(),
            "Things".to_owned(),
        ]
    );
}

#[test]
fn resolves_navigation_semantics_fields() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Order".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: vec![csdl::NavigationProperty {
                    name: "Customer".to_owned(),
                    type_name: Some("Demo.Customer".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    partner: Some("Orders".to_owned()),
                    contains_target: Some(true),
                    on_delete: Some(csdl::OnDeleteAction::Cascade),
                    referential_constraints: vec![csdl::ReferentialConstraint {
                        property: "CustomerID".to_owned(),
                        referenced_property: "ID".to_owned(),
                        annotations: Vec::new(),
                    }],
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Customer".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx)
        .expect("resolver should support navigation semantics fields");
    let model = document.schemas.first().expect("resolved schema expected");

    let order_entity = model
        .elements
        .iter()
        .find_map(|element| match element.as_ref() {
            csdl_edm::edm::SchemaElement::EntityType(entity) if entity.name == "Order" => {
                Some(entity.clone())
            }
            _ => None,
        })
        .expect("Order entity expected");

    let nav = order_entity
        .navigation_properties()
        .first()
        .expect("Order.Customer navigation expected");

    assert_eq!(
        nav.partner().map(csdl_edm::edm::binding_path_to_string),
        Some("Orders".to_owned())
    );
    assert_eq!(nav.contains_target, Some(true));
    assert_eq!(nav.on_delete, Some(OnDeleteAction::Cascade));
    assert_eq!(nav.referential_constraints.len(), 1);
    assert_eq!(nav.referential_constraints[0].property, "CustomerID");
    assert_eq!(nav.referential_constraints[0].referenced_property, "ID");
}

#[test]
fn validates_navigation_partner_existence() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Order".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: vec![csdl::NavigationProperty {
                    name: "Customer".to_owned(),
                    type_name: Some("Demo.Customer".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    partner: Some("Orders".to_owned()),
                    contains_target: None,
                    on_delete: None,
                    referential_constraints: Vec::new(),
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Customer".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx)
        .expect("resolver should support navigation partner fields");
    let errors = validate_document(&document)
        .expect_err("validation should detect missing navigation partner");

    assert!(errors.iter().any(|error| matches!(
        error,
        ValidationError::UnknownNavigationPartner {
            entity,
            navigation,
            partner,
            target_entity,
        } if entity == "Order"
            && navigation == "Customer"
            && partner == "Orders"
            && target_entity == "Customer"
    )));
}

#[test]
fn validates_navigation_referential_constraints() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Order".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: vec![csdl::NavigationProperty {
                    name: "Customer".to_owned(),
                    type_name: Some("Demo.Customer".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    partner: None,
                    contains_target: None,
                    on_delete: None,
                    referential_constraints: vec![csdl::ReferentialConstraint {
                        property: "CustomerID".to_owned(),
                        referenced_property: "MissingID".to_owned(),
                        annotations: Vec::new(),
                    }],
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Customer".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx)
        .expect("resolver should support referential constraint fields");
    let errors = validate_document(&document)
        .expect_err("validation should detect bad referential constraints");

    assert!(errors.iter().any(|error| matches!(
        error,
        ValidationError::UnknownReferentialConstraintProperty {
            entity,
            navigation,
            property,
        } if entity == "Order" && navigation == "Customer" && property == "CustomerID"
    )));

    assert!(errors.iter().any(|error| matches!(
        error,
        ValidationError::UnknownReferentialConstraintReferencedProperty {
            entity,
            navigation,
            target_entity,
            referenced_property,
        } if entity == "Order"
            && navigation == "Customer"
            && target_entity == "Customer"
            && referenced_property == "MissingID"
    )));
}

#[test]
fn resolves_collection_valued_term() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![csdl::SchemaElement::Term(csdl::Term {
            name: "Tags".to_owned(),
            type_name: Some("Edm.String".to_owned()),
            is_collection: true,
            base_term: None,
            default_value: None,
            applies_to: Vec::new(),
            nullable: None,
            max_length: None,
            precision: None,
            scale: None,
            srid: None,
            unicode: None,
            annotations: Vec::new(),
        })],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document =
        Resolver::resolve_edmx_document(edmx).expect("resolver should support collection terms");
    let model = document.schemas.first().expect("resolved schema expected");
    let term = model
        .elements
        .iter()
        .find_map(|element| match element.as_ref() {
            csdl_edm::edm::SchemaElement::Term(term) if term.name == "Tags" => Some(term.clone()),
            _ => None,
        })
        .expect("Tags term expected");

    assert!(term.is_collection);
}

#[test]
fn validates_term_base_term_cycles() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::Term(csdl::Term {
                name: "A".to_owned(),
                type_name: Some("Edm.String".to_owned()),
                is_collection: false,
                base_term: Some("Demo.B".to_owned()),
                default_value: None,
                applies_to: Vec::new(),
                nullable: None,
                max_length: None,
                precision: None,
                scale: None,
                srid: None,
                unicode: None,
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Term(csdl::Term {
                name: "B".to_owned(),
                type_name: Some("Edm.String".to_owned()),
                is_collection: false,
                base_term: Some("Demo.A".to_owned()),
                default_value: None,
                applies_to: Vec::new(),
                nullable: None,
                max_length: None,
                precision: None,
                scale: None,
                srid: None,
                unicode: None,
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document =
        Resolver::resolve_edmx_document(edmx).expect("resolver should allow term base-term wiring");
    let errors =
        validate_document(&document).expect_err("validation should detect cyclic term base terms");

    assert!(errors.iter().any(|error| matches!(
        error,
        ValidationError::CyclicTermBaseTerm { term } if term == "A" || term == "B"
    )));
}

#[test]
fn validates_container_targets() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Thing".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Function(csdl::Function {
                name: "ListThings".to_owned(),
                is_bound: Some(false),
                is_composable: Some(false),
                entity_set_path: None,
                parameters: Vec::new(),
                return_type: Some(csdl::ReturnType {
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: None,
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    annotations: Vec::new(),
                }),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Action(csdl::Action {
                name: "Reset".to_owned(),
                is_bound: Some(false),
                entity_set_path: None,
                parameters: Vec::new(),
                return_type: None,
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityContainer(csdl::EntityContainer {
                name: "Container".to_owned(),
                extends: None,
                entity_sets: vec![csdl::EntitySet {
                    name: "Things".to_owned(),
                    entity_type: Some("Demo.Thing".to_owned()),
                    include_in_service_document: None,
                    navigation_property_bindings: vec![csdl::NavigationPropertyBinding {
                        path: "Nav".to_owned(),
                        target: "MissingSet".to_owned(),
                    }],
                    annotations: Vec::new(),
                }],
                singletons: Vec::new(),
                function_imports: vec![csdl::FunctionImport {
                    name: "ListThings".to_owned(),
                    entity_set: Some("MissingSet".to_owned()),
                    function: Some("Demo.ListThings".to_owned()),
                    include_in_service_document: None,
                    annotations: Vec::new(),
                }],
                action_imports: vec![csdl::ActionImport {
                    name: "Reset".to_owned(),
                    action: Some("Demo.Reset".to_owned()),
                    entity_set: Some("MissingSet".to_owned()),
                    include_in_service_document: None,
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx).expect("resolver should produce a model");
    let errors = validate_document(&document).expect_err("validation should detect bad targets");

    let unknown_targets = errors
        .iter()
        .filter(|error| matches!(error, ValidationError::UnknownContainerTarget { .. }))
        .count();

    assert_eq!(unknown_targets, 3);
}

#[test]
fn rejects_unresolved_function_and_action_import_targets() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Thing".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Function(csdl::Function {
                name: "ListThings".to_owned(),
                is_bound: Some(false),
                is_composable: Some(false),
                entity_set_path: None,
                parameters: Vec::new(),
                return_type: Some(csdl::ReturnType {
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: None,
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    annotations: Vec::new(),
                }),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Action(csdl::Action {
                name: "Reset".to_owned(),
                is_bound: Some(false),
                entity_set_path: None,
                parameters: Vec::new(),
                return_type: None,
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityContainer(csdl::EntityContainer {
                name: "Container".to_owned(),
                extends: None,
                entity_sets: vec![csdl::EntitySet {
                    name: "Things".to_owned(),
                    entity_type: Some("Demo.Thing".to_owned()),
                    include_in_service_document: None,
                    navigation_property_bindings: Vec::new(),
                    annotations: Vec::new(),
                }],
                singletons: Vec::new(),
                function_imports: vec![csdl::FunctionImport {
                    name: "ListThingsImport".to_owned(),
                    entity_set: Some("Things".to_owned()),
                    function: Some("Demo.MissingFunction".to_owned()),
                    include_in_service_document: None,
                    annotations: Vec::new(),
                }],
                action_imports: vec![csdl::ActionImport {
                    name: "ResetImport".to_owned(),
                    entity_set: Some("Things".to_owned()),
                    action: Some("Demo.MissingAction".to_owned()),
                    include_in_service_document: None,
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let err = Resolver::resolve_edmx_document(edmx)
        .expect_err("resolver should reject unresolved function import targets");
    match err {
        ResolveError::UnknownType(type_name) => {
            assert_eq!(type_name, "Demo.MissingFunction");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn rejects_unresolved_action_import_targets() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Thing".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Action(csdl::Action {
                name: "Reset".to_owned(),
                is_bound: Some(false),
                entity_set_path: None,
                parameters: Vec::new(),
                return_type: None,
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityContainer(csdl::EntityContainer {
                name: "Container".to_owned(),
                extends: None,
                entity_sets: vec![csdl::EntitySet {
                    name: "Things".to_owned(),
                    entity_type: Some("Demo.Thing".to_owned()),
                    include_in_service_document: None,
                    navigation_property_bindings: Vec::new(),
                    annotations: Vec::new(),
                }],
                singletons: Vec::new(),
                function_imports: Vec::new(),
                action_imports: vec![csdl::ActionImport {
                    name: "ResetImport".to_owned(),
                    entity_set: Some("Things".to_owned()),
                    action: Some("Demo.MissingAction".to_owned()),
                    include_in_service_document: None,
                    annotations: Vec::new(),
                }],
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let err = Resolver::resolve_edmx_document(edmx)
        .expect_err("resolver should reject unresolved action import targets");
    match err {
        ResolveError::UnknownType(type_name) => {
            assert_eq!(type_name, "Demo.MissingAction");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn validates_operation_binding_semantics() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "Thing".to_owned(),
                base_type: None,
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Function(csdl::Function {
                name: "BrokenBoundFunction".to_owned(),
                is_bound: Some(true),
                is_composable: Some(true),
                entity_set_path: Some("binding/Target".to_owned()),
                parameters: Vec::new(),
                return_type: Some(csdl::ReturnType {
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: None,
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    annotations: Vec::new(),
                }),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Action(csdl::Action {
                name: "UnboundWithPath".to_owned(),
                is_bound: Some(false),
                entity_set_path: Some("binding/Target".to_owned()),
                parameters: vec![csdl::Parameter {
                    name: "binding".to_owned(),
                    type_name: Some("Demo.Thing".to_owned()),
                    is_collection: false,
                    nullable: None,
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                return_type: None,
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::Action(csdl::Action {
                name: "WrongBindingPrefix".to_owned(),
                is_bound: Some(true),
                entity_set_path: Some("other/Target".to_owned()),
                parameters: vec![csdl::Parameter {
                    name: "binding".to_owned(),
                    type_name: Some("Demo.Thing".to_owned()),
                    is_collection: false,
                    nullable: None,
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                return_type: None,
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx).expect("resolver should map operations");
    let errors = validate_document(&document)
        .expect_err("validation should detect broken operation semantics");

    let missing_binding = errors
        .iter()
        .filter(|error| {
            matches!(
                error,
                ValidationError::BoundOperationMissingBindingParameter {
                    operation_kind,
                    operation,
                } if *operation_kind == "Function" && operation == "BrokenBoundFunction"
            )
        })
        .count();
    assert_eq!(missing_binding, 1);

    let invalid_paths = errors
        .iter()
        .filter(|error| {
            matches!(
                error,
                ValidationError::InvalidEntitySetPath {
                    operation,
                    ..
                } if operation == "UnboundWithPath" || operation == "WrongBindingPrefix"
            )
        })
        .count();
    assert_eq!(invalid_paths, 2);
}

#[test]
fn resolves_inherited_entity_keys() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "BaseEntity".to_owned(),
                base_type: None,
                abstract_: Some(true),
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "DerivedEntity".to_owned(),
                base_type: Some("Demo.BaseEntity".to_owned()),
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: None,
                properties: Vec::new(),
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx)
        .expect("resolver should inherit keys for derived entity types");
    let model = document.schemas.first().expect("resolved schema expected");
    let derived_entity = model
        .elements
        .iter()
        .find_map(|element| match element.as_ref() {
            csdl_edm::edm::SchemaElement::EntityType(entity) if entity.name == "DerivedEntity" => {
                Some(entity.clone())
            }
            _ => None,
        })
        .expect("DerivedEntity expected");

    let key_strings: Vec<String> = derived_entity
        .keys()
        .iter()
        .map(|key| csdl_edm::edm::key_path_to_string(key))
        .collect();
    assert_eq!(key_strings, vec!["ID".to_owned()]);
}

#[test]
fn rejects_derived_entity_redeclaring_base_keys() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "BaseEntity".to_owned(),
                base_type: None,
                abstract_: Some(true),
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "DerivedEntity".to_owned(),
                base_type: Some("Demo.BaseEntity".to_owned()),
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: Some(csdl::Key {
                    property_refs: vec![csdl::PropertyRef {
                        alias: None,
                        name: "ID".to_owned(),
                    }],
                }),
                properties: vec![csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let error = Resolver::resolve_edmx_document(edmx)
        .expect_err("resolver should reject derived entity key redeclarations");

    match error {
        ResolveError::UnsupportedCsdlFeature { feature, location } => {
            assert_eq!(feature, "Derived EntityType.Key");
            assert_eq!(location, "Demo.DerivedEntity");
        }
        other => panic!("unexpected resolve error: {other:?}"),
    }
}

#[test]
fn rejects_derived_entity_member_redeclaration() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "BaseEntity".to_owned(),
                base_type: None,
                abstract_: Some(true),
                open_type: None,
                has_stream: None,
                key: None,
                properties: vec![csdl::Property {
                    name: "Name".to_owned(),
                    type_name: Some("Edm.String".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::EntityType(csdl::EntityType {
                name: "DerivedEntity".to_owned(),
                base_type: Some("Demo.BaseEntity".to_owned()),
                abstract_: None,
                open_type: None,
                has_stream: None,
                key: None,
                properties: vec![csdl::Property {
                    name: "Name".to_owned(),
                    type_name: Some("Edm.String".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let error = Resolver::resolve_edmx_document(edmx)
        .expect_err("resolver should reject derived entity member redeclarations");

    match error {
        ResolveError::UnsupportedCsdlFeature { feature, location } => {
            assert_eq!(feature, "Derived EntityType member redeclaration");
            assert_eq!(location, "Demo.DerivedEntity");
        }
        other => panic!("unexpected resolve error: {other:?}"),
    }
}

#[test]
fn rejects_derived_complex_member_redeclaration() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![
            csdl::SchemaElement::ComplexType(csdl::ComplexType {
                name: "BaseComplex".to_owned(),
                base_type: None,
                abstract_: Some(true),
                open_type: None,
                properties: vec![csdl::Property {
                    name: "Code".to_owned(),
                    type_name: Some("Edm.String".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
            csdl::SchemaElement::ComplexType(csdl::ComplexType {
                name: "DerivedComplex".to_owned(),
                base_type: Some("Demo.BaseComplex".to_owned()),
                abstract_: None,
                open_type: None,
                properties: vec![csdl::Property {
                    name: "Code".to_owned(),
                    type_name: Some("Edm.String".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                }],
                navigation_properties: Vec::new(),
                annotations: Vec::new(),
            }),
        ],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let error = Resolver::resolve_edmx_document(edmx)
        .expect_err("resolver should reject derived complex member redeclarations");

    match error {
        ResolveError::UnsupportedCsdlFeature { feature, location } => {
            assert_eq!(feature, "Derived ComplexType member redeclaration");
            assert_eq!(location, "Demo.DerivedComplex");
        }
        other => panic!("unexpected resolve error: {other:?}"),
    }
}

#[test]
fn resolves_collection_valued_structural_properties() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![csdl::SchemaElement::EntityType(csdl::EntityType {
            name: "Bucket".to_owned(),
            base_type: None,
            abstract_: None,
            open_type: None,
            has_stream: None,
            key: Some(csdl::Key {
                property_refs: vec![csdl::PropertyRef {
                    alias: None,
                    name: "ID".to_owned(),
                }],
            }),
            properties: vec![
                csdl::Property {
                    name: "ID".to_owned(),
                    type_name: Some("Edm.Int32".to_owned()),
                    is_collection: false,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                },
                csdl::Property {
                    name: "Tags".to_owned(),
                    type_name: Some("Edm.String".to_owned()),
                    is_collection: true,
                    nullable: Some(false),
                    max_length: None,
                    precision: None,
                    scale: None,
                    srid: None,
                    unicode: None,
                    default_value: None,
                    annotations: Vec::new(),
                },
            ],
            navigation_properties: Vec::new(),
            annotations: Vec::new(),
        })],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let document = Resolver::resolve_edmx_document(edmx)
        .expect("resolver should support collection-valued structural properties");
    let model = document.schemas.first().expect("resolved schema expected");
    let bucket = model
        .elements
        .iter()
        .find_map(|element| match element.as_ref() {
            csdl_edm::edm::SchemaElement::EntityType(entity) if entity.name == "Bucket" => {
                Some(entity.clone())
            }
            _ => None,
        })
        .expect("Bucket entity expected");

    let tags = bucket
        .properties()
        .iter()
        .find(|property| property.name == "Tags")
        .expect("Tags property expected");
    assert!(tags.is_collection);
}

#[test]
fn rejects_function_without_return_type() {
    let schema = csdl::Schema {
        namespace: "Demo".to_owned(),
        alias: None,
        elements: vec![csdl::SchemaElement::Function(csdl::Function {
            name: "BrokenFunction".to_owned(),
            is_bound: Some(false),
            is_composable: Some(false),
            entity_set_path: None,
            parameters: Vec::new(),
            return_type: None,
            annotations: Vec::new(),
        })],
        annotations: Vec::new(),
    };

    let edmx = csdl::Edmx {
        version: Some("4.01".to_owned()),
        references: Vec::new(),
        schemas: vec![schema],
    };

    let err = Resolver::resolve_edmx_document(edmx)
        .expect_err("resolver should reject function without ReturnType");
    match err {
        ResolveError::UnsupportedCsdlFeature { feature, location } => {
            assert_eq!(feature, "Function without ReturnType");
            assert_eq!(location, "Demo.BrokenFunction");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

fn assert_expected_resolve_error(
    fixture_name: &str,
    expected: ExpectedOutcome,
    error: ResolveError,
) {
    match expected {
        ExpectedOutcome::ResolveUnknownType(type_name) => match error {
            ResolveError::UnknownType(actual_type_name) => assert_eq!(
                actual_type_name, type_name,
                "{} returned unexpected unknown type",
                fixture_name
            ),
            other => panic!(
                "{} expected unknown type {:?}, got {:?}",
                fixture_name, type_name, other
            ),
        },
        ExpectedOutcome::ResolveMissingType {
            element_kind,
            element_name,
        } => match error {
            ResolveError::MissingTypeName {
                element_kind: actual_kind,
                element_name: actual_name,
            } => {
                assert_eq!(
                    actual_kind, element_kind,
                    "{} wrong missing-type kind",
                    fixture_name
                );
                assert_eq!(
                    actual_name, element_name,
                    "{} wrong missing-type element",
                    fixture_name
                );
            }
            other => panic!(
                "{} expected missing type {:?}/{:?}, got {:?}",
                fixture_name, element_kind, element_name, other
            ),
        },
        other => panic!(
            "{} expected {:?}, but resolve failed with {:?}",
            fixture_name, other, error
        ),
    }
}

fn assert_expected_validation(
    fixture_name: &str,
    expected: ExpectedOutcome,
    errors: &[ValidationError],
) {
    match expected {
        ExpectedOutcome::ValidateInvalidNavigation(reason) => {
            assert!(
                errors.iter().any(|error| matches!(error,
                ValidationError::InvalidNavigation { reason: actual, .. } if *actual == reason)),
                "{fixture_name}: expected {reason}, got {errors:?}"
            );
        }
        ExpectedOutcome::ValidateUnknownContainerTarget {
            source_kind,
            source,
            target,
        } => {
            let found = errors.iter().any(|error| {
                matches!(
                    error,
                    ValidationError::UnknownContainerTarget {
                        source_kind: actual_source_kind,
                        source: actual_source,
                        target: actual_target,
                        ..
                    } if *actual_source_kind == source_kind
                        && actual_source == source
                        && actual_target == target
                )
            });
            assert!(
                found,
                "{} expected unknown container target {:?}/{:?}->{:?}, got {:?}",
                fixture_name, source_kind, source, target, errors
            );
        }
        other => panic!(
            "{} expected {:?}, but validation failed with {:?}",
            fixture_name, other, errors
        ),
    }
}

fn fixture_paths() -> Vec<PathBuf> {
    let mut paths = fs::read_dir(inputs_dir())
        .expect("list data/inputs")
        .map(|entry| entry.expect("valid dir entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(|name| name.ends_with(".csdl.xml") || name.ends_with(".csdl.json"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn fixture_path(file_name: &str) -> PathBuf {
    let path = inputs_dir().join(file_name);
    assert!(path.is_file(), "fixture not found: {}", path.display());
    path
}

fn inputs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("inputs")
}
