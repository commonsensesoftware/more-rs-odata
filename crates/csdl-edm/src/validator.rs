//! Semantic validation that is intentionally separate from reference resolution.

use std::collections::HashSet;

use crate::edm::{
    Action, BindingPathSegment, ComplexType, DocumentModel, EntityContainer,
    EntityContainerElement, EntitySetPathSegment, EntityType, EnumType, Function, KeyPathSegment,
    Model, NavigationPropertyBinding, ResolvedType, SchemaElement, Term, binding_path_to_string,
    entity_set_path_to_string, key_path_to_string,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyPathClassification {
    Unknown,
    Scalar,
    Complex,
    Collection,
}

#[derive(Debug, Clone)]
pub enum ValidationError {
    InvalidInheritance {
        type_kind: &'static str,
        type_name: String,
        reason: &'static str,
    },
    DuplicateKey {
        entity: String,
        key: String,
    },
    UnknownKeyProperty {
        entity: String,
        key: String,
    },
    NonScalarKeyProperty {
        entity: String,
        key: String,
    },
    DuplicateChildName {
        parent_kind: &'static str,
        parent: String,
        child_kind: &'static str,
        child: String,
    },
    UnknownContainerTarget {
        container: String,
        source_kind: &'static str,
        source: String,
        target: String,
    },
    InvalidNavigationPropertyBinding {
        container: String,
        source_kind: &'static str,
        source: String,
        attribute: &'static str,
        value: String,
        reason: &'static str,
    },
    BoundOperationMissingBindingParameter {
        operation_kind: &'static str,
        operation: String,
    },
    InvalidEntitySetPath {
        operation_kind: &'static str,
        operation: String,
        path: String,
        reason: &'static str,
    },
    UnknownNavigationPartner {
        entity: String,
        navigation: String,
        partner: String,
        target_entity: String,
    },
    UnknownReferentialConstraintProperty {
        entity: String,
        navigation: String,
        property: String,
    },
    UnknownReferentialConstraintReferencedProperty {
        entity: String,
        navigation: String,
        target_entity: String,
        referenced_property: String,
    },
    CyclicTermBaseTerm {
        term: String,
    },
}

pub fn validate_document(document: &DocumentModel) -> Result<(), Vec<ValidationError>> {
    let engine = ValidatorEngine::default();
    let mut errors = Vec::new();

    for schema in &document.schemas {
        errors.extend(engine.validate_model_collect(schema.as_ref()));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn validate_model(model: &Model) -> Result<(), Vec<ValidationError>> {
    let engine = ValidatorEngine::default();
    let errors = engine.validate_model_collect(model);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[derive(Default)]
struct ValidationContext {
    errors: Vec<ValidationError>,
}

impl ValidationContext {
    fn push(&mut self, error: ValidationError) {
        self.errors.push(error);
    }
}

trait ValidationRule: Send + Sync {
    fn name(&self) -> &'static str;

    fn visit_model(&self, _model: &Model, _ctx: &mut ValidationContext) {}
    fn visit_entity_type(&self, _entity: &EntityType, _ctx: &mut ValidationContext) {}
    fn visit_complex_type(&self, _complex: &ComplexType, _ctx: &mut ValidationContext) {}
    fn visit_enum_type(&self, _enum_type: &EnumType, _ctx: &mut ValidationContext) {}
    fn visit_function(&self, _function: &Function, _ctx: &mut ValidationContext) {}
    fn visit_action(&self, _action: &Action, _ctx: &mut ValidationContext) {}
    fn visit_term(&self, _term: &Term, _ctx: &mut ValidationContext) {}
    fn visit_entity_container(&self, _container: &EntityContainer, _ctx: &mut ValidationContext) {}
}

struct ValidatorEngine {
    rules: Vec<Box<dyn ValidationRule>>,
}

impl Default for ValidatorEngine {
    fn default() -> Self {
        Self {
            rules: vec![
                Box::new(InheritanceSemanticsRule),
                Box::new(UniqueSchemaChildNamesRule),
                Box::new(UniqueEntityChildNamesRule),
                Box::new(UniqueComplexChildNamesRule),
                Box::new(UniqueEnumMemberNamesRule),
                Box::new(UniqueEntityContainerChildNamesRule),
                Box::new(NavigationPropertyBindingSemanticsRule),
                Box::new(KnownEntityContainerTargetsRule),
                Box::new(NavigationPartnerConsistencyRule),
                Box::new(ReferentialConstraintConsistencyRule),
                Box::new(TermBaseTermCycleRule),
                Box::new(BoundOperationBindingParameterRule),
                Box::new(OperationEntitySetPathRule),
                Box::new(DuplicateEntityKeyRule),
                Box::new(UnknownEntityKeyPropertyRule),
                Box::new(NonScalarEntityKeyRule),
            ],
        }
    }
}

impl ValidatorEngine {
    fn validate_model_collect(&self, model: &Model) -> Vec<ValidationError> {
        let mut ctx = ValidationContext::default();
        self.walk_model(model, &mut ctx);
        ctx.errors
    }

    fn apply_rules(&self, mut f: impl FnMut(&dyn ValidationRule)) {
        for rule in &self.rules {
            let _ = rule.name();
            f(rule.as_ref());
        }
    }

    fn walk_model(&self, model: &Model, ctx: &mut ValidationContext) {
        self.apply_rules(|rule| rule.visit_model(model, ctx));

        for element in &model.elements {
            match element.as_ref() {
                SchemaElement::EntityType(entity) => {
                    self.apply_rules(|rule| rule.visit_entity_type(entity, ctx));
                }
                SchemaElement::ComplexType(complex) => {
                    self.apply_rules(|rule| rule.visit_complex_type(complex, ctx));
                }
                SchemaElement::EnumType(enum_type) => {
                    self.apply_rules(|rule| rule.visit_enum_type(enum_type, ctx));
                }
                SchemaElement::Function(function) => {
                    self.apply_rules(|rule| rule.visit_function(function, ctx));
                }
                SchemaElement::Action(action) => {
                    self.apply_rules(|rule| rule.visit_action(action, ctx));
                }
                SchemaElement::TypeDefinition(_) => {}
                SchemaElement::Term(term) => {
                    self.apply_rules(|rule| rule.visit_term(term, ctx));
                }
            }
        }

        if let Some(container) = model.entity_container.as_ref() {
            self.apply_rules(|rule| rule.visit_entity_container(container, ctx));
        }
    }
}

fn report_duplicate_names<I>(
    names: I,
    parent_kind: &'static str,
    parent: String,
    child_kind: &'static str,
    ctx: &mut ValidationContext,
) where
    I: IntoIterator<Item = String>,
{
    let mut seen = HashSet::new();
    let mut reported = HashSet::new();

    for name in names {
        if !seen.insert(name.clone()) && reported.insert(name.clone()) {
            ctx.push(ValidationError::DuplicateChildName {
                parent_kind,
                parent: parent.clone(),
                child_kind,
                child: name,
            });
        }
    }
}

fn schema_element_name(element: &SchemaElement) -> &str {
    match element {
        SchemaElement::EntityType(e) => &e.name,
        SchemaElement::ComplexType(c) => &c.name,
        SchemaElement::EnumType(en) => &en.name,
        SchemaElement::TypeDefinition(td) => &td.name,
        SchemaElement::Term(t) => &t.name,
        SchemaElement::Function(f) => &f.name,
        SchemaElement::Action(a) => &a.name,
    }
}

struct InheritanceSemanticsRule;

impl ValidationRule for InheritanceSemanticsRule {
    fn name(&self) -> &'static str {
        "inheritance_semantics"
    }

    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        let Some(base) = entity
            .base_type
            .get()
            .and_then(Option::as_ref)
            .and_then(|b| b.upgrade())
        else {
            return;
        };
        for (invalid, reason) in [
            (
                entity.is_abstract && !base.is_abstract,
                "Abstract entity type cannot derive from a concrete entity type",
            ),
            (
                base.is_open && !entity.is_open,
                "Derived type of an open type must be open",
            ),
            (
                entity.has_stream != base.has_stream,
                "Derived entity type must preserve HasStream",
            ),
        ] {
            if invalid {
                ctx.push(ValidationError::InvalidInheritance {
                    type_kind: "EntityType",
                    type_name: entity.name.clone(),
                    reason,
                });
            }
        }
    }

    fn visit_complex_type(&self, complex: &ComplexType, ctx: &mut ValidationContext) {
        let Some(base) = complex
            .base_type
            .get()
            .and_then(Option::as_ref)
            .and_then(|b| b.upgrade())
        else {
            return;
        };
        if base.is_open && !complex.is_open {
            ctx.push(ValidationError::InvalidInheritance {
                type_kind: "ComplexType",
                type_name: complex.name.clone(),
                reason: "Derived type of an open type must be open",
            });
        }
    }
}

struct UniqueSchemaChildNamesRule;

impl ValidationRule for UniqueSchemaChildNamesRule {
    fn name(&self) -> &'static str {
        "unique_schema_child_names"
    }

    fn visit_model(&self, model: &Model, ctx: &mut ValidationContext) {
        let names = model
            .elements
            .iter()
            .map(|element| schema_element_name(element.as_ref()).to_owned());

        report_duplicate_names(
            names,
            "Schema",
            model.namespace.clone(),
            "SchemaElement",
            ctx,
        );
    }
}

struct UniqueEntityChildNamesRule;

impl ValidationRule for UniqueEntityChildNamesRule {
    fn name(&self) -> &'static str {
        "unique_entity_child_names"
    }

    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        let property_names = entity.properties().iter().map(|p| p.name.clone());
        let navigation_names = entity
            .navigation_properties()
            .iter()
            .map(|n| n.name.clone());

        report_duplicate_names(
            property_names.chain(navigation_names),
            "EntityType",
            entity.name.clone(),
            "PropertyOrNavigationProperty",
            ctx,
        );
    }
}

