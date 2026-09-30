use crate::edm::*;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationIssue {
    UnknownTerm,
    UnresolvedTarget,
    InvalidQualifier,
    Duplicate,
    NotApplicable,
    MissingBaseTerm,
    InvalidExpressionPath,
    IncompatibleValue,
    MissingTargetAnnotation,
}

#[derive(Debug, Clone)]
pub struct AnnotationDiagnostic {
    pub term: String,
    pub target: String,
    pub issue: AnnotationIssue,
}

/// Authoring diagnostics for annotations. These deliberately do not make
/// validate_document fail: CSDL requires consumers to tolerate unknown/invalid
/// annotations, including unavailable referenced vocabularies.
pub fn validate_annotations(document: &DocumentModel) -> Vec<AnnotationDiagnostic> {
    collect(document.all_schemas().map(Arc::as_ref))
}

pub fn validate_model_annotations(model: &Model) -> Vec<AnnotationDiagnostic> {
    collect(std::iter::once(model))
}

fn collect<'a>(models: impl Iterator<Item = &'a Model>) -> Vec<AnnotationDiagnostic> {
    let annotations = models
        .flat_map(|m| m.annotations.get().into_iter().flatten())
        .collect::<Vec<_>>();
    let mut errors = Vec::new();
    let mut applications = HashSet::new();
    for annotation in &annotations {
        if let Some(term) = &annotation.term {
            for target in &annotation.targets {
                let key = (
                    identity(target),
                    Arc::as_ptr(term) as usize,
                    annotation.qualifier.clone(),
                );
                if !applications.insert(key) {
                    push(&mut errors, annotation, AnnotationIssue::Duplicate);
                }
            }
        }
    }
    for annotation in annotations {
        if annotation
            .qualifier
            .as_ref()
            .is_some_and(|q| !super::is_simple_identifier(q))
        {
            push(&mut errors, annotation, AnnotationIssue::InvalidQualifier);
        }
        if annotation.targets.is_empty()
            || annotation.targets.iter().any(|p| {
                p.iter().any(|s| {
                    matches!(
                        s,
                        AnnotationTarget::Unresolved(_) | AnnotationTarget::DynamicProperty(_)
                    )
                })
            })
        {
            push(&mut errors, annotation, AnnotationIssue::UnresolvedTarget);
        }
        for target in &annotation.targets {
            for (index, segment) in target.iter().enumerate() {
                if let AnnotationTarget::AppliedTerm { term, qualifier } = segment {
                    if !applications.contains(&(
                        identity(&target[..index]),
                        Arc::as_ptr(term) as usize,
                        qualifier.clone(),
                    )) {
                        push(
                            &mut errors,
                            annotation,
                            AnnotationIssue::MissingTargetAnnotation,
                        );
                    }
                }
            }
        }
        let Some(term) = &annotation.term else {
            push(&mut errors, annotation, AnnotationIssue::UnknownTerm);
            continue;
        };
        for target in &annotation.targets {
            if let Some(last) = target.last() {
                let kind = kind(last);
                if !term.applies_to.is_empty()
                    && !term.applies_to.contains(&kind)
                    && !(term.applies_to.contains(&AnnotationTargetKind::Collection)
                        && is_collection(last))
                {
                    push(&mut errors, annotation, AnnotationIssue::NotApplicable);
                }
            }
            let mut base = term
                .base_term
                .get()
                .and_then(Option::as_ref)
                .and_then(|t| t.upgrade());
            let mut visited = HashSet::new();
            while let Some(t) = base {
                if !visited.insert(Arc::as_ptr(&t)) {
                    break;
                }
                if !applications.contains(&(
                    identity(target),
                    Arc::as_ptr(&t) as usize,
                    annotation.qualifier.clone(),
                )) {
                    push(&mut errors, annotation, AnnotationIssue::MissingBaseTerm);
                }
                base = t
                    .base_term
                    .get()
                    .and_then(Option::as_ref)
                    .and_then(|t| t.upgrade());
            }
        }
        for path in &annotation.expression_paths {
            let invalid = path
                .segments
                .iter()
                .any(|s| matches!(s, AnnotationTarget::Unresolved(_)))
                || match path.kind {
                    AnnotationPathKind::Value => false,
                    AnnotationPathKind::Property => {
                        !matches!(path.segments.last(), Some(AnnotationTarget::Property(_)))
                    }
                    AnnotationPathKind::NavigationProperty => !matches!(
                        path.segments.last(),
                        Some(AnnotationTarget::NavigationProperty(_))
                    ),
                    AnnotationPathKind::Annotation => !matches!(
                        path.segments.last(),
                        Some(AnnotationTarget::AppliedTerm { .. })
                    ),
                };
            if invalid {
                push(
                    &mut errors,
                    annotation,
                    AnnotationIssue::InvalidExpressionPath,
                );
            }
        }
        if let (Some(expression), Some(ty)) = (&annotation.expression, term.ty()) {
            if !compatible(
                expression,
                ty,
                term.is_collection,
                term.nullable.unwrap_or(true),
            ) {
                push(&mut errors, annotation, AnnotationIssue::IncompatibleValue);
            }
        }
    }
    errors
}

