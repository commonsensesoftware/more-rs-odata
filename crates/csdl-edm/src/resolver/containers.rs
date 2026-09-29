use super::*;

pub(super) struct Containers<'a> {
    pub definitions: &'a HashMap<String, &'a csdl::EntityContainer>,
    pub entities: &'a HashMap<String, Arc<EntityType>>,
    pub complexes: &'a HashMap<String, Arc<ComplexType>>,
    pub functions: &'a HashMap<String, Vec<Arc<Function>>>,
    pub actions: &'a HashMap<String, Vec<Arc<Action>>>,
    pub aliases: &'a HashMap<String, String>,
    resources: HashMap<String, Vec<Arc<EntityContainerElement>>>,
    resolved: HashMap<String, Arc<EntityContainer>>,
}

impl<'a> Containers<'a> {
    pub fn new(
        definitions: &'a HashMap<String, &'a csdl::EntityContainer>,
        entities: &'a HashMap<String, Arc<EntityType>>,
        complexes: &'a HashMap<String, Arc<ComplexType>>,
        functions: &'a HashMap<String, Vec<Arc<Function>>>,
        actions: &'a HashMap<String, Vec<Arc<Action>>>,
        aliases: &'a HashMap<String, String>,
    ) -> Result<Self, ResolveError> {
        let mut result = Self {
            definitions,
            entities,
            complexes,
            functions,
            actions,
            aliases,
            resources: HashMap::new(),
            resolved: HashMap::new(),
        };
        for name in definitions.keys() {
            result.allocate(name, &mut HashSet::new())?;
        }
        Ok(result)
    }

    fn allocate(
        &mut self,
        name: &str,
        visiting: &mut HashSet<String>,
    ) -> Result<Vec<Arc<EntityContainerElement>>, ResolveError> {
        if let Some(resources) = self.resources.get(name) {
            return Ok(resources.clone());
        }
        if !visiting.insert(name.into()) {
            return Err(ResolveError::UnsupportedCsdlFeature {
                feature: "Cyclic EntityContainer.Extends",
                location: name.into(),
            });
        }
        let definition = self
            .definitions
            .get(name)
            .ok_or_else(|| ResolveError::UnknownType(name.into()))?;
        let ns = name.rsplit_once('.').unwrap().0;
        let mut resources = if let Some(base) = &definition.extends {
            self.allocate(&qualify_name(base, ns, self.aliases).unwrap(), visiting)?
        } else {
            Vec::new()
        };
        let mut own_names = HashSet::new();
        for set in &definition.entity_sets {
            let ty = set
                .entity_type
                .as_ref()
                .ok_or_else(|| ResolveError::MissingTypeName {
                    element_kind: "EntitySet",
                    element_name: set.name.clone(),
                })?;
            let target = resolve_entity_ref(ty, self.entities, ns, self.aliases)?;
            if Arc::ptr_eq(&target, abstract_entity_type()) {
                return Err(ResolveError::UnsupportedCsdlFeature {
                    feature: "EntitySet cannot use Edm.EntityType",
                    location: set.name.clone(),
                });
            }
            let element = Arc::new(EntityContainerElement::EntitySet(Arc::new(EntitySet {
                name: set.name.clone(),
                target,
                include_in_service_document: set.include_in_service_document.unwrap_or(true),
                navigation_property_bindings: OnceLock::new(),
            })));
            Self::insert_resource(name, &mut resources, &mut own_names, element)?;
        }
        for singleton in &definition.singletons {
            let ty = singleton
                .type_name
                .as_ref()
                .ok_or_else(|| ResolveError::MissingTypeName {
                    element_kind: "Singleton",
                    element_name: singleton.name.clone(),
                })?;
            let target = resolve_entity_ref(ty, self.entities, ns, self.aliases)?;
            if Arc::ptr_eq(&target, abstract_entity_type()) {
                return Err(ResolveError::UnsupportedCsdlFeature {
                    feature: "Singleton cannot use Edm.EntityType",
                    location: singleton.name.clone(),
                });
            }
            let element = Arc::new(EntityContainerElement::Singleton(Arc::new(Singleton {
                name: singleton.name.clone(),
                target,
                nullable: singleton.nullable,
                navigation_property_bindings: OnceLock::new(),
            })));
            Self::insert_resource(name, &mut resources, &mut own_names, element)?;
        }
        visiting.remove(name);
        self.resources.insert(name.into(), resources.clone());
        Ok(resources)
    }

