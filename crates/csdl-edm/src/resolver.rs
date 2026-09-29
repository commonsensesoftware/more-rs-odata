//! ---------------------------------------------------------------------------
//! Resolver
//! ---------------------------------------------------------------------------

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, Weak};

use crate::{csdl, edm::*};

#[derive(Debug)]
pub enum ResolveError {
    UnknownType(String),
    UnknownEntity(String),
    DuplicateName(String),
    NoSchemaInDocument,
    MissingTypeName {
        element_kind: &'static str,
        element_name: String,
    },
    UnknownPropertyPath {
        entity: String,
        key_path: String,
    },
    UnsupportedCsdlFeature {
        feature: &'static str,
        location: String,
    },
}

pub struct Resolver;

type ResolvedMembers = (Vec<Arc<Property>>, Vec<Arc<NavigationProperty>>);

impl Resolver {
    pub fn resolve_document(document: csdl::Edmx) -> Result<DocumentModel, ResolveError> {
        let document_aliases = collect_document_aliases(&document.references);
        let references = document
            .references
            .into_iter()
            .map(|r| Reference {
                uri: r.uri,
                includes: r
                    .includes
                    .into_iter()
                    .map(|i| Include {
                        namespace: i.namespace,
                        alias: i.alias,
                    })
                    .collect(),
                include_annotations: r
                    .include_annotations
                    .into_iter()
                    .map(|a| IncludeAnnotations {
                        term_namespace: a.term_namespace,
                        target_namespace: a.target_namespace,
                        qualifier: a.qualifier,
                    })
                    .collect(),
            })
            .collect();

        let schemas = document
            .schemas
            .into_iter()
            .map(|schema| {
                let mut aliases = document_aliases.clone();
                if let Some(alias) = &schema.alias {
                    aliases.insert(alias.clone(), schema.namespace.clone());
                }
                Self::resolve_with_aliases(schema, &aliases)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(Arc::new)
            .collect::<Vec<_>>();

        if schemas.is_empty() {
            return Err(ResolveError::NoSchemaInDocument);
        }

        Ok(DocumentModel {
            version: document.version.unwrap_or_else(|| "4.01".to_owned()),
            references,
            schemas,
        })
    }

    pub fn resolve_edmx_document(document: csdl::Edmx) -> Result<DocumentModel, ResolveError> {
        Self::resolve_document(document)
    }

    pub fn resolve(schema: csdl::Schema) -> Result<Model, ResolveError> {
        let mut aliases = HashMap::new();
        if let Some(alias) = &schema.alias {
            aliases.insert(alias.clone(), schema.namespace.clone());
        }
        Self::resolve_with_aliases(schema, &aliases)
    }

    fn resolve_with_aliases(
        schema: csdl::Schema,
        aliases: &HashMap<String, String>,
    ) -> Result<Model, ResolveError> {
        let ns = schema.namespace.clone();

        let entity_defs = schema
            .elements
            .iter()
            .filter_map(|el| match el {
                csdl::SchemaElement::EntityType(e) => Some((e.name.clone(), e)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();

        let mut entity_key_cache: HashMap<String, Vec<String>> = HashMap::new();
        for name in entity_defs.keys() {
            let _ = resolve_effective_entity_keys(
                name,
                &entity_defs,
                &ns,
                aliases,
                &mut entity_key_cache,
                &mut Vec::new(),
            )?;
        }

        let mut entities: HashMap<String, Arc<EntityType>> = HashMap::new();
        let mut complexes: HashMap<String, Arc<ComplexType>> = HashMap::new();
        let mut enums: HashMap<String, Arc<EnumType>> = HashMap::new();
        let mut type_definitions: HashMap<String, Arc<TypeDefinition>> = HashMap::new();
        let mut terms: HashMap<String, Arc<Term>> = HashMap::new();
        let function_defs = schema
            .elements
            .iter()
            .filter_map(|el| match el {
                csdl::SchemaElement::Function(function) => Some((function.name.clone(), function)),
                _ => None,
            })
            .collect::<HashMap<String, &csdl::Function>>();
        let action_defs = schema
            .elements
            .iter()
            .filter_map(|el| match el {
                csdl::SchemaElement::Action(action) => Some((action.name.clone(), action)),
                _ => None,
            })
            .collect::<HashMap<String, &csdl::Action>>();

        for el in &schema.elements {
            match el {
                csdl::SchemaElement::EntityType(e) => {
                    let arc = Arc::new(EntityType {
                        name: e.name.clone(),
                        is_abstract: e.abstract_.unwrap_or(false),
                        is_open: e.open_type.unwrap_or(false),
                        has_stream: e.has_stream.unwrap_or(false),
                        base_type: OnceLock::new(),
                        keys: std::sync::OnceLock::new(),
                        properties: std::sync::OnceLock::new(),
                        navigation_properties: std::sync::OnceLock::new(),
                    });
                    insert_unique(&mut entities, &e.name, arc, &ns)?;
                }
                csdl::SchemaElement::ComplexType(c) => {
                    let arc = Arc::new(ComplexType {
                        name: c.name.clone(),
                        is_abstract: c.abstract_.unwrap_or(false),
                        is_open: c.open_type.unwrap_or(false),
                        base_type: OnceLock::new(),
                        properties: std::sync::OnceLock::new(),
                        navigation_properties: std::sync::OnceLock::new(),
                    });
                    insert_unique(&mut complexes, &c.name, arc, &ns)?;
                }
                csdl::SchemaElement::EnumType(en) => {
                    let members = en
                        .members
                        .iter()
                        .map(|m| {
                            Arc::new(EnumMember {
                                name: m.name.clone(),
                                value: m.value,
                            })
                        })
                        .collect();
                    let arc = Arc::new(EnumType {
                        name: en.name.clone(),
                        members,
                    });
                    insert_unique(&mut enums, &en.name, arc, &ns)?;
                }
                csdl::SchemaElement::TypeDefinition(td) => {
                    let primitive = resolve_primitive(&td.underlying_type)
                        .ok_or_else(|| ResolveError::UnknownType(td.underlying_type.clone()))?;
                    let arc = Arc::new(TypeDefinition {
                        name: td.name.clone(),
                        underlying_type: primitive,
                    });
                    insert_unique(&mut type_definitions, &td.name, arc, &ns)?;
                }
                csdl::SchemaElement::Term(t) => {
                    let arc = Arc::new(Term {
                        name: t.name.clone(),
                        is_collection: t.is_collection,
                        ty: std::sync::OnceLock::new(),
                        base_term: std::sync::OnceLock::new(),
                    });
                    insert_unique(&mut terms, &t.name, arc, &ns)?;
                }
                csdl::SchemaElement::Function(_) | csdl::SchemaElement::Action(_) => {}
                csdl::SchemaElement::EntityContainer(_) => {}
            }
        }

        let complex_defs = schema
            .elements
            .iter()
            .filter_map(|el| match el {
                csdl::SchemaElement::ComplexType(c) => Some((c.name.clone(), c)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();

        let mut container_defs: HashMap<String, &csdl::EntityContainer> = HashMap::new();
        for el in &schema.elements {
            if let csdl::SchemaElement::EntityContainer(container) = el {
                if container_defs.contains_key(&container.name) {
                    return Err(ResolveError::DuplicateName(format!(
                        "{}.{}",
                        ns, container.name
                    )));
                }
                container_defs.insert(container.name.clone(), container);
            }
        }

        // Weak base links preserve identity without retaining invalid cyclic inputs.
        for element in &schema.elements {
            match element {
                csdl::SchemaElement::EntityType(e) => {
                    let base = e
                        .base_type
                        .as_ref()
                        .map(|name| {
                            let short = unqualify_to_local(name, &ns, aliases)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))?;
                            entities
                                .get(short)
                                .map(Arc::downgrade)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))
                        })
                        .transpose()?;
                    let _ = entities[&e.name].base_type.set(base);
                }
                csdl::SchemaElement::ComplexType(c) => {
                    let base = c
                        .base_type
                        .as_ref()
                        .map(|name| {
                            let short = unqualify_to_local(name, &ns, aliases)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))?;
                            complexes
                                .get(short)
                                .map(Arc::downgrade)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))
                        })
                        .transpose()?;
                    let _ = complexes[&c.name].base_type.set(base);
                }
                _ => {}
            }
        }

        let mut entity_member_cache: HashMap<String, ResolvedMembers> = HashMap::new();
        let mut complex_member_cache: HashMap<String, ResolvedMembers> = HashMap::new();

        for el in &schema.elements {
            match el {
                csdl::SchemaElement::EntityType(e) => {
                    let arc = entities.get(&e.name).expect("inserted above");
                    let (props, navs) = resolve_entity_members(
                        &e.name,
                        &entity_defs,
                        &complexes,
                        &enums,
                        &type_definitions,
                        &entities,
                        &ns,
                        aliases,
                        &mut entity_member_cache,
                        &mut Vec::new(),
                    )?;
                    validate_entity_key_paths(
                        e,
                        &props,
                        &complex_defs,
                        &complexes,
                        &enums,
                        &type_definitions,
                        &entities,
                        &ns,
                        aliases,
                        &mut complex_member_cache,
                    )?;
                    let _ = arc.properties.set(props);
                    let _ = arc.navigation_properties.set(navs);
                }
                csdl::SchemaElement::ComplexType(c) => {
                    let arc = complexes.get(&c.name).expect("inserted above");
                    let (props, navs) = resolve_complex_members(
                        &c.name,
                        &complex_defs,
                        &complexes,
                        &enums,
                        &type_definitions,
                        &entities,
                        &ns,
                        aliases,
                        &mut complex_member_cache,
                        &mut Vec::new(),
                    )?;
                    let _ = arc.properties.set(props);
                    let _ = arc.navigation_properties.set(navs);
                }
                csdl::SchemaElement::Term(t) => {
                    let arc = terms.get(&t.name).expect("inserted above");
                    let ty_name = t.type_name.as_deref().unwrap_or("Edm.String");
                    let ty = resolve_term_type(
                        ty_name,
                        &entities,
                        &complexes,
                        &enums,
                        &type_definitions,
                        &ns,
                        aliases,
                    )?;
                    let _ = arc.ty.set(ty);

                    let base = match &t.base_term {
                        Some(name) => {
                            let short = unqualify_to_local(name, &ns, aliases)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))?;
                            let base = terms
                                .get(short)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))?;
                            Some(Arc::downgrade(base))
                        }
                        None => None,
                    };
                    let _ = arc.base_term.set(base);
                }
                csdl::SchemaElement::EnumType(_)
                | csdl::SchemaElement::TypeDefinition(_)
                | csdl::SchemaElement::Function(_)
                | csdl::SchemaElement::Action(_)
                | csdl::SchemaElement::EntityContainer(_) => {}
            }
        }

        // Late pass: every entity/complex type now has its properties, so the
        // effective key strings can be resolved into typed [`KeyPathSegment`]
        // paths. The key paths were already validated to resolve above.
        for (name, arc) in &entities {
            let typed_keys = entity_key_cache
                .get(name)
                .map(|keys| keys.iter().map(|key| resolve_key_path(arc, key)).collect())
                .unwrap_or_default();
            let _ = arc.keys.set(typed_keys);
        }

        // Late pass: resolve navigation `Partner` paths now that every entity's
        // navigation properties exist. A partner resolves against the
        // navigation's target entity type and ends at a navigation property.
        // Inherited navigation properties share the same `Arc`, so resolving
        // each owning type's own navigations covers derived types too.
        for el in &schema.elements {
            let (csdl_navs, resolved_navs): (
                &[csdl::NavigationProperty],
                &[Arc<NavigationProperty>],
            ) = match el {
                csdl::SchemaElement::EntityType(e) => match entities.get(&e.name) {
                    Some(arc) => (&e.navigation_properties, arc.navigation_properties()),
                    None => continue,
                },
                csdl::SchemaElement::ComplexType(c) => match complexes.get(&c.name) {
                    Some(arc) => (&c.navigation_properties, arc.navigation_properties()),
                    None => continue,
                },
                _ => continue,
            };

            for csdl_nav in csdl_navs {
                let Some(partner) = csdl_nav.partner.as_deref() else {
                    continue;
                };
                let Some(resolved) = resolved_navs.iter().find(|n| n.name == csdl_nav.name) else {
                    continue;
                };
                let path = match resolved.target.upgrade() {
                    Some(target) => resolve_binding_path(&target, partner),
                    None => Arc::from([BindingPathSegment::Unresolved(partner.to_owned())]),
                };
                let _ = resolved.partner.set(path);
            }
        }

        let mut elements: Vec<Arc<SchemaElement>> = Vec::new();
        for el in &schema.elements {
            let wrapped = match el {
                csdl::SchemaElement::EntityType(e) => SchemaElement::EntityType(
                    entities.get(&e.name).cloned().expect("entity exists"),
                ),
                csdl::SchemaElement::ComplexType(c) => SchemaElement::ComplexType(
                    complexes.get(&c.name).cloned().expect("complex exists"),
                ),
                csdl::SchemaElement::EnumType(en) => {
                    SchemaElement::EnumType(enums.get(&en.name).cloned().expect("enum exists"))
                }
                csdl::SchemaElement::TypeDefinition(td) => SchemaElement::TypeDefinition(
                    type_definitions
                        .get(&td.name)
                        .cloned()
                        .expect("type definition exists"),
                ),
                csdl::SchemaElement::Term(t) => {
                    SchemaElement::Term(terms.get(&t.name).cloned().expect("term exists"))
                }
                csdl::SchemaElement::Function(function) => {
                    SchemaElement::Function(Arc::new(resolve_function(
                        function,
                        &entities,
                        &complexes,
                        &enums,
                        &type_definitions,
                        &ns,
                        aliases,
                    )?))
                }
                csdl::SchemaElement::Action(action) => {
                    SchemaElement::Action(Arc::new(resolve_action(
                        action,
                        &entities,
                        &complexes,
                        &enums,
                        &type_definitions,
                        &ns,
                        aliases,
                    )?))
                }
                csdl::SchemaElement::EntityContainer(_) => continue,
            };
            elements.push(Arc::new(wrapped));
        }

        let entity_container = schema
            .elements
            .iter()
            .find_map(|el| match el {
                csdl::SchemaElement::EntityContainer(container) => Some(container),
                _ => None,
            })
            .map(|c| {
                resolve_entity_container(
                    c,
                    &container_defs,
                    &entities,
                    &function_defs,
                    &action_defs,
                    &ns,
                    aliases,
                    &mut Vec::new(),
                )
            })
            .transpose()?;

        Ok(Model {
            namespace: schema.namespace,
            alias: schema.alias,
            elements,
            entity_container,
        })
    }
}