fn push(
    out: &mut Vec<AnnotationDiagnostic>,
    annotation: &ResolvedAnnotation,
    issue: AnnotationIssue,
) {
    out.push(AnnotationDiagnostic {
        term: annotation.term_name.clone(),
        target: annotation.target_path.clone(),
        issue,
    });
}

fn identity(path: &[AnnotationTarget]) -> Vec<String> {
    use AnnotationTarget as T;
    path.iter()
        .map(|target| match target {
            T::Expression { kind, ordinal } => format!("expression:{kind:?}:{ordinal}"),
            T::Schema(s) => format!("schema:{s}"),
            T::Element(e) => format!("element:{:p}", Arc::as_ptr(e)),
            T::Property(e) => format!("property:{:p}", Arc::as_ptr(e)),
            T::NavigationProperty(e) => format!("navigation:{:p}", Arc::as_ptr(e)),
            T::EnumMember(e) => format!("member:{:p}", Arc::as_ptr(e)),
            T::Container(e) => format!("container:{:p}", Arc::as_ptr(e)),
            T::Resource(e) => format!("resource:{:p}", Arc::as_ptr(e)),
            T::Parameter { operation, index } => {
                format!("parameter:{:p}:{index}", Arc::as_ptr(operation))
            }
            T::ReturnType(e) => format!("return:{:p}", Arc::as_ptr(e)),
            T::ReferentialConstraint { navigation, index } => {
                format!("constraint:{:p}:{index}", Arc::as_ptr(navigation))
            }
            T::OnDelete(e) => format!("delete:{:p}", Arc::as_ptr(e)),
            T::AppliedTerm { term, qualifier } => {
                format!("annotation:{:p}:{qualifier:?}", Arc::as_ptr(term))
            }
            T::DynamicProperty(s) => format!("dynamic:{s}"),
            T::Unresolved(s) => format!("unknown:{s}"),
        })
        .collect()
}

fn kind(target: &AnnotationTarget) -> AnnotationTargetKind {
    use AnnotationTarget as T;
    use AnnotationTargetKind as K;
    match target {
        T::Expression { kind, .. } => kind.clone(),
        T::Schema(_) => K::Schema,
        T::Element(e) => match e.as_ref() {
            SchemaElement::EntityType(_) => K::EntityType,
            SchemaElement::ComplexType(_) => K::ComplexType,
            SchemaElement::EnumType(_) => K::EnumType,
            SchemaElement::TypeDefinition(_) => K::TypeDefinition,
            SchemaElement::Term(_) => K::Term,
            SchemaElement::Function(_) => K::Function,
            SchemaElement::Action(_) => K::Action,
        },
        T::Property(_) => K::Property,
        T::NavigationProperty(_) => K::NavigationProperty,
        T::EnumMember(_) => K::Member,
        T::Container(_) => K::EntityContainer,
        T::Resource(r) => match r.as_ref() {
            EntityContainerElement::EntitySet(_) => K::EntitySet,
            EntityContainerElement::Singleton(_) => K::Singleton,
            EntityContainerElement::FunctionImport(_) => K::FunctionImport,
            EntityContainerElement::ActionImport(_) => K::ActionImport,
        },
        T::Parameter { .. } => K::Parameter,
        T::ReturnType(_) => K::ReturnType,
        T::ReferentialConstraint { .. } => K::ReferentialConstraint,
        T::OnDelete(_) => K::OnDelete,
        T::AppliedTerm { .. } => K::Annotation,
        T::DynamicProperty(_) | T::Unresolved(_) => K::Other("Unresolved".into()),
    }
}