struct UniqueComplexChildNamesRule;

impl ValidationRule for UniqueComplexChildNamesRule {
    fn name(&self) -> &'static str {
        "unique_complex_child_names"
    }

    fn visit_complex_type(&self, complex: &ComplexType, ctx: &mut ValidationContext) {
        let property_names = complex.properties().iter().map(|p| p.name.clone());
        let navigation_names = complex
            .navigation_properties()
            .iter()
            .map(|n| n.name.clone());

        report_duplicate_names(
            property_names.chain(navigation_names),
            "ComplexType",
            complex.name.clone(),
            "PropertyOrNavigationProperty",
            ctx,
        );
    }
}

struct UniqueEnumMemberNamesRule;

impl ValidationRule for UniqueEnumMemberNamesRule {
    fn name(&self) -> &'static str {
        "unique_enum_member_names"
    }

    fn visit_enum_type(&self, enum_type: &EnumType, ctx: &mut ValidationContext) {
        let member_names = enum_type.members.iter().map(|m| m.name.clone());
        report_duplicate_names(
            member_names,
            "EnumType",
            enum_type.name.clone(),
            "Member",
            ctx,
        );
    }
}

struct UniqueEntityContainerChildNamesRule;

impl ValidationRule for UniqueEntityContainerChildNamesRule {
    fn name(&self) -> &'static str {
        "unique_entity_container_child_names"
    }

    fn visit_entity_container(&self, container: &EntityContainer, ctx: &mut ValidationContext) {
        let child_names = container.elements.iter().map(|el| match el.as_ref() {
            EntityContainerElement::EntitySet(es) => es.name.clone(),
            EntityContainerElement::Singleton(s) => s.name.clone(),
            EntityContainerElement::FunctionImport(i) => i.name.clone(),
            EntityContainerElement::ActionImport(i) => i.name.clone(),
        });

        report_duplicate_names(
            child_names,
            "EntityContainer",
            container.name.clone(),
            "EntityContainerElement",
            ctx,
        );
    }
}