fn collect_document_aliases(references: &[csdl::Reference]) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    for reference in references {
        for include in &reference.includes {
            if let Some(alias) = &include.alias {
                aliases.insert(alias.clone(), include.namespace.clone());
            }
        }
    }
    aliases
}

fn unqualify_to_local<'a>(
    name: &'a str,
    namespace: &str,
    aliases: &HashMap<String, String>,
) -> Option<&'a str> {
    if let Some((qualifier, short)) = name.split_once('.') {
        if qualifier == namespace {
            return Some(short);
        }
        if aliases.get(qualifier).map(String::as_str) == Some(namespace) {
            return Some(short);
        }
        return None;
    }
    Some(name)
}

fn insert_unique<T>(
    map: &mut HashMap<String, T>,
    name: &str,
    value: T,
    ns: &str,
) -> Result<(), ResolveError> {
    if map.contains_key(name) {
        return Err(ResolveError::DuplicateName(format!("{}.{}", ns, name)));
    }
    map.insert(name.to_string(), value);
    Ok(())
}

fn resolve_effective_entity_keys(
    name: &str,
    entity_defs: &HashMap<String, &csdl::EntityType>,
    ns: &str,
    aliases: &HashMap<String, String>,
    cache: &mut HashMap<String, Vec<String>>,
    visiting: &mut Vec<String>,
) -> Result<Vec<String>, ResolveError> {
    if let Some(keys) = cache.get(name) {
        return Ok(keys.clone());
    }

    if visiting.iter().any(|current| current == name) {
        return Err(ResolveError::UnsupportedCsdlFeature {
            feature: "Cyclic EntityType.BaseType",
            location: format!("{}.{}", ns, name),
        });
    }
    visiting.push(name.to_owned());

    let entity = entity_defs
        .get(name)
        .ok_or_else(|| ResolveError::UnknownType(name.to_owned()))?;

    let own_keys = entity
        .key
        .as_ref()
        .map(|k| {
            k.property_refs
                .iter()
                .map(|r| r.name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let effective_keys = if let Some(base_type) = &entity.base_type {
        let base_name = unqualify_to_local(base_type, ns, aliases)
            .ok_or_else(|| ResolveError::UnknownType(base_type.clone()))?;
        let base_keys =
            resolve_effective_entity_keys(base_name, entity_defs, ns, aliases, cache, visiting)?;

        if !base_keys.is_empty() && entity.key.is_some() {
            return Err(ResolveError::UnsupportedCsdlFeature {
                feature: "Derived EntityType.Key",
                location: format!("{}.{}", ns, entity.name),
            });
        }

        if own_keys.is_empty() {
            base_keys
        } else {
            own_keys
        }
    } else {
        own_keys
    };

    visiting.pop();
    cache.insert(name.to_owned(), effective_keys.clone());
    Ok(effective_keys)
}

fn resolve_type(
    type_name: &str,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<ResolvedType, ResolveError> {
    if let Some(primitive) = resolve_primitive(type_name) {
        return Ok(ResolvedType::Primitive(primitive));
    }
    let Some(short) = unqualify_to_local(type_name, ns, aliases) else {
        return Err(ResolveError::UnknownType(type_name.to_string()));
    };
    if let Some(c) = complexes.get(short) {
        return Ok(ResolvedType::Complex(c.clone()));
    }
    if let Some(e) = enums.get(short) {
        return Ok(ResolvedType::Enum(e.clone()));
    }
    if let Some(td) = type_definitions.get(short) {
        return Ok(ResolvedType::TypeDefinition(td.clone()));
    }
    Err(ResolveError::UnknownType(type_name.to_string()))
}

fn resolve_properties(
    props: &[csdl::Property],
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<Vec<Arc<Property>>, ResolveError> {
    props
        .iter()
        .map(|p| {
            let type_name =
                p.type_name
                    .as_deref()
                    .ok_or_else(|| ResolveError::MissingTypeName {
                        element_kind: "Property",
                        element_name: p.name.clone(),
                    })?;

            let ty = resolve_type(type_name, complexes, enums, type_definitions, ns, aliases)?;
            Ok(Arc::new(Property {
                name: p.name.clone(),
                ty,
                is_collection: p.is_collection,
            }))
        })
        .collect()
}

fn entity_is_strict_subtype(derived: &Arc<EntityType>, base: &Arc<EntityType>) -> bool {
    let mut current = derived
        .base_type
        .get()
        .and_then(Option::as_ref)
        .and_then(Weak::upgrade);
    let mut seen = HashSet::new();
    while let Some(ty) = current {
        if !seen.insert(Arc::as_ptr(&ty)) {
            return false;
        }
        if Arc::ptr_eq(&ty, base) {
            return true;
        }
        current = ty
            .base_type
            .get()
            .and_then(Option::as_ref)
            .and_then(Weak::upgrade);
    }
    false
}

fn complex_is_strict_subtype(derived: &Arc<ComplexType>, base: &Arc<ComplexType>) -> bool {
    let mut current = derived
        .base_type
        .get()
        .and_then(Option::as_ref)
        .and_then(Weak::upgrade);
    let mut seen = HashSet::new();
    while let Some(ty) = current {
        if !seen.insert(Arc::as_ptr(&ty)) {
            return false;
        }
        if Arc::ptr_eq(&ty, base) {
            return true;
        }
        current = ty
            .base_type
            .get()
            .and_then(Option::as_ref)
            .and_then(Weak::upgrade);
    }
    false
}

fn reject_inherited_member_redeclarations(
    kind: &'static str,
    type_name: &str,
    base_props: &[Arc<Property>],
    base_navs: &[Arc<NavigationProperty>],
    own_props: &[Arc<Property>],
    own_navs: &[Arc<NavigationProperty>],
    ns: &str,
) -> Result<(), ResolveError> {
    let invalid_property = own_props.iter().any(|own| {
        if base_navs.iter().any(|base| base.name == own.name) {
            return true;
        }
        base_props
            .iter()
            .find(|base| base.name == own.name)
            .is_some_and(|base| {
                own.is_collection != base.is_collection
                    || !matches!(
                        (&own.ty, &base.ty),
                        (ResolvedType::Complex(derived), ResolvedType::Complex(base))
                            if complex_is_strict_subtype(derived, base)
                    )
            })
    });
    let invalid_navigation = own_navs.iter().any(|own| {
        if base_props.iter().any(|base| base.name == own.name) {
            return true;
        }
        base_navs
            .iter()
            .find(|base| base.name == own.name)
            .is_some_and(|base| {
                own.is_collection != base.is_collection
                    || match (own.target.upgrade(), base.target.upgrade()) {
                        (Some(derived), Some(base)) => !entity_is_strict_subtype(&derived, &base),
                        _ => true,
                    }
            })
    });
    if invalid_property || invalid_navigation {
        return Err(ResolveError::UnsupportedCsdlFeature {
            feature: match kind {
                "EntityType" => "Derived EntityType member redeclaration",
                "ComplexType" => "Derived ComplexType member redeclaration",
                _ => "Derived member redeclaration",
            },
            location: format!("{}.{}", ns, type_name),
        });
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn resolve_entity_members(
    name: &str,
    entity_defs: &HashMap<String, &csdl::EntityType>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    entities: &HashMap<String, Arc<EntityType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
    cache: &mut HashMap<String, ResolvedMembers>,
    visiting: &mut Vec<String>,
) -> Result<ResolvedMembers, ResolveError> {
    if let Some((props, navs)) = cache.get(name) {
        return Ok((props.clone(), navs.clone()));
    }

    if visiting.iter().any(|current| current == name) {
        return Err(ResolveError::UnsupportedCsdlFeature {
            feature: "Cyclic EntityType.BaseType",
            location: format!("{}.{}", ns, name),
        });
    }
    visiting.push(name.to_owned());

    let entity = entity_defs
        .get(name)
        .ok_or_else(|| ResolveError::UnknownType(name.to_owned()))?;

    let mut props = Vec::new();
    let mut navs = Vec::new();

    if let Some(base_type) = &entity.base_type {
        let base_name = unqualify_to_local(base_type, ns, aliases)
            .ok_or_else(|| ResolveError::UnknownType(base_type.clone()))?;
        let (base_props, base_navs) = resolve_entity_members(
            base_name,
            entity_defs,
            complexes,
            enums,
            type_definitions,
            entities,
            ns,
            aliases,
            cache,
            visiting,
        )?;
        props.extend(base_props);
        navs.extend(base_navs);
    }

    let own_props = resolve_properties(
        &entity.properties,
        complexes,
        enums,
        type_definitions,
        ns,
        aliases,
    )?;
    let own_navs = resolve_navs(&entity.navigation_properties, entities, ns, aliases)?;

    reject_inherited_member_redeclarations(
        "EntityType",
        &entity.name,
        &props,
        &navs,
        &own_props,
        &own_navs,
        ns,
    )?;

    props.retain(|base| !own_props.iter().any(|own| own.name == base.name));
    props.extend(own_props);
    navs.retain(|base| !own_navs.iter().any(|own| own.name == base.name));
    navs.extend(own_navs);

    visiting.pop();
    cache.insert(name.to_owned(), (props.clone(), navs.clone()));
    Ok((props, navs))
}

#[allow(clippy::too_many_arguments)]
fn resolve_complex_members(
    name: &str,
    complex_defs: &HashMap<String, &csdl::ComplexType>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    entities: &HashMap<String, Arc<EntityType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
    cache: &mut HashMap<String, ResolvedMembers>,
    visiting: &mut Vec<String>,
) -> Result<ResolvedMembers, ResolveError> {
    if let Some((props, navs)) = cache.get(name) {
        return Ok((props.clone(), navs.clone()));
    }

    if visiting.iter().any(|current| current == name) {
        return Err(ResolveError::UnsupportedCsdlFeature {
            feature: "Cyclic ComplexType.BaseType",
            location: format!("{}.{}", ns, name),
        });
    }
    visiting.push(name.to_owned());

    let complex = complex_defs
        .get(name)
        .ok_or_else(|| ResolveError::UnknownType(name.to_owned()))?;

    let mut props = Vec::new();
    let mut navs = Vec::new();

    if let Some(base_type) = &complex.base_type {
        let base_name = unqualify_to_local(base_type, ns, aliases)
            .ok_or_else(|| ResolveError::UnknownType(base_type.clone()))?;
        let (base_props, base_navs) = resolve_complex_members(
            base_name,
            complex_defs,
            complexes,
            enums,
            type_definitions,
            entities,
            ns,
            aliases,
            cache,
            visiting,
        )?;
        props.extend(base_props);
        navs.extend(base_navs);
    }

    let own_props = resolve_properties(
        &complex.properties,
        complexes,
        enums,
        type_definitions,
        ns,
        aliases,
    )?;
    let own_navs = resolve_navs(&complex.navigation_properties, entities, ns, aliases)?;

    reject_inherited_member_redeclarations(
        "ComplexType",
        &complex.name,
        &props,
        &navs,
        &own_props,
        &own_navs,
        ns,
    )?;

    props.retain(|base| !own_props.iter().any(|own| own.name == base.name));
    props.extend(own_props);
    navs.retain(|base| !own_navs.iter().any(|own| own.name == base.name));
    navs.extend(own_navs);

    visiting.pop();
    cache.insert(name.to_owned(), (props.clone(), navs.clone()));
    Ok((props, navs))
}

#[allow(clippy::too_many_arguments)]
fn validate_entity_key_paths(
    entity: &csdl::EntityType,
    entity_properties: &[Arc<Property>],
    complex_defs: &HashMap<String, &csdl::ComplexType>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    entities: &HashMap<String, Arc<EntityType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
    complex_member_cache: &mut HashMap<String, ResolvedMembers>,
) -> Result<(), ResolveError> {
    let Some(key) = &entity.key else {
        return Ok(());
    };

    for key_ref in &key.property_refs {
        validate_property_path_from_entity(
            &entity.name,
            &key_ref.name,
            entity_properties,
            complex_defs,
            complexes,
            enums,
            type_definitions,
            entities,
            ns,
            aliases,
            complex_member_cache,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_property_path_from_entity(
    entity_name: &str,
    key_path: &str,
    entity_properties: &[Arc<Property>],
    complex_defs: &HashMap<String, &csdl::ComplexType>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    entities: &HashMap<String, Arc<EntityType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
    complex_member_cache: &mut HashMap<String, ResolvedMembers>,
) -> Result<(), ResolveError> {
    let mut segments = key_path.split('/').filter(|segment| !segment.is_empty());

    let Some(first_segment) = segments.next() else {
        return Err(ResolveError::UnknownPropertyPath {
            entity: entity_name.to_owned(),
            key_path: key_path.to_owned(),
        });
    };

    let first_property = entity_properties
        .iter()
        .find(|property| property.name == first_segment)
        .ok_or_else(|| ResolveError::UnknownPropertyPath {
            entity: entity_name.to_owned(),
            key_path: key_path.to_owned(),
        })?;

    let mut current_type = first_property.ty.clone();

    for segment in segments {
        let ResolvedType::Complex(complex) = &current_type else {
            return Err(ResolveError::UnknownPropertyPath {
                entity: entity_name.to_owned(),
                key_path: key_path.to_owned(),
            });
        };

        let (complex_properties, _) = resolve_complex_members(
            &complex.name,
            complex_defs,
            complexes,
            enums,
            type_definitions,
            entities,
            ns,
            aliases,
            complex_member_cache,
            &mut Vec::new(),
        )?;

        let next_property = complex_properties
            .iter()
            .find(|property| property.name == segment)
            .ok_or_else(|| ResolveError::UnknownPropertyPath {
                entity: entity_name.to_owned(),
                key_path: key_path.to_owned(),
            })?;

        current_type = next_property.ty.clone();
    }

    Ok(())
}

fn resolve_term_type(
    type_name: &str,
    entities: &HashMap<String, Arc<EntityType>>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<TermType, ResolveError> {
    if let Some(primitive) = resolve_primitive(type_name) {
        return Ok(TermType::Primitive(primitive));
    }

    let Some(short) = unqualify_to_local(type_name, ns, aliases) else {
        return Err(ResolveError::UnknownType(type_name.to_string()));
    };
    if let Some(td) = type_definitions.get(short) {
        return Ok(TermType::TypeDefinition(td.clone()));
    }
    if let Some(e) = enums.get(short) {
        return Ok(TermType::Enum(e.clone()));
    }
    if let Some(c) = complexes.get(short) {
        return Ok(TermType::Complex(c.clone()));
    }
    if let Some(e) = entities.get(short) {
        return Ok(TermType::Entity(e.clone()));
    }
    Err(ResolveError::UnknownType(type_name.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn resolve_function(
    function: &csdl::Function,
    entities: &HashMap<String, Arc<EntityType>>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<Function, ResolveError> {
    if function.return_type.is_none() {
        return Err(ResolveError::UnsupportedCsdlFeature {
            feature: "Function without ReturnType",
            location: format!("{}.{}", ns, function.name),
        });
    }

    let parameters = resolve_operation_parameters(
        &function.parameters,
        entities,
        complexes,
        enums,
        type_definitions,
        ns,
        aliases,
        "Function",
        &function.name,
    )?;

    Ok(Function {
        name: function.name.clone(),
        is_bound: function.is_bound.unwrap_or(false),
        is_composable: function.is_composable.unwrap_or(false),
        entity_set_path: function
            .entity_set_path
            .as_deref()
            .map(|path| resolve_entity_set_path(&parameters, path)),
        return_type: resolve_operation_return_type(
            function.return_type.as_ref(),
            entities,
            complexes,
            enums,
            type_definitions,
            ns,
            aliases,
            "Function",
            &function.name,
        )?,
        parameters,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_action(
    action: &csdl::Action,
    entities: &HashMap<String, Arc<EntityType>>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<Action, ResolveError> {
    let parameters = resolve_operation_parameters(
        &action.parameters,
        entities,
        complexes,
        enums,
        type_definitions,
        ns,
        aliases,
        "Action",
        &action.name,
    )?;

    Ok(Action {
        name: action.name.clone(),
        is_bound: action.is_bound.unwrap_or(false),
        entity_set_path: action
            .entity_set_path
            .as_deref()
            .map(|path| resolve_entity_set_path(&parameters, path)),
        return_type: resolve_operation_return_type(
            action.return_type.as_ref(),
            entities,
            complexes,
            enums,
            type_definitions,
            ns,
            aliases,
            "Action",
            &action.name,
        )?,
        parameters,
    })
}

/// Resolve an operation `EntitySetPath` (`binding/Nav/.../Nav`) into typed
/// segments. The head names the binding parameter; remaining segments walk that
/// parameter's entity/complex type via navigation/structural properties. A
/// segment that cannot be resolved (or a type-cast) becomes
/// [`EntitySetPathSegment::Unresolved`] and walking stops.
fn resolve_entity_set_path(
    parameters: &[OperationParameter],
    path: &str,
) -> Arc<[EntitySetPathSegment]> {
    enum Ctx {
        Entity(Arc<EntityType>),
        Complex(Arc<ComplexType>),
    }

    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut out: Vec<EntitySetPathSegment> = Vec::new();
    let Some((head, rest)) = parts.split_first() else {
        return Arc::from(out);
    };
    out.push(EntitySetPathSegment::BindingParameter((*head).to_owned()));

    // Walk the remainder against the named parameter's type, if it resolves to a
    // structured type.
    let mut ctx = parameters
        .iter()
        .find(|parameter| parameter.name == *head)
        .and_then(|parameter| match &parameter.ty {
            TermType::Entity(entity) => Some(Ctx::Entity(entity.clone())),
            TermType::Complex(complex) => Some(Ctx::Complex(complex.clone())),
            _ => None,
        });

    for part in rest {
        // Qualified segment ⇒ type-cast; not resolved in v1.
        if part.contains('.') {
            out.push(EntitySetPathSegment::Unresolved((*part).to_owned()));
            break;
        }

        let Some(current) = ctx.take() else {
            out.push(EntitySetPathSegment::Unresolved((*part).to_owned()));
            break;
        };

        let (navs, props): (&[Arc<NavigationProperty>], &[Arc<Property>]) = match &current {
            Ctx::Entity(e) => (e.navigation_properties(), e.properties()),
            Ctx::Complex(c) => (c.navigation_properties(), c.properties()),
        };

        if let Some(nav) = navs.iter().find(|n| n.name == *part) {
            out.push(EntitySetPathSegment::NavigationProperty(Arc::downgrade(
                nav,
            )));
            if let Some(target) = nav.target.upgrade() {
                ctx = Some(Ctx::Entity(target));
            }
        } else if let Some(prop) = props.iter().find(|p| p.name == *part) {
            out.push(EntitySetPathSegment::Property(Arc::downgrade(prop)));
            if let ResolvedType::Complex(complex) = &prop.ty {
                ctx = Some(Ctx::Complex(complex.clone()));
            }
        } else {
            out.push(EntitySetPathSegment::Unresolved((*part).to_owned()));
            break;
        }
    }

    Arc::from(out)
}

#[allow(clippy::too_many_arguments)]
fn resolve_operation_parameters(
    parameters: &[csdl::Parameter],
    entities: &HashMap<String, Arc<EntityType>>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    ns: &str,
    aliases: &HashMap<String, String>,
    operation_kind: &'static str,
    operation_name: &str,
) -> Result<Vec<OperationParameter>, ResolveError> {
    parameters
        .iter()
        .map(|parameter| {
            let type_name =
                parameter
                    .type_name
                    .as_deref()
                    .ok_or_else(|| ResolveError::MissingTypeName {
                        element_kind: "Parameter",
                        element_name: format!(
                            "{}.{}({})",
                            operation_kind, operation_name, parameter.name
                        ),
                    })?;
            let ty = resolve_term_type(
                type_name,
                entities,
                complexes,
                enums,
                type_definitions,
                ns,
                aliases,
            )?;
            Ok(OperationParameter {
                name: parameter.name.clone(),
                ty,
                is_collection: parameter.is_collection,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn resolve_operation_return_type(
    return_type: Option<&csdl::ReturnType>,
    entities: &HashMap<String, Arc<EntityType>>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    enums: &HashMap<String, Arc<EnumType>>,
    type_definitions: &HashMap<String, Arc<TypeDefinition>>,
    ns: &str,
    aliases: &HashMap<String, String>,
    operation_kind: &'static str,
    operation_name: &str,
) -> Result<Option<OperationReturnType>, ResolveError> {
    let Some(return_type) = return_type else {
        return Ok(None);
    };
    let type_name =
        return_type
            .type_name
            .as_deref()
            .ok_or_else(|| ResolveError::MissingTypeName {
                element_kind: "ReturnType",
                element_name: format!("{}.{}", operation_kind, operation_name),
            })?;
    let ty = resolve_term_type(
        type_name,
        entities,
        complexes,
        enums,
        type_definitions,
        ns,
        aliases,
    )?;
    Ok(Some(OperationReturnType {
        ty,
        is_collection: return_type.is_collection,
    }))
}

fn resolve_primitive(type_name: &str) -> Option<PrimitiveType> {
    let rest = type_name.strip_prefix("Edm.")?;
    match rest {
        "Binary" => Some(PrimitiveType::Binary),
        "Boolean" => Some(PrimitiveType::Boolean),
        "Byte" => Some(PrimitiveType::Byte),
        "Date" => Some(PrimitiveType::Date),
        "DateTimeOffset" => Some(PrimitiveType::DateTimeOffset),
        "Decimal" => Some(PrimitiveType::Decimal),
        "Double" => Some(PrimitiveType::Double),
        "Duration" => Some(PrimitiveType::Duration),
        "Guid" => Some(PrimitiveType::Guid),
        "Int16" => Some(PrimitiveType::Int16),
        "Int32" => Some(PrimitiveType::Int32),
        "Int64" => Some(PrimitiveType::Int64),
        "SByte" => Some(PrimitiveType::SByte),
        "Single" => Some(PrimitiveType::Single),
        "String" => Some(PrimitiveType::String),
        "TimeOfDay" => Some(PrimitiveType::TimeOfDay),
        _ => None,
    }
}

fn resolve_navs(
    navs: &[csdl::NavigationProperty],
    entities: &HashMap<String, Arc<EntityType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<Vec<Arc<NavigationProperty>>, ResolveError> {
    navs.iter()
        .map(|n| {
            let type_name =
                n.type_name
                    .as_deref()
                    .ok_or_else(|| ResolveError::MissingTypeName {
                        element_kind: "NavigationProperty",
                        element_name: n.name.clone(),
                    })?;

            let short = unqualify_to_local(type_name, ns, aliases)
                .ok_or_else(|| ResolveError::UnknownEntity(type_name.to_string()))?;
            let target = entities
                .get(short)
                .ok_or_else(|| ResolveError::UnknownEntity(type_name.to_string()))?;

            Ok(Arc::new(NavigationProperty {
                name: n.name.clone(),
                target: Arc::downgrade(target),
                is_collection: n.is_collection,
                partner: OnceLock::new(),
                contains_target: n.contains_target,
                on_delete: n.on_delete.as_ref().map(|action| match action {
                    csdl::OnDeleteAction::Cascade => OnDeleteAction::Cascade,
                    csdl::OnDeleteAction::None => OnDeleteAction::None,
                    csdl::OnDeleteAction::SetNull => OnDeleteAction::SetNull,
                    csdl::OnDeleteAction::SetDefault => OnDeleteAction::SetDefault,
                }),
                referential_constraints: n
                    .referential_constraints
                    .iter()
                    .map(|constraint| ReferentialConstraint {
                        property: constraint.property.clone(),
                        referenced_property: constraint.referenced_property.clone(),
                    })
                    .collect(),
            }))
        })
        .collect()
}

fn resolve_entity_container(
    container: &csdl::EntityContainer,
    container_defs: &HashMap<String, &csdl::EntityContainer>,
    entities: &HashMap<String, Arc<EntityType>>,
    function_defs: &HashMap<String, &csdl::Function>,
    action_defs: &HashMap<String, &csdl::Action>,
    ns: &str,
    aliases: &HashMap<String, String>,
    visiting: &mut Vec<String>,
) -> Result<Arc<EntityContainer>, ResolveError> {
    if visiting.iter().any(|name| name == &container.name) {
        return Err(ResolveError::UnsupportedCsdlFeature {
            feature: "Cyclic EntityContainer.Extends",
            location: format!("{}.{}", ns, container.name),
        });
    }
    visiting.push(container.name.clone());

    let mut elements = Vec::new();
    let mut element_names: HashMap<String, ()> = HashMap::new();

    if let Some(base_ref) = &container.extends {
        let base_name = unqualify_to_local(base_ref, ns, aliases)
            .ok_or_else(|| ResolveError::UnknownType(base_ref.clone()))?;
        let base_container = container_defs
            .get(base_name)
            .ok_or_else(|| ResolveError::UnknownType(base_ref.clone()))?;

        let resolved_base = resolve_entity_container(
            base_container,
            container_defs,
            entities,
            function_defs,
            action_defs,
            ns,
            aliases,
            visiting,
        )?;

        for element in &resolved_base.elements {
            element_names.insert(entity_container_element_name(element).to_owned(), ());
            elements.push(element.clone());
        }
    }

    // Phase 1: create the EntitySet / Singleton Arcs with their bindings left
    // unset. Targets resolved below may reference sibling sets/singletons, so
    // every container element must exist before any binding is resolved.
    let mut pending_sets: Vec<(Arc<EntitySet>, &csdl::EntitySet, Arc<EntityType>)> = Vec::new();
    let mut pending_singletons: Vec<(Arc<Singleton>, &csdl::Singleton, Arc<EntityType>)> =
        Vec::new();

    for es in &container.entity_sets {
        if element_names.contains_key(&es.name) {
            visiting.pop();
            return Err(ResolveError::DuplicateName(format!(
                "{}.{}",
                container.name, es.name
            )));
        }
        let type_name = es
            .entity_type
            .as_deref()
            .ok_or_else(|| ResolveError::MissingTypeName {
                element_kind: "EntitySet",
                element_name: es.name.clone(),
            })?;

        let target = resolve_entity_ref(type_name, entities, ns, aliases)?;
        element_names.insert(es.name.clone(), ());
        let set = Arc::new(EntitySet {
            name: es.name.clone(),
            target: target.clone(),
            navigation_property_bindings: OnceLock::new(),
        });
        elements.push(Arc::new(EntityContainerElement::EntitySet(set.clone())));
        pending_sets.push((set, es, target));
    }

    for s in &container.singletons {
        if element_names.contains_key(&s.name) {
            visiting.pop();
            return Err(ResolveError::DuplicateName(format!(
                "{}.{}",
                container.name, s.name
            )));
        }
        let type_name = s
            .type_name
            .as_deref()
            .ok_or_else(|| ResolveError::MissingTypeName {
                element_kind: "Singleton",
                element_name: s.name.clone(),
            })?;

        let target = resolve_entity_ref(type_name, entities, ns, aliases)?;
        element_names.insert(s.name.clone(), ());
        let singleton = Arc::new(Singleton {
            name: s.name.clone(),
            target: target.clone(),
            navigation_property_bindings: OnceLock::new(),
        });
        elements.push(Arc::new(EntityContainerElement::Singleton(
            singleton.clone(),
        )));
        pending_singletons.push((singleton, s, target));
    }

    // Every set/singleton Arc (own + inherited via `extends`) now exists, so the
    // target maps can be built and shared by import-target resolution below and
    // the navigation-binding pass further down.
    let mut set_targets: HashMap<String, Weak<EntitySet>> = HashMap::new();
    let mut singleton_targets: HashMap<String, Weak<Singleton>> = HashMap::new();
    for element in &elements {
        match element.as_ref() {
            EntityContainerElement::EntitySet(set) => {
                set_targets.insert(set.name.clone(), Arc::downgrade(set));
            }
            EntityContainerElement::Singleton(singleton) => {
                singleton_targets.insert(singleton.name.clone(), Arc::downgrade(singleton));
            }
            EntityContainerElement::FunctionImport(_) | EntityContainerElement::ActionImport(_) => {
            }
        }
    }

    for fi in &container.function_imports {
        if element_names.contains_key(&fi.name) {
            visiting.pop();
            return Err(ResolveError::DuplicateName(format!(
                "{}.{}",
                container.name, fi.name
            )));
        }

        let function = fi
            .function
            .as_deref()
            .ok_or_else(|| ResolveError::MissingTypeName {
                element_kind: "FunctionImport",
                element_name: fi.name.clone(),
            })?
            .to_owned();

        resolve_operation_ref(&function, function_defs, ns, aliases)?;

        element_names.insert(fi.name.clone(), ());
        elements.push(Arc::new(EntityContainerElement::FunctionImport(Arc::new(
            FunctionImport {
                name: fi.name.clone(),
                function,
                entity_set: fi.entity_set.as_deref().map(|target| {
                    resolve_binding_target(
                        target,
                        &container.name,
                        &set_targets,
                        &singleton_targets,
                    )
                }),
            },
        ))));
    }

    for ai in &container.action_imports {
        if element_names.contains_key(&ai.name) {
            visiting.pop();
            return Err(ResolveError::DuplicateName(format!(
                "{}.{}",
                container.name, ai.name
            )));
        }

        let action = ai
            .action
            .as_deref()
            .ok_or_else(|| ResolveError::MissingTypeName {
                element_kind: "ActionImport",
                element_name: ai.name.clone(),
            })?
            .to_owned();

        resolve_operation_ref(&action, action_defs, ns, aliases)?;

        element_names.insert(ai.name.clone(), ());
        elements.push(Arc::new(EntityContainerElement::ActionImport(Arc::new(
            ActionImport {
                name: ai.name.clone(),
                action,
                entity_set: ai.entity_set.as_deref().map(|target| {
                    resolve_binding_target(
                        target,
                        &container.name,
                        &set_targets,
                        &singleton_targets,
                    )
                }),
            },
        ))));
    }

    // Phase 2: now that every set/singleton Arc exists (including any inherited
    // from `extends`), resolve each binding's path and target against the EDM
    // graph and fill the deferred `OnceLock`.
    for (set, csdl_set, source) in &pending_sets {
        let bindings = resolve_bindings(
            &csdl_set.navigation_property_bindings,
            source,
            &container.name,
            &set_targets,
            &singleton_targets,
        );
        let _ = set.navigation_property_bindings.set(bindings);
    }
    for (singleton, csdl_singleton, source) in &pending_singletons {
        let bindings = resolve_bindings(
            &csdl_singleton.navigation_property_bindings,
            source,
            &container.name,
            &set_targets,
            &singleton_targets,
        );
        let _ = singleton.navigation_property_bindings.set(bindings);
    }

    visiting.pop();

    Ok(Arc::new(EntityContainer {
        name: container.name.clone(),
        elements,
    }))
}

fn entity_container_element_name(element: &Arc<EntityContainerElement>) -> &str {
    match element.as_ref() {
        EntityContainerElement::EntitySet(set) => set.name.as_str(),
        EntityContainerElement::Singleton(singleton) => singleton.name.as_str(),
        EntityContainerElement::FunctionImport(import_) => import_.name.as_str(),
        EntityContainerElement::ActionImport(import_) => import_.name.as_str(),
    }
}

fn resolve_bindings(
    bindings: &[csdl::NavigationPropertyBinding],
    source: &Arc<EntityType>,
    container_name: &str,
    sets: &HashMap<String, Weak<EntitySet>>,
    singletons: &HashMap<String, Weak<Singleton>>,
) -> Vec<NavigationPropertyBinding> {
    bindings
        .iter()
        .map(|binding| NavigationPropertyBinding {
            path: resolve_binding_path(source, &binding.path),
            target: resolve_binding_target(&binding.target, container_name, sets, singletons),
        })
        .collect()
}

/// Resolve an effective key string (`Prop` or `Prop/SubProp/...`) into typed
/// segments against the entity's properties, descending through complex-typed
/// properties. Key paths are pre-validated to resolve, so
/// [`KeyPathSegment::Unresolved`] is a defensive fallback only.
fn resolve_key_path(entity: &Arc<EntityType>, key: &str) -> Arc<[KeyPathSegment]> {
    let mut out: Vec<KeyPathSegment> = Vec::new();
    let mut props: &[Arc<Property>] = entity.properties();
    let parts: Vec<&str> = key.split('/').filter(|s| !s.is_empty()).collect();

    for (i, part) in parts.iter().enumerate() {
        let is_last = i + 1 == parts.len();
        match props.iter().find(|p| p.name == *part) {
            Some(prop) => {
                out.push(KeyPathSegment::Property(Arc::downgrade(prop)));
                if is_last {
                    break;
                }
                match &prop.ty {
                    ResolvedType::Complex(complex) => props = complex.properties(),
                    _ => {
                        out.push(KeyPathSegment::Unresolved(parts[i + 1].to_owned()));
                        break;
                    }
                }
            }
            None => {
                out.push(KeyPathSegment::Unresolved((*part).to_owned()));
                break;
            }
        }
    }

    Arc::from(out)
}

/// Resolve a binding `Path` against the bound entity type, walking the EDM
/// graph. Each segment is a navigation property (the terminal, or a containment
/// hop), or a complex-typed structural property descended into. Type-cast
/// (qualified) segments are not resolved in v1. A segment that cannot be
/// resolved becomes [`BindingPathSegment::Unresolved`] and walking stops.
fn resolve_binding_path(source: &Arc<EntityType>, path: &str) -> Arc<[BindingPathSegment]> {
    enum Ctx {
        Entity(Arc<EntityType>),
        Complex(Arc<ComplexType>),
    }

    let mut out: Vec<BindingPathSegment> = Vec::new();
    let mut ctx = Ctx::Entity(source.clone());
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    for (i, part) in parts.iter().enumerate() {
        let is_last = i + 1 == parts.len();

        // Qualified segment ⇒ type-cast; not resolved in v1.
        if part.contains('.') {
            out.push(BindingPathSegment::Unresolved((*part).to_owned()));
            break;
        }

        let (navs, props): (&[Arc<NavigationProperty>], &[Arc<Property>]) = match &ctx {
            Ctx::Entity(e) => (e.navigation_properties(), e.properties()),
            Ctx::Complex(c) => (c.navigation_properties(), c.properties()),
        };

        if let Some(nav) = navs.iter().find(|n| n.name == *part) {
            out.push(BindingPathSegment::NavigationProperty(Arc::downgrade(nav)));
            if is_last {
                break;
            }
            match nav.target.upgrade() {
                Some(target) => ctx = Ctx::Entity(target),
                None => break,
            }
        } else if let Some(prop) = props.iter().find(|p| p.name == *part) {
            out.push(BindingPathSegment::Property(Arc::downgrade(prop)));
            match &prop.ty {
                ResolvedType::Complex(complex) if !is_last => ctx = Ctx::Complex(complex.clone()),
                _ => break,
            }
        } else {
            out.push(BindingPathSegment::Unresolved((*part).to_owned()));
            break;
        }
    }

    Arc::from(out)
}

/// Resolve a binding `Target` to an entity set / singleton, optionally through a
/// leading same-container qualifier and a trailing chain of containment
/// navigation properties. Cross-container / namespace-qualified targets are not
/// resolved in v1. Unresolved heads become [`BindingPathSegment::Unresolved`].
fn resolve_binding_target(
    target: &str,
    container_name: &str,
    sets: &HashMap<String, Weak<EntitySet>>,
    singletons: &HashMap<String, Weak<Singleton>>,
) -> Arc<[BindingPathSegment]> {
    let mut out: Vec<BindingPathSegment> = Vec::new();
    let mut parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Arc::from(out);
    }

    // Optional leading container qualifier (`Container` or `Namespace.Container`)
    // on a multi-segment target.
    if parts.len() > 1 {
        let head = parts[0];
        let head_local = head.rsplit('.').next().unwrap_or(head);
        if head.contains('.') {
            if head_local == container_name {
                parts.remove(0);
            } else {
                // Cross-container target — not resolved in v1.
                out.push(BindingPathSegment::Unresolved(target.to_owned()));
                return Arc::from(out);
            }
        } else if head == container_name {
            parts.remove(0);
        }
    }

    let Some(first) = parts.first().copied() else {
        return Arc::from(out);
    };

    let mut current: Arc<EntityType> = if let Some(weak) = sets.get(first) {
        out.push(BindingPathSegment::EntitySet(weak.clone()));
        match weak.upgrade() {
            Some(set) => set.target.clone(),
            None => return Arc::from(out),
        }
    } else if let Some(weak) = singletons.get(first) {
        out.push(BindingPathSegment::Singleton(weak.clone()));
        match weak.upgrade() {
            Some(singleton) => singleton.target.clone(),
            None => return Arc::from(out),
        }
    } else {
        out.push(BindingPathSegment::Unresolved(first.to_owned()));
        return Arc::from(out);
    };

    // Trailing segments must be containment navigation properties.
    for part in &parts[1..] {
        let nav = current
            .navigation_properties()
            .iter()
            .find(|n| n.name == *part && n.contains_target == Some(true))
            .cloned();
        match nav {
            Some(nav) => {
                out.push(BindingPathSegment::NavigationProperty(Arc::downgrade(&nav)));
                match nav.target.upgrade() {
                    Some(target) => current = target,
                    None => break,
                }
            }
            None => {
                out.push(BindingPathSegment::Unresolved((*part).to_owned()));
                break;
            }
        }
    }

    Arc::from(out)
}

fn resolve_operation_ref<'a, T>(
    type_name: &str,
    defs: &HashMap<String, &'a T>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<(), ResolveError> {
    let Some(short) = unqualify_to_local(type_name, ns, aliases) else {
        return Err(ResolveError::UnknownType(type_name.to_string()));
    };

    if defs.contains_key(short) {
        Ok(())
    } else {
        Err(ResolveError::UnknownType(type_name.to_string()))
    }
}

fn resolve_entity_ref(
    name: &str,
    entities: &HashMap<String, Arc<EntityType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<Arc<EntityType>, ResolveError> {
    let short = unqualify_to_local(name, ns, aliases)
        .ok_or_else(|| ResolveError::UnknownEntity(name.to_string()))?;
    entities
        .get(short)
        .cloned()
        .ok_or_else(|| ResolveError::UnknownEntity(name.to_string()))
}
