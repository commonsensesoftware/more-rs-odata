//! ---------------------------------------------------------------------------
//! Resolver
//! ---------------------------------------------------------------------------

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, Weak};

use crate::{csdl, edm::*};

mod containers;

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
        let legacy_schemas =
            (document.version.as_deref() == Some("4.0")).then(|| document.schemas.clone());
        let mut document_aliases = collect_document_aliases(&document.references);
        for schema in &document.schemas {
            if let Some(alias) = &schema.alias {
                if let Some(previous) =
                    document_aliases.insert(alias.clone(), schema.namespace.clone())
                {
                    if previous != schema.namespace {
                        return Err(ResolveError::DuplicateName(alias.clone()));
                    }
                }
            }
        }
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

        let schemas = Self::resolve_scope(document.schemas, &document_aliases)?
            .into_iter()
            .map(Arc::new)
            .collect::<Vec<_>>();

        if schemas.is_empty() {
            return Err(ResolveError::NoSchemaInDocument);
        }

        if let Some(authored_schemas) = legacy_schemas {
            for (authored, resolved) in authored_schemas.iter().zip(&schemas) {
                reject_legacy_member_redefinitions(authored, resolved)?;
            }
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
        Ok(Self::resolve_scope(vec![schema], &aliases)?.remove(0))
    }

    fn resolve_scope(
        schemas: Vec<csdl::Schema>,
        aliases: &HashMap<String, String>,
    ) -> Result<Vec<Model>, ResolveError> {
        let entity_defs = scoped_elements(&schemas)
            .filter_map(|(ns, el)| match el {
                csdl::SchemaElement::EntityType(e) => Some((format!("{ns}.{}", e.name), e)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();

        let mut entity_key_cache: HashMap<String, Vec<csdl::PropertyRef>> = HashMap::new();
        for name in entity_defs.keys() {
            let _ = resolve_effective_entity_keys(
                name,
                &entity_defs,
                "",
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
        for (ns, el) in scoped_elements(&schemas) {
            match el {
                csdl::SchemaElement::EntityType(e) => {
                    let arc = Arc::new(EntityType {
                        name: e.name.clone(),
                        is_abstract: e.abstract_.unwrap_or(false),
                        is_open: e.open_type.unwrap_or(false),
                        has_stream: e.has_stream.unwrap_or(false),
                        base_type: OnceLock::new(),
                        keys: std::sync::OnceLock::new(),
                        key_aliases: std::sync::OnceLock::new(),
                        properties: std::sync::OnceLock::new(),
                        navigation_properties: std::sync::OnceLock::new(),
                    });
                    insert_unique(&mut entities, &e.name, arc, ns)?;
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
                    insert_unique(&mut complexes, &c.name, arc, ns)?;
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
                    insert_unique(&mut enums, &en.name, arc, ns)?;
                }
                csdl::SchemaElement::TypeDefinition(td) => {
                    let primitive = resolve_primitive(&td.underlying_type)
                        .ok_or_else(|| ResolveError::UnknownType(td.underlying_type.clone()))?;
                    let arc = Arc::new(TypeDefinition {
                        facets: TypeFacets {
                            max_length: td.max_length,
                            precision: td.precision.clone(),
                            scale: td.scale,
                            srid: td.srid,
                            unicode: td.unicode,
                        },
                        name: td.name.clone(),
                        underlying_type: primitive,
                    });
                    insert_unique(&mut type_definitions, &td.name, arc, ns)?;
                }
                csdl::SchemaElement::Term(t) => {
                    let arc = Arc::new(Term {
                        facets: TypeFacets {
                            max_length: t.max_length,
                            precision: t.precision.clone(),
                            scale: t.scale,
                            srid: t.srid,
                            unicode: t.unicode,
                        },
                        nullable: t.nullable,
                        default_value: t.default_value.clone(),
                        applies_to: t
                            .applies_to
                            .iter()
                            .map(|s| AnnotationTargetKind::parse(s))
                            .collect(),
                        name: t.name.clone(),
                        is_collection: t.is_collection,
                        ty: std::sync::OnceLock::new(),
                        base_term: std::sync::OnceLock::new(),
                    });
                    insert_unique(&mut terms, &t.name, arc, ns)?;
                }
                csdl::SchemaElement::Function(_) | csdl::SchemaElement::Action(_) => {}
                csdl::SchemaElement::EntityContainer(_) => {}
            }
        }

        let complex_defs = scoped_elements(&schemas)
            .filter_map(|(ns, el)| match el {
                csdl::SchemaElement::ComplexType(c) => Some((format!("{ns}.{}", c.name), c)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();

        let mut container_defs: HashMap<String, &csdl::EntityContainer> = HashMap::new();
        for (ns, el) in scoped_elements(&schemas) {
            if let csdl::SchemaElement::EntityContainer(container) = el {
                if container_defs.contains_key(&format!("{ns}.{}", container.name)) {
                    return Err(ResolveError::DuplicateName(format!(
                        "{}.{}",
                        ns, container.name
                    )));
                }
                container_defs.insert(format!("{ns}.{}", container.name), container);
            }
        }

        // Weak base links preserve identity without retaining invalid cyclic inputs.
        for (ns, element) in scoped_elements(&schemas) {
            match element {
                csdl::SchemaElement::EntityType(e) => {
                    let base = e
                        .base_type
                        .as_ref()
                        .map(|name| {
                            let short = qualify_name(name, ns, aliases)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))?;
                            entities
                                .get(short.as_ref())
                                .map(Arc::downgrade)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))
                        })
                        .transpose()?;
                    let _ = entities[&format!("{ns}.{}", e.name)].base_type.set(base);
                }
                csdl::SchemaElement::ComplexType(c) => {
                    let base = c
                        .base_type
                        .as_ref()
                        .map(|name| {
                            let short = qualify_name(name, ns, aliases)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))?;
                            complexes
                                .get(short.as_ref())
                                .map(Arc::downgrade)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))
                        })
                        .transpose()?;
                    let _ = complexes[&format!("{ns}.{}", c.name)].base_type.set(base);
                }
                _ => {}
            }
        }

        let mut entity_member_cache: HashMap<String, ResolvedMembers> = HashMap::new();
        let mut complex_member_cache: HashMap<String, ResolvedMembers> = HashMap::new();

        for (ns, el) in scoped_elements(&schemas) {
            match el {
                csdl::SchemaElement::EntityType(e) => {
                    let arc = entities
                        .get(&format!("{ns}.{}", e.name))
                        .expect("inserted above");
                    let (props, navs) = resolve_entity_members(
                        &format!("{ns}.{}", e.name),
                        &entity_defs,
                        &complexes,
                        &enums,
                        &type_definitions,
                        &entities,
                        ns,
                        aliases,
                        &mut entity_member_cache,
                        &mut Vec::new(),
                    )?;
                    let _ = arc.properties.set(props);
                    let _ = arc.navigation_properties.set(navs);
                }
                csdl::SchemaElement::ComplexType(c) => {
                    let arc = complexes
                        .get(&format!("{ns}.{}", c.name))
                        .expect("inserted above");
                    let (props, navs) = resolve_complex_members(
                        &format!("{ns}.{}", c.name),
                        &complex_defs,
                        &complexes,
                        &enums,
                        &type_definitions,
                        &entities,
                        ns,
                        aliases,
                        &mut complex_member_cache,
                        &mut Vec::new(),
                    )?;
                    let _ = arc.properties.set(props);
                    let _ = arc.navigation_properties.set(navs);
                }
                csdl::SchemaElement::Term(t) => {
                    let arc = terms
                        .get(&format!("{ns}.{}", t.name))
                        .expect("inserted above");
                    let ty_name = t.type_name.as_deref().unwrap_or("Edm.String");
                    let ty = resolve_term_type(
                        ty_name,
                        &entities,
                        &complexes,
                        &enums,
                        &type_definitions,
                        ns,
                        aliases,
                    )?;
                    let _ = arc.ty.set(ty);

                    let base = match &t.base_term {
                        Some(name) => {
                            let short = qualify_name(name, ns, aliases)
                                .ok_or_else(|| ResolveError::UnknownType(name.clone()))?;
                            let base = terms
                                .get(short.as_ref())
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
        // effective key declarations can be resolved into typed paths.
        for (name, arc) in &entities {
            let keys = &entity_key_cache[name];
            let mut typed_keys = Vec::new();
            for key in keys {
                let path = resolve_key_path(arc, &key.name);
                if path
                    .iter()
                    .any(|s| matches!(s, KeyPathSegment::Unresolved(_)))
                {
                    return Err(ResolveError::UnknownPropertyPath {
                        entity: arc.name.clone(),
                        key_path: key.name.clone(),
                    });
                }
                typed_keys.push(path);
            }
            let _ = arc
                .key_aliases
                .set(keys.iter().map(|key| key.alias.clone()).collect());
            let _ = arc.keys.set(typed_keys);
        }

        // Late pass: resolve navigation `Partner` paths now that every entity's
        // navigation properties exist. A partner resolves against the
        // navigation's target entity type and ends at a navigation property.
        // Inherited navigation properties share the same `Arc`, so resolving
        // each owning type's own navigations covers derived types too.
        for (ns, el) in scoped_elements(&schemas) {
            let (csdl_navs, resolved_navs): (
                &[csdl::NavigationProperty],
                &[Arc<NavigationProperty>],
            ) = match el {
                csdl::SchemaElement::EntityType(e) => {
                    match entities.get(&format!("{ns}.{}", e.name)) {
                        Some(arc) => (&e.navigation_properties, arc.navigation_properties()),
                        None => continue,
                    }
                }
                csdl::SchemaElement::ComplexType(c) => {
                    match complexes.get(&format!("{ns}.{}", c.name)) {
                        Some(arc) => (&c.navigation_properties, arc.navigation_properties()),
                        None => continue,
                    }
                }
                _ => continue,
            };

            for csdl_nav in csdl_navs {
                let Some(resolved) = resolved_navs.iter().find(|n| n.name == csdl_nav.name) else {
                    continue;
                };
                let source_properties = match el {
                    csdl::SchemaElement::EntityType(e) => {
                        entities[&format!("{ns}.{}", e.name)].properties()
                    }
                    csdl::SchemaElement::ComplexType(c) => {
                        complexes[&format!("{ns}.{}", c.name)].properties()
                    }
                    _ => unreachable!(),
                };
                if let Some(target) = resolved.target.upgrade() {
                    for constraint in &resolved.referential_constraints {
                        let _ = constraint.property_path.set(resolve_constraint_path(
                            source_properties,
                            resolved_navs,
                            &constraint.property,
                        ));
                        let _ = constraint
                            .referenced_property_path
                            .set(resolve_constraint_path(
                                target.properties(),
                                target.navigation_properties(),
                                &constraint.referenced_property,
                            ));
                    }
                }
                let Some(partner) = csdl_nav.partner.as_deref() else {
                    continue;
                };
                let path = match resolved.target.upgrade() {
                    Some(target) => {
                        resolve_partner_path(&target, partner, &entities, &complexes, ns, aliases)
                    }
                    None => Arc::from([BindingPathSegment::Unresolved(partner.to_owned())]),
                };
                let _ = resolved.partner.set(path);
            }
        }

        let mut functions = HashMap::<String, Vec<Arc<Function>>>::new();
        let mut actions = HashMap::<String, Vec<Arc<Action>>>::new();
        let mut function_nodes = HashMap::new();
        let mut action_nodes = HashMap::new();
        for (ns, element) in scoped_elements(&schemas) {
            match element {
                csdl::SchemaElement::Function(function) => {
                    let node = Arc::new(resolve_function(
                        function,
                        &entities,
                        &complexes,
                        &enums,
                        &type_definitions,
                        ns,
                        aliases,
                    )?);
                    functions
                        .entry(format!("{ns}.{}", function.name))
                        .or_default()
                        .push(node.clone());
                    function_nodes.insert(function as *const csdl::Function, node);
                }
                csdl::SchemaElement::Action(action) => {
                    let node = Arc::new(resolve_action(
                        action,
                        &entities,
                        &complexes,
                        &enums,
                        &type_definitions,
                        ns,
                        aliases,
                    )?);
                    actions
                        .entry(format!("{ns}.{}", action.name))
                        .or_default()
                        .push(node.clone());
                    action_nodes.insert(action as *const csdl::Action, node);
                }
                _ => {}
            }
        }
        let mut containers = containers::Containers::new(
            &container_defs,
            &entities,
            &complexes,
            &functions,
            &actions,
            aliases,
        )?;
        let mut models = Vec::new();
        for schema in &schemas {
            let ns = &schema.namespace;
            let mut elements: Vec<Arc<SchemaElement>> = Vec::new();
            for el in &schema.elements {
                let wrapped = match el {
                    csdl::SchemaElement::EntityType(e) => SchemaElement::EntityType(
                        entities
                            .get(&format!("{ns}.{}", e.name))
                            .cloned()
                            .expect("entity exists"),
                    ),
                    csdl::SchemaElement::ComplexType(c) => SchemaElement::ComplexType(
                        complexes
                            .get(&format!("{ns}.{}", c.name))
                            .cloned()
                            .expect("complex exists"),
                    ),
                    csdl::SchemaElement::EnumType(en) => SchemaElement::EnumType(
                        enums
                            .get(&format!("{ns}.{}", en.name))
                            .cloned()
                            .expect("enum exists"),
                    ),
                    csdl::SchemaElement::TypeDefinition(td) => SchemaElement::TypeDefinition(
                        type_definitions
                            .get(&format!("{ns}.{}", td.name))
                            .cloned()
                            .expect("type definition exists"),
                    ),
                    csdl::SchemaElement::Term(t) => SchemaElement::Term(
                        terms
                            .get(&format!("{ns}.{}", t.name))
                            .cloned()
                            .expect("term exists"),
                    ),
                    csdl::SchemaElement::Function(function) => SchemaElement::Function(
                        function_nodes[&(function as *const csdl::Function)].clone(),
                    ),
                    csdl::SchemaElement::Action(action) => SchemaElement::Action(
                        action_nodes[&(action as *const csdl::Action)].clone(),
                    ),
                    csdl::SchemaElement::EntityContainer(_) => continue,
                };
                elements.push(Arc::new(wrapped));
            }

            let entity_containers = schema
                .elements
                .iter()
                .filter_map(|element| match element {
                    csdl::SchemaElement::EntityContainer(container) => {
                        Some(containers.resolve(&format!("{ns}.{}", container.name)))
                    }
                    _ => None,
                })
                .collect::<Result<Vec<_>, _>>()?;
            let entity_container = entity_containers.first().cloned();

            models.push(Model {
                namespace: schema.namespace.clone(),
                alias: schema.alias.clone(),
                elements,
                entity_container,
                entity_containers,
            });
        }
        Ok(models)
    }
}

fn scoped_elements(
    schemas: &[csdl::Schema],
) -> impl Iterator<Item = (&String, &csdl::SchemaElement)> {
    schemas.iter().flat_map(|schema| {
        schema
            .elements
            .iter()
            .map(move |element| (&schema.namespace, element))
    })
}

fn reject_legacy_member_redefinitions(
    authored: &csdl::Schema,
    resolved: &Model,
) -> Result<(), ResolveError> {
    for element in &authored.elements {
        let (name, properties, navigations, base_members) = match element {
            csdl::SchemaElement::EntityType(entity) => {
                let base = resolved
                    .elements
                    .iter()
                    .find_map(|element| match element.as_ref() {
                        SchemaElement::EntityType(ty) if ty.name == entity.name => ty
                            .base_type
                            .get()
                            .and_then(Option::as_ref)
                            .and_then(Weak::upgrade),
                        _ => None,
                    });
                let Some(base) = base else {
                    continue;
                };
                let members = base
                    .properties()
                    .iter()
                    .map(|p| p.name.clone())
                    .chain(base.navigation_properties().iter().map(|n| n.name.clone()))
                    .collect::<HashSet<_>>();
                (
                    &entity.name,
                    &entity.properties,
                    &entity.navigation_properties,
                    members,
                )
            }
            csdl::SchemaElement::ComplexType(complex) => {
                let base = resolved
                    .elements
                    .iter()
                    .find_map(|element| match element.as_ref() {
                        SchemaElement::ComplexType(ty) if ty.name == complex.name => ty
                            .base_type
                            .get()
                            .and_then(Option::as_ref)
                            .and_then(Weak::upgrade),
                        _ => None,
                    });
                let Some(base) = base else {
                    continue;
                };
                let members = base
                    .properties()
                    .iter()
                    .map(|p| p.name.clone())
                    .chain(base.navigation_properties().iter().map(|n| n.name.clone()))
                    .collect::<HashSet<_>>();
                (
                    &complex.name,
                    &complex.properties,
                    &complex.navigation_properties,
                    members,
                )
            }
            _ => continue,
        };
        if properties.iter().any(|p| base_members.contains(&p.name))
            || navigations.iter().any(|n| base_members.contains(&n.name))
        {
            return Err(ResolveError::UnsupportedCsdlFeature {
                feature: "Inherited member redefinition in CSDL 4.0",
                location: format!("{}.{}", authored.namespace, name),
            });
        }
    }
    Ok(())
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

fn qualify_name<'a>(
    name: &'a str,
    namespace: &str,
    aliases: &HashMap<String, String>,
) -> Option<Cow<'a, str>> {
    if let Some((qualifier, short)) = name.rsplit_once('.') {
        if let Some(namespace) = aliases.get(qualifier) {
            return Some(Cow::Owned(format!("{namespace}.{short}")));
        }
        Some(Cow::Borrowed(name))
    } else {
        Some(Cow::Owned(format!("{namespace}.{name}")))
    }
}

fn insert_unique<T>(
    map: &mut HashMap<String, T>,
    name: &str,
    value: T,
    ns: &str,
) -> Result<(), ResolveError> {
    let qualified = format!("{ns}.{name}");
    if map.contains_key(&qualified) {
        return Err(ResolveError::DuplicateName(format!("{}.{}", ns, name)));
    }
    map.insert(qualified, value);
    Ok(())
}

fn resolve_effective_entity_keys(
    name: &str,
    entity_defs: &HashMap<String, &csdl::EntityType>,
    ns: &str,
    aliases: &HashMap<String, String>,
    cache: &mut HashMap<String, Vec<csdl::PropertyRef>>,
    visiting: &mut Vec<String>,
) -> Result<Vec<csdl::PropertyRef>, ResolveError> {
    let ns = name.rsplit_once('.').map(|(ns, _)| ns).unwrap_or(ns);
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

    if entity
        .key
        .as_ref()
        .is_some_and(|key| key.property_refs.is_empty())
    {
        return Err(ResolveError::UnsupportedCsdlFeature {
            feature: "Empty EntityType.Key",
            location: name.to_owned(),
        });
    }

    let own_keys = entity
        .key
        .as_ref()
        .map(|k| k.property_refs.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    let effective_keys = if let Some(base_type) = &entity.base_type {
        let base_name = qualify_name(base_type, ns, aliases)
            .ok_or_else(|| ResolveError::UnknownType(base_type.clone()))?;
        let base_keys = resolve_effective_entity_keys(
            base_name.as_ref(),
            entity_defs,
            ns,
            aliases,
            cache,
            visiting,
        )?;

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
    let Some(short) = qualify_name(type_name, ns, aliases) else {
        return Err(ResolveError::UnknownType(type_name.to_string()));
    };
    if let Some(c) = complexes.get(short.as_ref()) {
        return Ok(ResolvedType::Complex(c.clone()));
    }
    if let Some(e) = enums.get(short.as_ref()) {
        return Ok(ResolvedType::Enum(e.clone()));
    }
    if let Some(td) = type_definitions.get(short.as_ref()) {
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
                facets: TypeFacets {
                    max_length: p.max_length,
                    precision: p.precision.clone(),
                    scale: p.scale,
                    srid: p.srid,
                    unicode: p.unicode,
                },
                name: p.name.clone(),
                ty,
                is_collection: p.is_collection,
                nullable: p.nullable,
                default_value: p.default_value.clone(),
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
    let ns = name.rsplit_once('.').map(|(ns, _)| ns).unwrap_or(ns);
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
        let base_name = qualify_name(base_type, ns, aliases)
            .ok_or_else(|| ResolveError::UnknownType(base_type.clone()))?;
        let (base_props, base_navs) = resolve_entity_members(
            base_name.as_ref(),
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
    let ns = name.rsplit_once('.').map(|(ns, _)| ns).unwrap_or(ns);
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
        let base_name = qualify_name(base_type, ns, aliases)
            .ok_or_else(|| ResolveError::UnknownType(base_type.clone()))?;
        let (base_props, base_navs) = resolve_complex_members(
            base_name.as_ref(),
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

    let Some(short) = qualify_name(type_name, ns, aliases) else {
        return Err(ResolveError::UnknownType(type_name.to_string()));
    };
    if let Some(td) = type_definitions.get(short.as_ref()) {
        return Ok(TermType::TypeDefinition(td.clone()));
    }
    if let Some(e) = enums.get(short.as_ref()) {
        return Ok(TermType::Enum(e.clone()));
    }
    if let Some(c) = complexes.get(short.as_ref()) {
        return Ok(TermType::Complex(c.clone()));
    }
    if let Some(e) = entities.get(short.as_ref()) {
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
            .map(|path| resolve_entity_set_path(&parameters, path, entities, ns, aliases)),
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
            .map(|path| resolve_entity_set_path(&parameters, path, entities, ns, aliases)),
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
    entities: &HashMap<String, Arc<EntityType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Arc<[EntitySetPathSegment]> {
    let parts: Vec<_> = path.split('/').collect();
    let mut out = vec![EntitySetPathSegment::BindingParameter(parts[0].into())];
    let mut current = parameters
        .first()
        .filter(|p| p.name == parts[0])
        .and_then(|p| {
            if let TermType::Entity(entity) = &p.ty {
                Some(entity.clone())
            } else {
                None
            }
        });
    for part in &parts[1..] {
        let Some(entity) = current.take() else {
            out.push(EntitySetPathSegment::Unresolved((*part).into()));
            break;
        };
        if part.contains('.') {
            let name = qualify_name(part, ns, aliases).expect("qualified segment");
            if let Some(derived) = entities.get(name.as_ref()).filter(|derived| {
                Arc::ptr_eq(derived, &entity) || entity_is_strict_subtype(derived, &entity)
            }) {
                out.push(EntitySetPathSegment::EntityTypeCast(Arc::downgrade(
                    derived,
                )));
                current = Some(derived.clone());
                continue;
            }
        } else if let Some(nav) = entity
            .navigation_properties()
            .iter()
            .find(|n| n.name == *part)
        {
            out.push(EntitySetPathSegment::NavigationProperty(Arc::downgrade(
                nav,
            )));
            current = nav.target.upgrade();
            continue;
        }
        out.push(EntitySetPathSegment::Unresolved((*part).into()));
        break;
    }
    out.into()
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
                facets: TypeFacets {
                    max_length: parameter.max_length,
                    precision: parameter.precision.clone(),
                    scale: parameter.scale,
                    srid: parameter.srid,
                    unicode: parameter.unicode,
                },
                default_value: parameter.default_value.clone(),
                name: parameter.name.clone(),
                ty,
                is_collection: parameter.is_collection,
                nullable: parameter.nullable,
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
        facets: TypeFacets {
            max_length: return_type.max_length,
            precision: return_type.precision.clone(),
            scale: return_type.scale,
            srid: return_type.srid,
            unicode: return_type.unicode,
        },
        ty,
        is_collection: return_type.is_collection,
        nullable: return_type.nullable,
    }))
}

fn resolve_primitive(type_name: &str) -> Option<PrimitiveType> {
    let rest = type_name.strip_prefix("Edm.")?;
    match rest {
        "Stream" => Some(PrimitiveType::Stream),
        "Geography" => Some(PrimitiveType::Geography),
        "GeographyPoint" => Some(PrimitiveType::GeographyPoint),
        "GeographyLineString" => Some(PrimitiveType::GeographyLineString),
        "GeographyPolygon" => Some(PrimitiveType::GeographyPolygon),
        "GeographyMultiPoint" => Some(PrimitiveType::GeographyMultiPoint),
        "GeographyMultiLineString" => Some(PrimitiveType::GeographyMultiLineString),
        "GeographyMultiPolygon" => Some(PrimitiveType::GeographyMultiPolygon),
        "GeographyCollection" => Some(PrimitiveType::GeographyCollection),
        "Geometry" => Some(PrimitiveType::Geometry),
        "GeometryPoint" => Some(PrimitiveType::GeometryPoint),
        "GeometryLineString" => Some(PrimitiveType::GeometryLineString),
        "GeometryPolygon" => Some(PrimitiveType::GeometryPolygon),
        "GeometryMultiPoint" => Some(PrimitiveType::GeometryMultiPoint),
        "GeometryMultiLineString" => Some(PrimitiveType::GeometryMultiLineString),
        "GeometryMultiPolygon" => Some(PrimitiveType::GeometryMultiPolygon),
        "GeometryCollection" => Some(PrimitiveType::GeometryCollection),
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

            let short = qualify_name(type_name, ns, aliases)
                .ok_or_else(|| ResolveError::UnknownEntity(type_name.to_string()))?;
            let target = entities
                .get(short.as_ref())
                .ok_or_else(|| ResolveError::UnknownEntity(type_name.to_string()))?;

            Ok(Arc::new(NavigationProperty {
                name: n.name.clone(),
                target: Arc::downgrade(target),
                is_collection: n.is_collection,
                nullable: n.nullable,
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
                        property_path: OnceLock::new(),
                        referenced_property_path: OnceLock::new(),
                    })
                    .collect(),
            }))
        })
        .collect()
}

fn entity_container_element_name(element: &Arc<EntityContainerElement>) -> &str {
    match element.as_ref() {
        EntityContainerElement::EntitySet(set) => set.name.as_str(),
        EntityContainerElement::Singleton(singleton) => singleton.name.as_str(),
        EntityContainerElement::FunctionImport(import_) => import_.name.as_str(),
        EntityContainerElement::ActionImport(import_) => import_.name.as_str(),
    }
}

/// Resolve an effective key string (`Prop` or `Prop/SubProp/...`) into typed
/// segments against the entity's properties, descending through complex-typed
/// properties. Key paths are pre-validated to resolve, so
/// [`KeyPathSegment::Unresolved`] is a defensive fallback only.
fn resolve_key_path(entity: &Arc<EntityType>, key: &str) -> Arc<[KeyPathSegment]> {
    let mut properties = entity.properties().to_vec();
    let mut navigations = entity.navigation_properties().to_vec();
    let mut path = Vec::new();
    let parts: Vec<_> = key.split('/').collect();
    for (index, name) in parts.iter().enumerate() {
        if let Some(property) = properties.iter().cloned().find(|p| p.name == *name) {
            path.push(KeyPathSegment::Property(Arc::downgrade(&property)));
            if index + 1 == parts.len() {
                return path.into();
            }
            if let ResolvedType::Complex(complex) = &property.ty {
                properties = complex.properties().to_vec();
                navigations = complex.navigation_properties().to_vec();
                continue;
            }
        } else if let Some(nav) = navigations.iter().cloned().find(|n| n.name == *name) {
            path.push(KeyPathSegment::NavigationProperty(Arc::downgrade(&nav)));
            let target = nav.target.upgrade().expect("resolved navigation target");
            properties = target.properties().to_vec();
            navigations = target.navigation_properties().to_vec();
            continue;
        }
        path.push(KeyPathSegment::Unresolved((*name).into()));
        break;
    }
    path.into()
}

fn resolve_constraint_path<'a>(
    mut properties: &'a [Arc<Property>],
    mut navigations: &'a [Arc<NavigationProperty>],
    path: &str,
) -> Arc<[ConstraintPathSegment]> {
    let mut out = Vec::new();
    let parts: Vec<_> = path.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        if let Some(property) = properties.iter().find(|p| p.name == *part) {
            out.push(ConstraintPathSegment::Property(Arc::downgrade(property)));
            if last {
                return out.into();
            }
            if !property.is_collection {
                if let ResolvedType::Complex(complex) = &property.ty {
                    properties = complex.properties();
                    navigations = complex.navigation_properties();
                    continue;
                }
            }
        } else if last {
            if let Some(nav) = navigations.iter().find(|n| n.name == *part) {
                out.push(ConstraintPathSegment::NavigationProperty(Arc::downgrade(
                    nav,
                )));
                return out.into();
            }
        }
        out.push(ConstraintPathSegment::Unresolved((*part).to_owned()));
        break;
    }
    out.into()
}

/// Resolve a binding `Path` against the bound entity type, walking the EDM
/// graph. Each segment is a navigation property (the terminal, or a containment
/// hop), or a complex-typed structural property descended into. Type-cast
/// (qualified) segments are not resolved in v1. A segment that cannot be
/// resolved becomes [`BindingPathSegment::Unresolved`] and walking stops.
fn resolve_partner_path(
    source: &Arc<EntityType>,
    path: &str,
    entities: &HashMap<String, Arc<EntityType>>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Arc<[BindingPathSegment]> {
    enum Context {
        Entity(Arc<EntityType>),
        Complex(Arc<ComplexType>),
    }
    let mut context = Context::Entity(source.clone());
    let mut out = Vec::new();
    let parts: Vec<_> = path.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        if part.contains('.') && !last {
            let qualified = qualify_name(part, ns, aliases).expect("qualified path segment");
            match &context {
                Context::Entity(base) => {
                    if let Some(derived) = entities.get(qualified.as_ref()).filter(|derived| {
                        Arc::ptr_eq(derived, base) || entity_is_strict_subtype(derived, base)
                    }) {
                        out.push(BindingPathSegment::EntityTypeCast(Arc::downgrade(derived)));
                        context = Context::Entity(derived.clone());
                        continue;
                    }
                }
                Context::Complex(base) => {
                    if let Some(derived) = complexes.get(qualified.as_ref()).filter(|derived| {
                        Arc::ptr_eq(derived, base) || complex_is_strict_subtype(derived, base)
                    }) {
                        out.push(BindingPathSegment::ComplexTypeCast(Arc::downgrade(derived)));
                        context = Context::Complex(derived.clone());
                        continue;
                    }
                }
            }
        } else {
            let (props, navs) = match &context {
                Context::Entity(ty) => (ty.properties(), ty.navigation_properties()),
                Context::Complex(ty) => (ty.properties(), ty.navigation_properties()),
            };
            if last {
                if let Some(nav) = navs.iter().find(|n| n.name == *part) {
                    out.push(BindingPathSegment::NavigationProperty(Arc::downgrade(nav)));
                    return out.into();
                }
            } else if let Some(prop) = props.iter().find(|p| p.name == *part) {
                if let ResolvedType::Complex(complex) = &prop.ty {
                    out.push(BindingPathSegment::Property(Arc::downgrade(prop)));
                    context = Context::Complex(complex.clone());
                    continue;
                }
            }
        }
        out.push(BindingPathSegment::Unresolved((*part).to_owned()));
        break;
    }
    out.into()
}

fn resolve_binding_path(
    source: &Arc<EntityType>,
    path: &str,
    entities: &HashMap<String, Arc<EntityType>>,
    complexes: &HashMap<String, Arc<ComplexType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Arc<[BindingPathSegment]> {
    enum Ctx {
        Entity(Arc<EntityType>),
        Complex(Arc<ComplexType>),
    }

    let mut out: Vec<BindingPathSegment> = Vec::new();
    let mut ctx = Ctx::Entity(source.clone());
    let parts: Vec<&str> = path.split('/').collect();

    for (i, part) in parts.iter().enumerate() {
        let is_last = i + 1 == parts.len();

        if part.contains('.') {
            let qualified = qualify_name(part, ns, aliases).unwrap();
            let cast = match &ctx {
                Ctx::Entity(base) => entities
                    .get(qualified.as_ref())
                    .filter(|ty| Arc::ptr_eq(ty, base) || entity_is_strict_subtype(ty, base))
                    .map(|ty| {
                        (
                            BindingPathSegment::EntityTypeCast(Arc::downgrade(ty)),
                            Ctx::Entity(ty.clone()),
                        )
                    }),
                Ctx::Complex(base) => complexes
                    .get(qualified.as_ref())
                    .filter(|ty| Arc::ptr_eq(ty, base) || complex_is_strict_subtype(ty, base))
                    .map(|ty| {
                        (
                            BindingPathSegment::ComplexTypeCast(Arc::downgrade(ty)),
                            Ctx::Complex(ty.clone()),
                        )
                    }),
            };
            if let Some((segment, next)) = cast {
                out.push(segment);
                ctx = next;
                continue;
            }
            out.push(BindingPathSegment::Unresolved((*part).into()));
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

fn resolve_entity_ref(
    name: &str,
    entities: &HashMap<String, Arc<EntityType>>,
    ns: &str,
    aliases: &HashMap<String, String>,
) -> Result<Arc<EntityType>, ResolveError> {
    let short = qualify_name(name, ns, aliases)
        .ok_or_else(|| ResolveError::UnknownEntity(name.to_string()))?;
    entities
        .get(short.as_ref())
        .cloned()
        .ok_or_else(|| ResolveError::UnknownEntity(name.to_string()))
}