struct DuplicateEntityKeyRule;

struct KnownEntityContainerTargetsRule;
struct NavigationPropertyBindingSemanticsRule;
struct NavigationPartnerConsistencyRule;
struct ReferentialConstraintConsistencyRule;
struct TermBaseTermCycleRule;
struct BoundOperationBindingParameterRule;
struct OperationEntitySetPathRule;

enum BindingTargetStart {
    EntitySet,
    Singleton,
}

impl ValidationRule for NavigationPropertyBindingSemanticsRule {
    fn name(&self) -> &'static str {
        "navigation_property_binding_semantics"
    }

    fn visit_model(&self, model: &Model, ctx: &mut ValidationContext) {
        let Some(container) = model.entity_container.as_ref() else {
            return;
        };

        for element in &container.elements {
            match element.as_ref() {
                EntityContainerElement::EntitySet(set) => {
                    for binding in set.navigation_property_bindings() {
                        validate_navigation_binding_path(
                            container.name.as_str(),
                            "EntitySet.NavigationPropertyBinding",
                            set.name.as_str(),
                            binding,
                            ctx,
                        );

                        validate_navigation_binding_target(
                            container,
                            "EntitySet.NavigationPropertyBinding",
                            set.name.as_str(),
                            binding,
                            ctx,
                        );
                    }
                }
                EntityContainerElement::Singleton(singleton) => {
                    for binding in singleton.navigation_property_bindings() {
                        validate_navigation_binding_path(
                            container.name.as_str(),
                            "Singleton.NavigationPropertyBinding",
                            singleton.name.as_str(),
                            binding,
                            ctx,
                        );

                        validate_navigation_binding_target(
                            container,
                            "Singleton.NavigationPropertyBinding",
                            singleton.name.as_str(),
                            binding,
                            ctx,
                        );
                    }
                }
                EntityContainerElement::FunctionImport(_)
                | EntityContainerElement::ActionImport(_) => {}
            }
        }
    }
}