    fn insert_resource(
        container: &str,
        resources: &mut Vec<Arc<EntityContainerElement>>,
        own_names: &mut HashSet<String>,
        element: Arc<EntityContainerElement>,
    ) -> Result<(), ResolveError> {
        let name = entity_container_element_name(&element);
        if !own_names.insert(name.into()) {
            return Err(ResolveError::DuplicateName(format!("{container}.{name}")));
        }
        if let Some(index) = resources
            .iter()
            .position(|previous| entity_container_element_name(previous) == name)
        {
            let allowed = match (element.as_ref(), resources[index].as_ref()) {
                (EntityContainerElement::EntitySet(a), EntityContainerElement::EntitySet(b)) => {
                    entity_is_strict_subtype(&a.target, &b.target)
                }
                (EntityContainerElement::Singleton(a), EntityContainerElement::Singleton(b)) => {
                    entity_is_strict_subtype(&a.target, &b.target)
                }
                _ => false,
            };
            if !allowed {
                return Err(ResolveError::DuplicateName(format!("{container}.{name}")));
            }
            resources[index] = element;
        } else {
            resources.push(element);
        }
        Ok(())
    }

    pub fn resolve(&mut self, name: &str) -> Result<Arc<EntityContainer>, ResolveError> {
        if let Some(container) = self.resolved.get(name) {
            return Ok(container.clone());
        }
        let definition = self.definitions[name];
        let ns = name.rsplit_once('.').unwrap().0;
        let mut elements = self.resources[name].clone();
        let extends = definition
            .extends
            .as_ref()
            .map(|base| self.resolve(&qualify_name(base, ns, self.aliases).unwrap()))
            .transpose()?;
        if let Some(base) = &extends {
            for element in &base.elements {
                if matches!(
                    element.as_ref(),
                    EntityContainerElement::FunctionImport(_)
                        | EntityContainerElement::ActionImport(_)
                ) {
                    if elements.iter().any(|other| {
                        entity_container_element_name(other)
                            == entity_container_element_name(element)
                    }) {
                        return Err(ResolveError::DuplicateName(format!(
                            "{name}.{}",
                            entity_container_element_name(element)
                        )));
                    }
                    elements.push(element.clone());
                }
            }
        }
        for import in &definition.function_imports {
            Self::require_unique(name, &elements, &import.name)?;
            let function =
                import
                    .function
                    .as_ref()
                    .ok_or_else(|| ResolveError::MissingTypeName {
                        element_kind: "FunctionImport",
                        element_name: import.name.clone(),
                    })?;
            let qualified = qualify_name(function, ns, self.aliases).unwrap();
            let overloads = self
                .functions
                .get(qualified.as_ref())
                .ok_or_else(|| ResolveError::UnknownType(function.clone()))?
                .iter()
                .filter(|f| !f.is_bound)
                .cloned()
                .collect::<Vec<_>>();
            if overloads.is_empty() {
                return Err(ResolveError::UnsupportedCsdlFeature {
                    feature: "FunctionImport requires an unbound function",
                    location: format!("{name}.{}", import.name),
                });
            }
            elements.push(Arc::new(EntityContainerElement::FunctionImport(Arc::new(
                FunctionImport {
                    name: import.name.clone(),
                    function: function.clone(),
                    overloads,
                    include_in_service_document: import
                        .include_in_service_document
                        .unwrap_or(false),
                    entity_set: import
                        .entity_set
                        .as_ref()
                        .map(|path| self.target(name, path)),
                },
            ))));
        }
        for import in &definition.action_imports {
            Self::require_unique(name, &elements, &import.name)?;
            let action = import
                .action
                .as_ref()
                .ok_or_else(|| ResolveError::MissingTypeName {
                    element_kind: "ActionImport",
                    element_name: import.name.clone(),
                })?;
            let qualified = qualify_name(action, ns, self.aliases).unwrap();
            let operations = self
                .actions
                .get(qualified.as_ref())
                .ok_or_else(|| ResolveError::UnknownType(action.clone()))?;
            let operation = operations
                .iter()
                .find(|a| !a.is_bound)
                .cloned()
                .ok_or_else(|| ResolveError::UnsupportedCsdlFeature {
                    feature: "ActionImport requires an unbound action",
                    location: format!("{name}.{}", import.name),
                })?;
            elements.push(Arc::new(EntityContainerElement::ActionImport(Arc::new(
                ActionImport {
                    name: import.name.clone(),
                    action: action.clone(),
                    operation,
                    entity_set: import
                        .entity_set
                        .as_ref()
                        .map(|path| self.target(name, path)),
                },
            ))));
        }
        for set in &definition.entity_sets {
            let resource = elements
                .iter()
                .find(|e| entity_container_element_name(e) == set.name)
                .unwrap();
            if let EntityContainerElement::EntitySet(resource) = resource.as_ref() {
                let bindings =
                    self.bindings(name, &resource.target, &set.navigation_property_bindings);
                let _ = resource.navigation_property_bindings.set(bindings);
            }
        }
        for singleton in &definition.singletons {
            let resource = elements
                .iter()
                .find(|e| entity_container_element_name(e) == singleton.name)
                .unwrap();
            if let EntityContainerElement::Singleton(resource) = resource.as_ref() {
                let bindings = self.bindings(
                    name,
                    &resource.target,
                    &singleton.navigation_property_bindings,
                );
                let _ = resource.navigation_property_bindings.set(bindings);
            }
        }
        let container = Arc::new(EntityContainer {
            name: definition.name.clone(),
            extends,
            elements,
        });
        self.resolved.insert(name.into(), container.clone());
        Ok(container)
    }

