//! Semantic validation that is intentionally separate from reference resolution.

mod annotations;
mod expressions;
mod types;
mod values;
use crate::edm::ConstraintPathSegment;
pub use annotations::{
    AnnotationDiagnostic, AnnotationIssue, validate_annotations, validate_model_annotations,
};

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
    InvalidServiceDocument {
        reason: &'static str,
    },
    InvalidDefaultValue {
        element: String,
        value: String,
    },
    InvalidTypeUse {
        element: String,
        reason: &'static str,
    },
    InvalidEnum {
        name: String,
        reason: &'static str,
    },
    InvalidFacet {
        element: String,
        facet: &'static str,
        reason: &'static str,
    },
    InvalidKey {
        entity: String,
        key: String,
        reason: &'static str,
    },
    InvalidImport {
        container: String,
        import: String,
        reason: &'static str,
    },
    InvalidOperation {
        operation: String,
        reason: &'static str,
    },
    InvalidContainmentProperty {
        owner: String,
        property: String,
    },
    InvalidNavigation {
        owner: String,
        navigation: String,
        reason: &'static str,
    },
    MissingEntityKey {
        entity: String,
    },
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

/// Both profiles expose annotation diagnostics; only strict authoring treats
/// them as invalidating the report.
#[derive(Debug)]
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
    pub annotations: Vec<AnnotationDiagnostic>,
    pub profile: crate::conformance::ValidationProfile,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
            && (self.profile == crate::conformance::ValidationProfile::Tolerant
                || self.annotations.is_empty())
    }
}

pub fn validate_document_with_profile(
    document: &DocumentModel,
    profile: crate::conformance::ValidationProfile,
) -> ValidationReport {
    ValidationReport {
        errors: validate_document(document).err().unwrap_or_default(),
        annotations: validate_annotations(document),
        profile,
    }
}

/// Service metadata requires a declared service identity and exactly one
/// locally declared container. Vocabulary/model documents have no such rule.
pub fn validate_document_as(
    document: &DocumentModel,
    kind: crate::conformance::DocumentKind,
    profile: crate::conformance::ValidationProfile,
) -> ValidationReport {
    let mut report = validate_document_with_profile(document, profile);
    if kind == crate::conformance::DocumentKind::ServiceMetadata {
        if document.service_container.is_none() {
            report.errors.push(ValidationError::InvalidServiceDocument {
                reason: "Service metadata requires a service container",
            });
        }
        if document
            .schemas
            .iter()
            .map(|s| s.containers().len())
            .sum::<usize>()
            != 1
        {
            report.errors.push(ValidationError::InvalidServiceDocument {
                reason: "Service metadata must declare exactly one entity container",
            });
        }
    }
    report
}