impl ValidationRule for KnownEntityContainerTargetsRule {
    fn name(&self) -> &'static str {
        "known_entity_container_targets"
    }

    fn visit_entity_container(&self, container: &EntityContainer, ctx: &mut ValidationContext) {
        for element in &container.elements {
            match element.as_ref() {
                EntityContainerElement::EntitySet(set) => {
                    for binding in set.navigation_property_bindings() {
                        if let Some(target) = unresolved_binding_target(&binding.target) {
                            ctx.push(ValidationError::UnknownContainerTarget {
                                container: container.name.clone(),
                                source_kind: "EntitySet.NavigationPropertyBinding",
                                source: set.name.clone(),
                                target: target.to_owned(),
                            });
                        }
                    }
                }
                EntityContainerElement::Singleton(singleton) => {
                    for binding in singleton.navigation_property_bindings() {
                        if let Some(target) = unresolved_binding_target(&binding.target) {
                            ctx.push(ValidationError::UnknownContainerTarget {
                                container: container.name.clone(),
                                source_kind: "Singleton.NavigationPropertyBinding",
                                source: singleton.name.clone(),
                                target: target.to_owned(),
                            });
                        }
                    }
                }
                EntityContainerElement::FunctionImport(import_) => {
                    if let Some(target) = &import_.entity_set
                        && let Some(unresolved) = unresolved_binding_target(target)
                    {
                        ctx.push(ValidationError::UnknownContainerTarget {
                            container: container.name.clone(),
                            source_kind: "FunctionImport.EntitySet",
                            source: import_.name.clone(),
                            target: unresolved.to_owned(),
                        });
                    }
                }
                EntityContainerElement::ActionImport(import_) => {
                    if let Some(target) = &import_.entity_set
                        && let Some(unresolved) = unresolved_binding_target(target)
                    {
                        ctx.push(ValidationError::UnknownContainerTarget {
                            container: container.name.clone(),
                            source_kind: "ActionImport.EntitySet",
                            source: import_.name.clone(),
                            target: unresolved.to_owned(),
                        });
                    }
                }
            }
        }
    }
}

impl ValidationRule for NavigationPartnerConsistencyRule {
    fn name(&self) -> &'static str {
        "navigation_partner_consistency"
    }

    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        for navigation in entity.navigation_properties() {
            let Some(partner) = navigation.partner() else {
                continue;
            };

            let Some(target) = navigation.target.upgrade() else {
                continue;
            };

            // A valid partner resolves against the target type and terminates at
            // a navigation property, with no unresolved segments.
            let resolved_to_partner_navigation = !partner
                .iter()
                .any(|segment| matches!(segment, BindingPathSegment::Unresolved(_)))
                && matches!(
                    partner.last(),
                    Some(BindingPathSegment::NavigationProperty(_))
                );

            if !resolved_to_partner_navigation {
                ctx.push(ValidationError::UnknownNavigationPartner {
                    entity: entity.name.clone(),
                    navigation: navigation.name.clone(),
                    partner: binding_path_to_string(partner),
                    target_entity: target.name.clone(),
                });
            }
        }
    }
}