fn is_collection(target: &AnnotationTarget) -> bool {
    match target {
        AnnotationTarget::Property(p) => p.is_collection,
        AnnotationTarget::NavigationProperty(n) => n.is_collection,
        AnnotationTarget::Resource(r) => matches!(r.as_ref(), EntityContainerElement::EntitySet(_)),
        _ => false,
    }
}

fn compatible(
    expression: &crate::expr::CsdlAnnotationExpression,
    ty: &TermType,
    collection: bool,
    nullable: bool,
) -> bool {
    use crate::expr::CsdlAnnotationExpression as E;
    use PrimitiveType as P;
    if matches!(expression, E::Null) {
        return nullable && !collection;
    }
    if let E::Collection(values) = expression {
        return collection && values.iter().all(|v| compatible(v, ty, false, nullable));
    }
    if matches!(
        expression,
        E::Path(_)
            | E::If { .. }
            | E::Apply { .. }
            | E::Cast { .. }
            | E::LabeledElement { .. }
            | E::LabeledElementReference(_)
    ) {
        return true;
    }
    if collection {
        return false;
    }
    let primitive = match ty {
        TermType::Primitive(t) => Some(t),
        TermType::TypeDefinition(t) => Some(&t.underlying_type),
        TermType::Builtin(BuiltinType::Untyped) => return true,
        TermType::Builtin(BuiltinType::PropertyPath) => {
            return matches!(expression, E::PropertyPath(_) | E::String(_));
        }
        TermType::Builtin(BuiltinType::NavigationPropertyPath) => {
            return matches!(expression, E::NavigationPropertyPath(_) | E::String(_));
        }
        TermType::Builtin(BuiltinType::AnnotationPath) => {
            return matches!(expression, E::AnnotationPath(_) | E::String(_));
        }
        TermType::Builtin(_) => {
            return matches!(
                expression,
                E::PropertyPath(_)
                    | E::NavigationPropertyPath(_)
                    | E::AnnotationPath(_)
                    | E::String(_)
            );
        }
        TermType::Complex(_) | TermType::Entity(_) => {
            return matches!(expression, E::Record { .. });
        }
        TermType::Enum(_) => return matches!(expression, E::EnumMember(_) | E::String(_)),
    };
    match (primitive, expression) {
        (Some(P::PrimitiveType), _) => true,
        (
            Some(P::Boolean),
            E::Bool(_) | E::Not(_) | E::IsOf { .. } | E::BinaryExpression { .. },
        ) => true,
        (Some(P::Byte), E::Int(n)) => (0..=255).contains(n),
        (Some(P::SByte), E::Int(n)) => (-128..=127).contains(n),
        (Some(P::Int16), E::Int(n)) => i16::try_from(*n).is_ok(),
        (Some(P::Int32), E::Int(n)) => i32::try_from(*n).is_ok(),
        (Some(P::Int64), E::Int(_) | E::String(_)) => true,
        (Some(P::Decimal), E::Decimal(_) | E::Int(_) | E::Float(_) | E::String(_)) => true,
        (Some(P::Single | P::Double), E::Float(_) | E::Int(_)) => true,
        (Some(P::String), E::String(_) | E::UrlRef(_)) => true,
        (Some(P::Binary), E::Binary(_) | E::String(_)) => true,
        (Some(P::Date), E::Date(_) | E::String(_)) => true,
        (Some(P::DateTimeOffset), E::DateTimeOffset(_) | E::String(_)) => true,
        (Some(P::Duration), E::Duration(_) | E::String(_)) => true,
        (Some(P::Guid), E::Guid(_) | E::String(_)) => true,
        (Some(P::TimeOfDay), E::TimeOfDay(_) | E::String(_)) => true,
        _ => false,
    }
}