    fn require_unique(
        container: &str,
        elements: &[Arc<EntityContainerElement>],
        name: &str,
    ) -> Result<(), ResolveError> {
        if elements
            .iter()
            .any(|e| entity_container_element_name(e) == name)
        {
            Err(ResolveError::DuplicateName(format!("{container}.{name}")))
        } else {
            Ok(())
        }
    }

    fn bindings(
        &self,
        container: &str,
        source: &Arc<EntityType>,
        definitions: &[csdl::NavigationPropertyBinding],
    ) -> Vec<NavigationPropertyBinding> {
        let ns = container.rsplit_once('.').unwrap().0;
        definitions
            .iter()
            .map(|binding| NavigationPropertyBinding {
                path: resolve_binding_path(
                    source,
                    &binding.path,
                    self.entities,
                    self.complexes,
                    ns,
                    self.aliases,
                ),
                target: self.target(container, &binding.target),
            })
            .collect()
    }

    fn target(&self, container: &str, path: &str) -> Arc<[BindingPathSegment]> {
        let ns = container.rsplit_once('.').unwrap().0;
        let mut parts = path.split('/').peekable();
        let first = parts.next().unwrap_or("");
        let qualified = qualify_name(first, ns, self.aliases).unwrap();
        let (container, resource) =
            if parts.peek().is_some() && self.resources.contains_key(qualified.as_ref()) {
                (qualified.as_ref(), parts.next().unwrap())
            } else {
                (container, first)
            };
        let Some(element) = self.resources[container]
            .iter()
            .find(|e| entity_container_element_name(e) == resource)
        else {
            return Arc::from([BindingPathSegment::Unresolved(path.into())]);
        };
        let (head, entity) = match element.as_ref() {
            EntityContainerElement::EntitySet(set) => (
                BindingPathSegment::EntitySet(Arc::downgrade(set)),
                set.target.clone(),
            ),
            EntityContainerElement::Singleton(singleton) => (
                BindingPathSegment::Singleton(Arc::downgrade(singleton)),
                singleton.target.clone(),
            ),
            _ => unreachable!(),
        };
        let mut out = vec![head];
        enum Context {
            Entity(Arc<EntityType>),
            Complex(Arc<ComplexType>),
        }
        let mut context = Context::Entity(entity);
        for part in parts {
            let (props, navs) = match &context {
                Context::Entity(ty) => (ty.properties(), ty.navigation_properties()),
                Context::Complex(ty) => (ty.properties(), ty.navigation_properties()),
            };
            if let Some(nav) = navs.iter().find(|n| n.name == part) {
                out.push(BindingPathSegment::NavigationProperty(Arc::downgrade(nav)));
                if let Some(target) = nav.target.upgrade() {
                    context = Context::Entity(target);
                    continue;
                }
            } else if let Some(prop) = props.iter().find(|p| p.name == part) {
                out.push(BindingPathSegment::Property(Arc::downgrade(prop)));
                if let ResolvedType::Complex(complex) = &prop.ty {
                    context = Context::Complex(complex.clone());
                    continue;
                }
            }
            out.push(BindingPathSegment::Unresolved(part.into()));
            break;
        }
        out.into()
    }
}