impl ValidationRule for ReferentialConstraintConsistencyRule {
    fn name(&self) -> &'static str {
        "referential_constraint_consistency"
    }

    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        for navigation in entity.navigation_properties() {
            let Some(target) = navigation.target.upgrade() else {
                continue;
            };

            for constraint in &navigation.referential_constraints {
                if !entity
                    .properties()
                    .iter()
                    .any(|property| property.name == constraint.property)
                {
                    ctx.push(ValidationError::UnknownReferentialConstraintProperty {
                        entity: entity.name.clone(),
                        navigation: navigation.name.clone(),
                        property: constraint.property.clone(),
                    });
                }

                if !target
                    .properties()
                    .iter()
                    .any(|property| property.name == constraint.referenced_property)
                {
                    ctx.push(
                        ValidationError::UnknownReferentialConstraintReferencedProperty {
                            entity: entity.name.clone(),
                            navigation: navigation.name.clone(),
                            target_entity: target.name.clone(),
                            referenced_property: constraint.referenced_property.clone(),
                        },
                    );
                }
            }
        }
    }
}

impl ValidationRule for TermBaseTermCycleRule {
    fn name(&self) -> &'static str {
        "term_base_term_cycle"
    }

    fn visit_term(&self, term: &Term, ctx: &mut ValidationContext) {
        let Some(start_base) = term.base_term.get().and_then(|base| base.as_ref()) else {
            return;
        };

        let mut seen = HashSet::new();
        seen.insert(term.name.clone());

        let mut current = start_base.upgrade();
        while let Some(current_term) = current {
            if !seen.insert(current_term.name.clone()) {
                ctx.push(ValidationError::CyclicTermBaseTerm {
                    term: term.name.clone(),
                });
                return;
            }

            current = current_term
                .base_term
                .get()
                .and_then(|base| base.as_ref())
                .and_then(|base| base.upgrade());
        }
    }
}

impl ValidationRule for BoundOperationBindingParameterRule {
    fn name(&self) -> &'static str {
        "bound_operation_binding_parameter"
    }

    fn visit_function(&self, function: &Function, ctx: &mut ValidationContext) {
        if function.is_bound && function.parameters.is_empty() {
            ctx.push(ValidationError::BoundOperationMissingBindingParameter {
                operation_kind: "Function",
                operation: function.name.clone(),
            });
        }
    }

    fn visit_action(&self, action: &Action, ctx: &mut ValidationContext) {
        if action.is_bound && action.parameters.is_empty() {
            ctx.push(ValidationError::BoundOperationMissingBindingParameter {
                operation_kind: "Action",
                operation: action.name.clone(),
            });
        }
    }
}

impl ValidationRule for OperationEntitySetPathRule {
    fn name(&self) -> &'static str {
        "operation_entity_set_path"
    }

    fn visit_function(&self, function: &Function, ctx: &mut ValidationContext) {
        validate_entity_set_path(
            "Function",
            &function.name,
            function.is_bound,
            &function.parameters,
            function.entity_set_path.as_deref(),
            ctx,
        );
    }

    fn visit_action(&self, action: &Action, ctx: &mut ValidationContext) {
        validate_entity_set_path(
            "Action",
            &action.name,
            action.is_bound,
            &action.parameters,
            action.entity_set_path.as_deref(),
            ctx,
        );
    }
}

impl ValidationRule for DuplicateEntityKeyRule {
    fn name(&self) -> &'static str {
        "duplicate_entity_key"
    }

    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        let mut seen = HashSet::new();
        for key in entity.keys() {
            let key_string = key_path_to_string(key);
            if !seen.insert(key_string.clone()) {
                ctx.push(ValidationError::DuplicateKey {
                    entity: entity.name.clone(),
                    key: key_string,
                });
            }
        }
    }
}