pub fn validate_document(document: &DocumentModel) -> Result<(), Vec<ValidationError>> {
    let engine = ValidatorEngine::default();
    let mut errors = Vec::new();

    for schema in document.all_schemas() {
        let version = document.schema_version(&schema.namespace);
        errors.extend(engine.validate_model_collect(schema.as_ref(), version));
        if version == "4.0" {
            for element in &schema.elements {
                let navigations = match element.as_ref() {
                    SchemaElement::EntityType(t) => t.navigation_properties(),
                    SchemaElement::ComplexType(t) => t.navigation_properties(),
                    _ => continue,
                };
                for nav in navigations {
                    for constraint in &nav.referential_constraints {
                        let invalid = [
                            &constraint.property_path,
                            &constraint.referenced_property_path,
                        ]
                        .iter()
                        .any(|p| {
                            p.get().is_some_and(|p| {
                                p.iter().any(|segment| match segment {
                                    ConstraintPathSegment::NavigationProperty(_) => true,
                                    ConstraintPathSegment::Property(p) => p
                                        .upgrade()
                                        .is_some_and(|p| matches!(p.ty, ResolvedType::Complex(_))),
                                    _ => false,
                                })
                            })
                        });
                        if invalid {
                            errors.push(ValidationError::InvalidTypeUse { element: nav.name.clone(), reason: "Complex and navigation referential constraints require CSDL 4.01" });
                        }
                    }
                }
            }
            for element in &schema.elements {
                if let SchemaElement::EntityType(entity) = element.as_ref() {
                    for key in entity.keys() {
                        if key
                            .iter()
                            .any(|s| matches!(s, KeyPathSegment::NavigationProperty(_)))
                        {
                            errors.push(ValidationError::InvalidKey {
                                entity: entity.name.clone(),
                                key: key_path_to_string(key),
                                reason: "Navigation key paths require CSDL 4.01",
                            });
                        }
                    }
                }
            }
            for container in schema.containers() {
                for element in &container.elements {
                    let (kind, name, bindings) = match element.as_ref() {
                        EntityContainerElement::EntitySet(set) => (
                            "EntitySet.NavigationPropertyBinding",
                            &set.name,
                            set.navigation_property_bindings(),
                        ),
                        EntityContainerElement::Singleton(singleton) => (
                            "Singleton.NavigationPropertyBinding",
                            &singleton.name,
                            singleton.navigation_property_bindings(),
                        ),
                        _ => continue,
                    };
                    for binding in bindings {
                        if matches!(
                            binding.path.last(),
                            Some(
                                BindingPathSegment::EntityTypeCast(_)
                                    | BindingPathSegment::ComplexTypeCast(_)
                            )
                        ) {
                            errors.push(ValidationError::InvalidNavigationPropertyBinding {
                                container: container.name.clone(),
                                source_kind: kind,
                                source: name.clone(),
                                attribute: "Path",
                                value: binding.path_string(),
                                reason: "Terminal binding type casts require CSDL 4.01",
                            });
                        }
                    }
                }
            }
            for element in &schema.elements {
                if let SchemaElement::EntityType(entity) = element.as_ref() {
                    if !entity.is_abstract && entity.keys().is_empty() {
                        errors.push(ValidationError::MissingEntityKey {
                            entity: entity.name.clone(),
                        });
                    }
                }
                let (owner, properties) = match element.as_ref() {
                    SchemaElement::EntityType(entity) => (&entity.name, entity.properties()),
                    SchemaElement::ComplexType(complex) => (&complex.name, complex.properties()),
                    _ => continue,
                };
                for property in properties {
                    if property.is_collection {
                        if let ResolvedType::Complex(complex) = &property.ty {
                            if complex_contains_navigation(complex, &mut HashSet::new()) {
                                errors.push(ValidationError::InvalidContainmentProperty {
                                    owner: owner.clone(),
                                    property: property.name.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn complex_contains_navigation(
    complex: &ComplexType,
    seen: &mut HashSet<*const ComplexType>,
) -> bool {
    if !seen.insert(complex as *const ComplexType) {
        return false;
    }
    complex
        .navigation_properties()
        .iter()
        .any(|nav| nav.contains_target.unwrap_or(false))
        || complex
            .properties()
            .iter()
            .any(|property| match &property.ty {
                ResolvedType::Complex(nested) => complex_contains_navigation(nested, seen),
                _ => false,
            })
}

pub fn validate_model(model: &Model) -> Result<(), Vec<ValidationError>> {
    let engine = ValidatorEngine::default();
    let errors = engine.validate_model_collect(model, "4.01");

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
                Box::new(ContainerSemanticsRule),
                Box::new(NavigationPartnerConsistencyRule),
                Box::new(NavigationSemanticsRule),
                Box::new(ReferentialConstraintConsistencyRule),
                Box::new(TermBaseTermCycleRule),
                Box::new(BoundOperationBindingParameterRule),
                Box::new(OperationSemanticsRule),
                Box::new(OperationEntitySetPathRule),
                Box::new(DuplicateEntityKeyRule),
                Box::new(UnknownEntityKeyPropertyRule),
                Box::new(NonScalarEntityKeyRule),
                Box::new(KeySemanticsRule),
            ],
        }
    }
}

impl ValidatorEngine {
    fn validate_model_collect(&self, model: &Model, version: &str) -> Vec<ValidationError> {
        let mut ctx = ValidationContext::default();
        self.walk_model(model, &mut ctx);
        ctx.errors.extend(types::validate(model, version));
        ctx.errors.extend(values::validate(model));
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

        for container in model.containers() {
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
        let mut seen = std::collections::HashMap::new();
        for container in model.containers() {
            seen.insert(container.name.as_str(), 0);
        }
        for element in &model.elements {
            let name = schema_element_name(element);
            let kind = match element.as_ref() {
                SchemaElement::Function(_) => 1,
                SchemaElement::Action(_) => 2,
                _ => 0,
            };
            if let Some(previous) = seen.insert(name, kind) {
                if kind == 0 || previous != kind {
                    ctx.push(ValidationError::DuplicateChildName {
                        parent_kind: "Schema",
                        parent: model.namespace.clone(),
                        child_kind: "SchemaElement",
                        child: name.into(),
                    });
                }
            }
        }
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
        for container in model.containers() {
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
}

struct ContainerSemanticsRule;

fn binding_segment_identity(segment: &BindingPathSegment) -> (u8, usize, String) {
    match segment {
        BindingPathSegment::EntitySet(v) => (0, v.as_ptr() as usize, String::new()),
        BindingPathSegment::Singleton(v) => (1, v.as_ptr() as usize, String::new()),
        BindingPathSegment::EntityContainer(v) => (2, v.as_ptr() as usize, String::new()),
        BindingPathSegment::EntityTypeCast(v) => (3, v.as_ptr() as usize, String::new()),
        BindingPathSegment::ComplexTypeCast(v) => (4, v.as_ptr() as usize, String::new()),
        BindingPathSegment::Property(v) => (5, v.as_ptr() as usize, String::new()),
        BindingPathSegment::NavigationProperty(v) => (6, v.as_ptr() as usize, String::new()),
        BindingPathSegment::Unresolved(v) => (7, 0, v.clone()),
    }
}

fn binding_terminal_entity(path: &[BindingPathSegment]) -> Option<std::sync::Arc<EntityType>> {
    if path
        .iter()
        .any(|p| matches!(p, BindingPathSegment::Unresolved(_)))
    {
        return None;
    }
    match path.last()? {
        BindingPathSegment::EntitySet(set) => set.upgrade().map(|s| s.target.clone()),
        BindingPathSegment::Singleton(singleton) => singleton.upgrade().map(|s| s.target.clone()),
        BindingPathSegment::NavigationProperty(nav) => {
            nav.upgrade().and_then(|n| n.target.upgrade())
        }
        BindingPathSegment::EntityTypeCast(ty) => ty.upgrade(),
        _ => None,
    }
}

fn validate_binding_set(
    container: &str,
    source_kind: &'static str,
    source: &str,
    bindings: &[NavigationPropertyBinding],
    ctx: &mut ValidationContext,
) {
    let mut seen = HashSet::new();
    for binding in bindings {
        let signature = binding
            .path
            .iter()
            .map(binding_segment_identity)
            .collect::<Vec<_>>();
        let reason = if !seen.insert(signature) {
            Some("Binding paths must be unique within a source")
        } else if let (Some(source), Some(target)) = (
            binding_terminal_entity(&binding.path),
            binding_terminal_entity(&binding.target),
        ) {
            (!entity_derives_from(&source, &target) && !entity_derives_from(&target, &source))
                .then_some("Binding target type is incompatible with the navigation type")
        } else {
            None
        };
        if let Some(reason) = reason {
            ctx.push(ValidationError::InvalidNavigationPropertyBinding {
                container: container.into(),
                source_kind,
                source: source.into(),
                attribute: "Path",
                value: binding.path_string(),
                reason,
            });
        }
    }
}

fn validate_import_return(
    container: &str,
    name: &str,
    target: Option<&[BindingPathSegment]>,
    return_type: Option<&crate::edm::OperationReturnType>,
    ctx: &mut ValidationContext,
) {
    let Some(target) = target else {
        return;
    };
    let reason = if !matches!(
        return_type.map(|r| &r.ty),
        Some(crate::edm::TermType::Entity(_))
    ) {
        Some("Import EntitySet requires an entity-valued operation return")
    } else if target
        .iter()
        .any(|s| matches!(s, BindingPathSegment::Unresolved(_)))
    {
        None // The existing unknown-target rule supplies the precise diagnostic.
    } else if !matches!(target.last(), Some(BindingPathSegment::EntitySet(_))) {
        Some("Import EntitySet must resolve to an entity set")
    } else {
        let returned = match &return_type.unwrap().ty {
            crate::edm::TermType::Entity(e) => e,
            _ => unreachable!(),
        };
        binding_terminal_entity(target).and_then(|target| {
            (!entity_derives_from(returned, &target) && !entity_derives_from(&target, returned))
                .then_some("Import EntitySet type is incompatible with the return type")
        })
    };
    if let Some(reason) = reason {
        ctx.push(ValidationError::InvalidImport {
            container: container.into(),
            import: name.into(),
            reason,
        });
    }
}

impl ValidationRule for ContainerSemanticsRule {
    fn name(&self) -> &'static str {
        "container_semantics"
    }
    fn visit_entity_container(&self, container: &EntityContainer, ctx: &mut ValidationContext) {
        for element in &container.elements {
            match element.as_ref() {
                EntityContainerElement::EntitySet(set) => {
                    if set.target.keys().is_empty() {
                        ctx.push(ValidationError::MissingEntityKey {
                            entity: set.target.name.clone(),
                        });
                    }
                    validate_binding_set(
                        &container.name,
                        "EntitySet.NavigationPropertyBinding",
                        &set.name,
                        set.navigation_property_bindings(),
                        ctx,
                    );
                }
                EntityContainerElement::Singleton(singleton) => {
                    validate_binding_set(
                        &container.name,
                        "Singleton.NavigationPropertyBinding",
                        &singleton.name,
                        singleton.navigation_property_bindings(),
                        ctx,
                    );
                }
                EntityContainerElement::FunctionImport(import) => {
                    if import.include_in_service_document
                        && !import.overloads.iter().any(|f| f.parameters.is_empty())
                    {
                        ctx.push(ValidationError::InvalidImport { container: container.name.clone(), import: import.name.clone(), reason: "Service-document function import needs a parameterless overload" });
                    }
                    for function in &import.overloads {
                        validate_import_return(
                            &container.name,
                            &import.name,
                            import.entity_set.as_deref(),
                            function.return_type.as_ref(),
                            ctx,
                        );
                    }
                }
                EntityContainerElement::ActionImport(import) => {
                    validate_import_return(
                        &container.name,
                        &import.name,
                        import.entity_set.as_deref(),
                        import.operation.return_type.as_ref(),
                        ctx,
                    );
                }
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

enum ConstraintEndpoint {
    Property(std::sync::Arc<crate::edm::Property>),
    Navigation(std::sync::Arc<crate::edm::NavigationProperty>),
}

fn terminal_constraint_property(
    path: Option<&std::sync::Arc<[crate::edm::ConstraintPathSegment]>>,
) -> Option<ConstraintEndpoint> {
    use crate::edm::ConstraintPathSegment;
    let path = path?;
    if path
        .iter()
        .any(|s| matches!(s, ConstraintPathSegment::Unresolved(_)))
    {
        return None;
    }
    match path.last()? {
        ConstraintPathSegment::Property(p) => p.upgrade().map(ConstraintEndpoint::Property),
        ConstraintPathSegment::NavigationProperty(n) => {
            n.upgrade().map(ConstraintEndpoint::Navigation)
        }
        _ => None,
    }
}

fn path_nullable(path: &[crate::edm::ConstraintPathSegment]) -> bool {
    use crate::edm::ConstraintPathSegment;
    path.iter().any(|segment| match segment {
        ConstraintPathSegment::Property(property) => property
            .upgrade()
            .is_none_or(|p| p.nullable.unwrap_or(true)),
        ConstraintPathSegment::NavigationProperty(nav) => {
            nav.upgrade().is_none_or(|n| n.nullable.unwrap_or(true))
        }
        _ => true,
    })
}

fn constraint_types_match(left: &ConstraintEndpoint, right: &ConstraintEndpoint) -> bool {
    use std::sync::Arc;
    match (left, right) {
        (ConstraintEndpoint::Property(left), ConstraintEndpoint::Property(right)) => {
            left.is_collection == right.is_collection
                && match (&left.ty, &right.ty) {
                    (ResolvedType::Primitive(a), ResolvedType::Primitive(b)) => a == b,
                    (ResolvedType::Builtin(a), ResolvedType::Builtin(b)) => a == b,
                    (ResolvedType::Enum(a), ResolvedType::Enum(b)) => Arc::ptr_eq(a, b),
                    (ResolvedType::TypeDefinition(a), ResolvedType::TypeDefinition(b)) => {
                        Arc::ptr_eq(a, b)
                    }
                    (ResolvedType::Complex(_), ResolvedType::Complex(_)) => true,
                    _ => false,
                }
        }
        (ConstraintEndpoint::Navigation(left), ConstraintEndpoint::Navigation(right)) => {
            left.is_collection == right.is_collection
                && match (left.target.upgrade(), right.target.upgrade()) {
                    (Some(left), Some(right)) => Arc::ptr_eq(&left, &right),
                    _ => false,
                }
        }
        _ => false,
    }
}

fn invalid_navigation(
    owner: &str,
    nav: &crate::edm::NavigationProperty,
    reason: &'static str,
    ctx: &mut ValidationContext,
) {
    ctx.push(ValidationError::InvalidNavigation {
        owner: owner.into(),
        navigation: nav.name.clone(),
        reason,
    });
}

fn validate_constraints(
    owner: &str,
    navs: &[std::sync::Arc<crate::edm::NavigationProperty>],
    ctx: &mut ValidationContext,
) {
    for nav in navs {
        let Some(target) = nav.target.upgrade() else {
            continue;
        };
        if nav.is_collection && !nav.referential_constraints.is_empty() {
            invalid_navigation(
                owner,
                nav,
                "Referential constraints require a single-valued navigation",
                ctx,
            );
        }
        let mut seen = HashSet::new();
        for constraint in &nav.referential_constraints {
            if !seen.insert(&constraint.property) {
                invalid_navigation(owner, nav, "Dependent constraint paths must be unique", ctx);
            }
            let dependent = terminal_constraint_property(constraint.property_path.get());
            let principal = terminal_constraint_property(constraint.referenced_property_path.get());
            if dependent.is_none() {
                ctx.push(ValidationError::UnknownReferentialConstraintProperty {
                    entity: owner.into(),
                    navigation: nav.name.clone(),
                    property: constraint.property.clone(),
                });
            }
            if principal.is_none() {
                ctx.push(
                    ValidationError::UnknownReferentialConstraintReferencedProperty {
                        entity: owner.into(),
                        navigation: nav.name.clone(),
                        target_entity: target.name.clone(),
                        referenced_property: constraint.referenced_property.clone(),
                    },
                );
            }
            if let (Some(dependent), Some(principal)) = (dependent, principal) {
                if !constraint_types_match(&dependent, &principal) {
                    invalid_navigation(
                        owner,
                        nav,
                        "Dependent and principal constraint types must match",
                        ctx,
                    );
                }
                let dependent_nullable = path_nullable(constraint.property_path.get().unwrap());
                let principal_nullable =
                    path_nullable(constraint.referenced_property_path.get().unwrap());
                if dependent_nullable != (nav.nullable.unwrap_or(true) || principal_nullable) {
                    invalid_navigation(
                        owner,
                        nav,
                        "Dependent nullability must equal navigation or principal nullability",
                        ctx,
                    );
                }
            }
        }
    }
}

impl ValidationRule for ReferentialConstraintConsistencyRule {
    fn name(&self) -> &'static str {
        "referential_constraint_consistency"
    }
    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        validate_constraints(&entity.name, entity.navigation_properties(), ctx);
    }
    fn visit_complex_type(&self, complex: &ComplexType, ctx: &mut ValidationContext) {
        validate_constraints(&complex.name, complex.navigation_properties(), ctx);
    }
}

struct NavigationSemanticsRule;

fn entity_derives_from(entity: &EntityType, base: &EntityType) -> bool {
    if std::ptr::eq(base, crate::edm::abstract_entity_type().as_ref()) {
        return true;
    }
    if std::ptr::eq(entity, base) {
        return true;
    }
    let mut next = entity
        .base_type
        .get()
        .and_then(Option::as_ref)
        .and_then(|b| b.upgrade());
    let mut seen = HashSet::new();
    while let Some(current) = next {
        if !seen.insert(std::sync::Arc::as_ptr(&current)) {
            break;
        }
        if std::ptr::eq(current.as_ref(), base) {
            return true;
        }
        next = current
            .base_type
            .get()
            .and_then(Option::as_ref)
            .and_then(|b| b.upgrade());
    }
    false
}

fn partner_navigation(
    nav: &crate::edm::NavigationProperty,
) -> Option<std::sync::Arc<crate::edm::NavigationProperty>> {
    let path = nav.partner()?;
    if path
        .iter()
        .any(|s| matches!(s, BindingPathSegment::Unresolved(_)))
    {
        return None;
    }
    match path.last()? {
        BindingPathSegment::NavigationProperty(partner) => partner.upgrade(),
        _ => None,
    }
}

fn validate_navigation_members(
    owner: &str,
    entity: Option<&EntityType>,
    navs: &[std::sync::Arc<crate::edm::NavigationProperty>],
    ctx: &mut ValidationContext,
) {
    let mut container_partners = 0;
    for nav in navs {
        if nav.is_collection && nav.nullable.is_some() {
            invalid_navigation(
                owner,
                nav,
                "Collection-valued navigation must not specify Nullable",
                ctx,
            );
        }
        let Some(target) = nav.target.upgrade() else {
            continue;
        };
        if nav.is_collection && nav.contains_target.unwrap_or(false) && target.keys().is_empty() {
            invalid_navigation(
                owner,
                nav,
                "Collection containment target requires a key",
                ctx,
            );
        }
        if entity.is_none() && nav.partner().is_some() {
            invalid_navigation(
                owner,
                nav,
                "Navigation declared on a complex type cannot specify Partner",
                ctx,
            );
        }
        let Some(partner) = partner_navigation(nav) else {
            continue;
        };
        if partner.contains_target.unwrap_or(false) {
            container_partners += 1;
        }
        if let Some(entity) = entity {
            if partner
                .target
                .upgrade()
                .is_none_or(|ty| !entity_derives_from(entity, &ty))
            {
                invalid_navigation(
                    owner,
                    nav,
                    "Partner must target the declaring entity type or a base type",
                    ctx,
                );
            }
            if partner.partner().is_some()
                && partner_navigation(&partner)
                    .is_none_or(|back| !std::sync::Arc::ptr_eq(&back, nav))
            {
                invalid_navigation(
                    owner,
                    nav,
                    "Partner must point back to the current navigation",
                    ctx,
                );
            }
            if nav.contains_target.unwrap_or(false) {
                let recursive =
                    entity_derives_from(entity, &target) || entity_derives_from(&target, entity);
                if recursive && (partner.is_collection || !partner.nullable.unwrap_or(true)) {
                    invalid_navigation(
                        owner,
                        nav,
                        "Recursive containment partner must be nullable and single-valued",
                        ctx,
                    );
                } else if !recursive && partner.nullable.unwrap_or(true) {
                    invalid_navigation(
                        owner,
                        nav,
                        "Non-recursive containment partner must be non-nullable",
                        ctx,
                    );
                }
            }
        }
    }
    if container_partners > 1 {
        if let Some(nav) = navs.first() {
            invalid_navigation(
                owner,
                nav,
                "Entity inheritance chain has more than one containment partner",
                ctx,
            );
        }
    }
}

impl ValidationRule for NavigationSemanticsRule {
    fn name(&self) -> &'static str {
        "navigation_semantics"
    }
    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        validate_navigation_members(
            &entity.name,
            Some(entity),
            entity.navigation_properties(),
            ctx,
        );
    }
    fn visit_complex_type(&self, complex: &ComplexType, ctx: &mut ValidationContext) {
        validate_navigation_members(&complex.name, None, complex.navigation_properties(), ctx);
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
        seen.insert(term as *const Term);

        let mut current = start_base.upgrade();
        while let Some(current_term) = current {
            if !seen.insert(std::sync::Arc::as_ptr(&current_term)) {
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

struct OperationSemanticsRule;

fn same_term_type(left: &crate::edm::TermType, right: &crate::edm::TermType) -> bool {
    use crate::edm::TermType;
    use std::sync::Arc;
    match (left, right) {
        (TermType::Primitive(a), TermType::Primitive(b)) => a == b,
        (TermType::Builtin(a), TermType::Builtin(b)) => a == b,
        (TermType::Entity(a), TermType::Entity(b)) => Arc::ptr_eq(a, b),
        (TermType::Complex(a), TermType::Complex(b)) => Arc::ptr_eq(a, b),
        (TermType::Enum(a), TermType::Enum(b)) => Arc::ptr_eq(a, b),
        (TermType::TypeDefinition(a), TermType::TypeDefinition(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

fn same_parameter_type(
    left: &crate::edm::OperationParameter,
    right: &crate::edm::OperationParameter,
) -> bool {
    left.is_collection == right.is_collection && same_term_type(&left.ty, &right.ty)
}

fn same_binding(
    bound: bool,
    left: &[crate::edm::OperationParameter],
    right: &[crate::edm::OperationParameter],
) -> bool {
    !bound
        || match (left.first(), right.first()) {
            (Some(left), Some(right)) => same_parameter_type(left, right),
            _ => false,
        }
}

fn same_return_type(
    left: Option<&crate::edm::OperationReturnType>,
    right: Option<&crate::edm::OperationReturnType>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.is_collection == right.is_collection && same_term_type(&left.ty, &right.ty)
        }
        (None, None) => true,
        _ => false,
    }
}

fn invalid_operation(name: &str, reason: &'static str, ctx: &mut ValidationContext) {
    ctx.push(ValidationError::InvalidOperation {
        operation: name.into(),
        reason,
    });
}

fn validate_operation_signature(
    name: &str,
    parameters: &[crate::edm::OperationParameter],
    return_type: Option<&crate::edm::OperationReturnType>,
    path: Option<&[EntitySetPathSegment]>,
    ctx: &mut ValidationContext,
) {
    let mut names = HashSet::new();
    for parameter in parameters {
        if parameter.name.is_empty() || !names.insert(&parameter.name) {
            invalid_operation(name, "Parameter names must be non-empty and unique", ctx);
        }
    }
    if return_type.is_some_and(|ret| {
        ret.is_collection
            && matches!(ret.ty, crate::edm::TermType::Entity(_))
            && ret.nullable_was_authored
    }) {
        invalid_operation(
            name,
            "Collection-of-entity return type must not specify Nullable",
            ctx,
        );
    }
    if let Some(path) = path {
        let Some(crate::edm::OperationReturnType {
            ty: crate::edm::TermType::Entity(returned),
            ..
        }) = return_type
        else {
            invalid_operation(
                name,
                "EntitySetPath requires an entity-valued return type",
                ctx,
            );
            return;
        };
        let mut target = parameters.first().and_then(|p| match &p.ty {
            crate::edm::TermType::Entity(entity) => Some(entity.clone()),
            _ => None,
        });
        for segment in path.iter().skip(1) {
            target = match segment {
                EntitySetPathSegment::NavigationProperty(nav) => {
                    nav.upgrade().and_then(|n| n.target.upgrade())
                }
                EntitySetPathSegment::EntityTypeCast(entity) => entity.upgrade(),
                _ => None,
            };
            if target.is_none() {
                break;
            }
        }
        if target.is_none_or(|target| !entity_derives_from(returned, &target)) {
            invalid_operation(
                name,
                "EntitySetPath must end at a compatible entity type",
                ctx,
            );
        }
    }
}

impl ValidationRule for OperationSemanticsRule {
    fn name(&self) -> &'static str {
        "operation_semantics"
    }
    fn visit_function(&self, function: &Function, ctx: &mut ValidationContext) {
        if function.return_type.is_none() {
            invalid_operation(&function.name, "Function requires a return type", ctx);
        }
        validate_operation_signature(
            &function.name,
            &function.parameters,
            function.return_type.as_ref(),
            function.entity_set_path.as_deref(),
            ctx,
        );
    }
    fn visit_action(&self, action: &Action, ctx: &mut ValidationContext) {
        validate_operation_signature(
            &action.name,
            &action.parameters,
            action.return_type.as_ref(),
            action.entity_set_path.as_deref(),
            ctx,
        );
    }
    fn visit_model(&self, model: &Model, ctx: &mut ValidationContext) {
        for (index, element) in model.elements.iter().enumerate() {
            for previous in &model.elements[..index] {
                match (element.as_ref(), previous.as_ref()) {
                    (SchemaElement::Action(action), SchemaElement::Action(other))
                        if action.name == other.name
                            && action.is_bound == other.is_bound
                            && same_binding(
                                action.is_bound,
                                &action.parameters,
                                &other.parameters,
                            ) =>
                    {
                        invalid_operation(
                            &action.name,
                            "Action overloads must have distinct binding types; unbound actions cannot overload",
                            ctx,
                        );
                    }
                    (SchemaElement::Function(function), SchemaElement::Function(other))
                        if function.name == other.name
                            && function.is_bound == other.is_bound
                            && same_binding(
                                function.is_bound,
                                &function.parameters,
                                &other.parameters,
                            ) =>
                    {
                        let skip = usize::from(function.is_bound);
                        let names = function
                            .parameters
                            .iter()
                            .skip(skip)
                            .map(|p| &p.name)
                            .collect::<HashSet<_>>();
                        let other_names = other
                            .parameters
                            .iter()
                            .skip(skip)
                            .map(|p| &p.name)
                            .collect::<HashSet<_>>();
                        let same_types = function.parameters.len() == other.parameters.len()
                            && function
                                .parameters
                                .iter()
                                .zip(&other.parameters)
                                .all(|(a, b)| same_parameter_type(a, b));
                        if names == other_names || same_types {
                            invalid_operation(
                                &function.name,
                                "Function overloads need distinct parameter-name sets and ordered type signatures",
                                ctx,
                            );
                        }
                        if !same_return_type(
                            function.return_type.as_ref(),
                            other.return_type.as_ref(),
                        ) {
                            invalid_operation(
                                &function.name,
                                "Function overloads with the same binding type must have the same return type",
                                ctx,
                            );
                        }
                    }
                    _ => {}
                }
            }
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

struct KeySemanticsRule;

impl ValidationRule for KeySemanticsRule {
    fn name(&self) -> &'static str {
        "key_semantics"
    }

    fn visit_entity_type(&self, entity: &EntityType, ctx: &mut ValidationContext) {
        let mut aliases: HashSet<String> = entity
            .properties()
            .iter()
            .map(|p| p.name.clone())
            .chain(
                entity
                    .navigation_properties()
                    .iter()
                    .map(|n| n.name.clone()),
            )
            .collect();
        for (index, key) in entity.keys().iter().enumerate() {
            let mut fail = |reason| {
                ctx.push(ValidationError::InvalidKey {
                    entity: entity.name.clone(),
                    key: key_path_to_string(key),
                    reason,
                })
            };
            let alias = entity
                .key_aliases
                .get()
                .and_then(|a| a.get(index))
                .and_then(Option::as_deref);
            match (key.len() > 1, alias) {
                (true, None) => fail("Nested key properties require an alias"),
                (false, Some(_)) => fail("Direct key properties must not have an alias"),
                (_, Some(alias)) => {
                    if !is_simple_identifier(alias) {
                        fail("Key alias must be a simple identifier");
                    }
                    if !aliases.insert(alias.to_owned()) {
                        fail("Key alias conflicts with a member or another alias");
                    }
                }
                _ => {}
            }
            let mut navigation_count = 0;
            for (position, segment) in key.iter().enumerate() {
                match segment {
                    KeyPathSegment::Property(property) => {
                        if let Some(property) = property.upgrade() {
                            if property.nullable.unwrap_or(true) {
                                fail("Every key path property must be non-nullable");
                            }
                            if position + 1 == key.len() {
                                use crate::edm::PrimitiveType::*;
                                let primitive = match &property.ty {
                                    ResolvedType::Primitive(p) => Some(p),
                                    ResolvedType::TypeDefinition(t) => Some(&t.underlying_type),
                                    _ => None,
                                };
                                if primitive.is_some_and(|p| {
                                    !matches!(
                                        p,
                                        Boolean
                                            | Byte
                                            | Date
                                            | DateTimeOffset
                                            | Decimal
                                            | Duration
                                            | Guid
                                            | Int16
                                            | Int32
                                            | Int64
                                            | SByte
                                            | String
                                            | TimeOfDay
                                    )
                                }) {
                                    fail("Key property type is not permitted");
                                }
                            }
                        }
                    }
                    KeyPathSegment::NavigationProperty(nav) => {
                        if let Some(nav) = nav.upgrade() {
                            navigation_count += 1;
                            if navigation_count > 1 {
                                fail("Key paths may reference only directly related entities");
                            }
                            if nav.nullable.unwrap_or(true) {
                                fail("Every key path navigation must be non-nullable");
                            }
                            if let Some(target) = nav.target.upgrade() {
                                let tail = &key[position + 1..];
                                if !target.keys().iter().any(|k| same_key_path(k, tail)) {
                                    fail("Related entity key path must reference a key property");
                                }
                                for target_key in target.keys() {
                                    if !entity.keys().iter().any(|candidate| {
                                        candidate.len() == position + 1 + target_key.len()
                                            && same_key_path(
                                                &candidate[..position + 1],
                                                &key[..position + 1],
                                            )
                                            && same_key_path(&candidate[position + 1..], target_key)
                                    }) {
                                        fail(
                                            "All key properties of the related entity must be included",
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn same_key_path(left: &[KeyPathSegment], right: &[KeyPathSegment]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| match (a, b) {
            (KeyPathSegment::Property(a), KeyPathSegment::Property(b)) => a.ptr_eq(b),
            (KeyPathSegment::NavigationProperty(a), KeyPathSegment::NavigationProperty(b)) => {
                a.ptr_eq(b)
            }
            _ => false,
        })
}

fn is_simple_identifier(value: &str) -> bool {
    crate::conformance::is_simple_identifier(value)
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
        if let KeyPathSegment::NavigationProperty(nav) = segment {
            let Some(nav) = nav.upgrade() else {
                return KeyPathClassification::Unknown;
            };
            if nav.is_collection {
                return KeyPathClassification::Collection;
            }
            terminal = None;
            continue;
        }
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
            ResolvedType::Builtin(_) => KeyPathClassification::Complex,
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

    if !matches!(
        target.last(),
        Some(
            BindingPathSegment::EntitySet(_)
                | BindingPathSegment::Singleton(_)
                | BindingPathSegment::NavigationProperty(_)
                | BindingPathSegment::Unresolved(_)
        )
    ) {
        ctx.push(ValidationError::InvalidNavigationPropertyBinding {
            container: container.name.clone(),
            source_kind,
            source: source_name.into(),
            attribute: "Target",
            value: binding.target_string(),
            reason: "Target must end at an entity set, singleton, or containment navigation",
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