struct UnknownEntityKeyPropertyRule;

impl ValidationRule for UnknownEntityKeyPropertyRule {
    fn name(&self) -> &'static str {
        "unknown_entity_key_property"
    }

    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        for key in entity.keys() {
            if classify_key_path(key) == KeyPathClassification::Unknown {
                ctx.push(ValidationError::UnknownKeyProperty {
                    entity: entity.name.clone(),
                    key: key_path_to_string(key),
                });
            }
        }
    }
}

struct NonScalarEntityKeyRule;

impl ValidationRule for NonScalarEntityKeyRule {
    fn name(&self) -> &'static str {
        "non_scalar_entity_key"
    }

    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        for key in entity.keys() {
            let classification = classify_key_path(key);
            if matches!(
                classification,
                KeyPathClassification::Complex | KeyPathClassification::Collection
            ) {
                ctx.push(ValidationError::NonScalarKeyProperty {
                    entity: entity.name.clone(),
                    key: key_path_to_string(key),
                });
            }
        }
    }
}

fn classify_key_path(key: &[KeyPathSegment]) -> KeyPathClassification {
    let mut terminal = None;

    for segment in key {
        let KeyPathSegment::Property(property) = segment else {
            return KeyPathClassification::Unknown;
        };
        let Some(property) = property.upgrade() else {
            return KeyPathClassification::Unknown;
        };
        if property.is_collection {
            return KeyPathClassification::Collection;
        }
        terminal = Some(property);
    }

    match terminal {
        None => KeyPathClassification::Unknown,
        Some(property) => match property.ty {
            ResolvedType::Complex(_) => KeyPathClassification::Complex,
            ResolvedType::Primitive(_)
            | ResolvedType::Enum(_)
            | ResolvedType::TypeDefinition(_) => KeyPathClassification::Scalar,
        },
    }
}

fn validate_entity_set_path(
    operation_kind: &'static str,
    operation_name: &str,
    is_bound: bool,
    parameters: &[crate::edm::OperationParameter],
    entity_set_path: Option<&[EntitySetPathSegment]>,
    ctx: &mut ValidationContext,
) {
    let Some(path) = entity_set_path else {
        return;
    };
    let path_string = || entity_set_path_to_string(path);

    if !is_bound {
        ctx.push(ValidationError::InvalidEntitySetPath {
            operation_kind,
            operation: operation_name.to_owned(),
            path: path_string(),
            reason: "EntitySetPath requires a bound operation",
        });
        return;
    }

    if parameters.is_empty() {
        ctx.push(ValidationError::InvalidEntitySetPath {
            operation_kind,
            operation: operation_name.to_owned(),
            path: path_string(),
            reason: "EntitySetPath requires a binding parameter",
        });
        return;
    }

    let binding_parameter_name = parameters[0].name.as_str();
    let head_matches = matches!(
        path.first(),
        Some(EntitySetPathSegment::BindingParameter(name)) if name == binding_parameter_name
    );
    if !head_matches {
        ctx.push(ValidationError::InvalidEntitySetPath {
            operation_kind,
            operation: operation_name.to_owned(),
            path: path_string(),
            reason: "EntitySetPath must start with the binding parameter name",
        });
        return;
    }

    // Full resolution: any tail segment that failed to resolve is an invalid path.
    if path
        .iter()
        .any(|segment| matches!(segment, EntitySetPathSegment::Unresolved(_)))
    {
        ctx.push(ValidationError::InvalidEntitySetPath {
            operation_kind,
            operation: operation_name.to_owned(),
            path: path_string(),
            reason: "EntitySetPath contains an unresolved segment",
        });
    }
}

fn validate_navigation_binding_path(
    container_name: &str,
    source_kind: &'static str,
    source_name: &str,
    binding: &NavigationPropertyBinding,
    ctx: &mut ValidationContext,
) {
    let path = binding.path.as_ref();

    if path.is_empty() {
        ctx.push(ValidationError::InvalidNavigationPropertyBinding {
            container: container_name.to_owned(),
            source_kind,
            source: source_name.to_owned(),
            attribute: "Path",
            value: binding.path_string(),
            reason: "Path must not be empty",
        });
        return;
    }

    let mut effective_segments = path.iter().collect::<Vec<_>>();
    let has_terminal_type_cast = effective_segments
        .last()
        .map(|segment| {
            matches!(
                segment,
                BindingPathSegment::EntityTypeCast(_) | BindingPathSegment::ComplexTypeCast(_)
            )
        })
        .unwrap_or(false);
    if has_terminal_type_cast {
        if effective_segments.len() == 1 {
            ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                container: container_name.to_owned(),
                source_kind,
                source: source_name.to_owned(),
                attribute: "Path",
                value: binding.path_string(),
                reason: "Terminal type-cast requires a preceding navigation segment",
            });
            return;
        }
        effective_segments.pop();
    }

    let mut saw_final_non_containment_navigation = false;

    for (index, segment) in effective_segments.iter().enumerate() {
        let is_last = index + 1 == effective_segments.len();

        match segment {
            // Type-cast segments are allowed, but detailed cast compatibility
            // requires inheritance metadata that is currently not tracked here.
            BindingPathSegment::EntityTypeCast(_) | BindingPathSegment::ComplexTypeCast(_) => {
                continue;
            }
            BindingPathSegment::NavigationProperty(navigation) => {
                let Some(navigation) = navigation.upgrade() else {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container_name.to_owned(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Path",
                        value: binding.path_string(),
                        reason: "Path segment does not resolve to a property or navigation property",
                    });
                    return;
                };

                let contains_target = navigation.contains_target.unwrap_or(false);
                if is_last {
                    if contains_target {
                        ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                            container: container_name.to_owned(),
                            source_kind,
                            source: source_name.to_owned(),
                            attribute: "Path",
                            value: binding.path_string(),
                            reason: "Final navigation segment in Path must be non-containment",
                        });
                        return;
                    }
                    saw_final_non_containment_navigation = true;
                } else if !contains_target {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container_name.to_owned(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Path",
                        value: binding.path_string(),
                        reason: "Only containment navigation segments are allowed before the final segment",
                    });
                    return;
                }

                let Some(_target_entity) = navigation.target.upgrade() else {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container_name.to_owned(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Path",
                        value: binding.path_string(),
                        reason: "Navigation segment target cannot be resolved",
                    });
                    return;
                };
            }
            BindingPathSegment::Property(property) => {
                let Some(property) = property.upgrade() else {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container_name.to_owned(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Path",
                        value: binding.path_string(),
                        reason: "Path segment does not resolve to a property or navigation property",
                    });
                    return;
                };

                let ResolvedType::Complex(_complex) = &property.ty else {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container_name.to_owned(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Path",
                        value: binding.path_string(),
                        reason: "Non-navigation path segments must resolve to complex properties",
                    });
                    return;
                };
            }
            BindingPathSegment::Unresolved(_)
            | BindingPathSegment::EntitySet(_)
            | BindingPathSegment::Singleton(_)
            | BindingPathSegment::EntityContainer(_) => {
                ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                    container: container_name.to_owned(),
                    source_kind,
                    source: source_name.to_owned(),
                    attribute: "Path",
                    value: binding.path_string(),
                    reason: "Path segment does not resolve to a property or navigation property",
                });
                return;
            }
        }
    }

    if !saw_final_non_containment_navigation {
        ctx.push(ValidationError::InvalidNavigationPropertyBinding {
            container: container_name.to_owned(),
            source_kind,
            source: source_name.to_owned(),
            attribute: "Path",
            value: binding.path_string(),
            reason: "Path must end in a navigation property segment",
        });
    }
}

fn validate_navigation_binding_target(
    container: &EntityContainer,
    source_kind: &'static str,
    source_name: &str,
    binding: &NavigationPropertyBinding,
    ctx: &mut ValidationContext,
) {
    let target = binding.target.as_ref();

    if target.is_empty() {
        ctx.push(ValidationError::InvalidNavigationPropertyBinding {
            container: container.name.clone(),
            source_kind,
            source: source_name.to_owned(),
            attribute: "Target",
            value: binding.target_string(),
            reason: "Target must not be empty",
        });
        return;
    }

    let start = match &target[0] {
        BindingPathSegment::EntitySet(_) => BindingTargetStart::EntitySet,
        BindingPathSegment::Singleton(singleton) => {
            let Some(_singleton) = singleton.upgrade() else {
                return;
            };
            BindingTargetStart::Singleton
        }
        BindingPathSegment::Unresolved(_) => {
            return;
        }
        BindingPathSegment::EntityContainer(_)
        | BindingPathSegment::NavigationProperty(_)
        | BindingPathSegment::Property(_)
        | BindingPathSegment::EntityTypeCast(_)
        | BindingPathSegment::ComplexTypeCast(_) => {
            ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                container: container.name.clone(),
                source_kind,
                source: source_name.to_owned(),
                attribute: "Target",
                value: binding.target_string(),
                reason: "Target must start with an entity set or singleton",
            });
            return;
        }
    };

    if target.len() == 1 {
        return;
    }

    let BindingTargetStart::Singleton = start else {
        ctx.push(ValidationError::InvalidNavigationPropertyBinding {
            container: container.name.clone(),
            source_kind,
            source: source_name.to_owned(),
            attribute: "Target",
            value: binding.target_string(),
            reason: "Target paths with additional segments must start from a singleton",
        });
        return;
    };

    for (index, segment) in target.iter().enumerate().skip(1) {
        let is_last = index + 1 == target.len();

        match segment {
            BindingPathSegment::NavigationProperty(navigation) => {
                let Some(navigation) = navigation.upgrade() else {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container.name.clone(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Target",
                        value: binding.target_string(),
                        reason: "Target path segment does not resolve to a complex property or containment navigation property",
                    });
                    return;
                };

                let contains_target = navigation.contains_target.unwrap_or(false);
                if !contains_target {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container.name.clone(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Target",
                        value: binding.target_string(),
                        reason: "Target path navigation segments must be containment navigation properties",
                    });
                    return;
                }

                if !is_last && navigation.is_collection {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container.name.clone(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Target",
                        value: binding.target_string(),
                        reason: "Intermediate Target path segments must be single-valued",
                    });
                    return;
                }

                let Some(_target_entity) = navigation.target.upgrade() else {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container.name.clone(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Target",
                        value: binding.target_string(),
                        reason: "Target path navigation segment target cannot be resolved",
                    });
                    return;
                };
            }
            BindingPathSegment::Property(property) => {
                let Some(property) = property.upgrade() else {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container.name.clone(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Target",
                        value: binding.target_string(),
                        reason: "Target path segment does not resolve to a complex property or containment navigation property",
                    });
                    return;
                };

                if property.is_collection {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container.name.clone(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Target",
                        value: binding.target_string(),
                        reason: "Target path property segments must be single-valued complex properties",
                    });
                    return;
                }

                let ResolvedType::Complex(_complex) = &property.ty else {
                    ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                        container: container.name.clone(),
                        source_kind,
                        source: source_name.to_owned(),
                        attribute: "Target",
                        value: binding.target_string(),
                        reason: "Target path property segments must be complex properties",
                    });
                    return;
                };
            }
            BindingPathSegment::Unresolved(_)
            | BindingPathSegment::EntitySet(_)
            | BindingPathSegment::Singleton(_)
            | BindingPathSegment::EntityContainer(_)
            | BindingPathSegment::EntityTypeCast(_)
            | BindingPathSegment::ComplexTypeCast(_) => {
                ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                    container: container.name.clone(),
                    source_kind,
                    source: source_name.to_owned(),
                    attribute: "Target",
                    value: binding.target_string(),
                    reason: "Target path segment does not resolve to a complex property or containment navigation property",
                });
                return;
            }
        }
    }
}

/// Returns the authored name of the first unresolved segment in a binding's
/// resolved target path, if any. A resolved target that contains a
/// [`BindingPathSegment::Unresolved`] segment is a target the resolver could not bind
/// to a known entity set / singleton.
fn unresolved_binding_target(target: &[BindingPathSegment]) -> Option<&str> {
    target.iter().find_map(|segment| match segment {
        BindingPathSegment::Unresolved(name) => Some(name.as_str()),
        _ => None,
    })
}
