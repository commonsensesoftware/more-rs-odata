//! Recursive checks consume resolver bindings; authored paths are never reparsed here.
use super::{AnnotationValidationOptions, ExpressionType};
use crate::edm::*;
use crate::expr::{BinaryOperator as B, CsdlAnnotationExpression as E};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) fn has_unknown_url_type(
    annotation: &ResolvedAnnotation,
    options: &AnnotationValidationOptions,
) -> bool {
    fn walk(e: &E, options: &AnnotationValidationOptions) -> bool {
        if let E::UrlRef(arg) = e {
            if !matches!(arg.as_ref(), E::String(uri) if options.url_result_types.contains_key(uri))
            {
                return true;
            }
        }
        children(e).iter().any(|e| walk(e, options))
    }
    annotation
        .expression
        .as_ref()
        .is_some_and(|e| walk(e, options))
}

#[derive(Clone)]
struct Expected {
    ty: TermType,
    collection: bool,
    nullable: bool,
    facets: TypeFacets,
}
impl Expected {
    fn supplied(t: &ExpressionType) -> Self {
        Self {
            ty: t.ty.clone(),
            collection: t.is_collection,
            nullable: t.nullable,
            facets: t.facets.clone(),
        }
    }
    fn scalar(ty: TermType) -> Self {
        Self {
            ty,
            collection: false,
            nullable: true,
            facets: TypeFacets::default(),
        }
    }
    fn primitive(ty: PrimitiveType) -> Self {
        Self::scalar(TermType::Primitive(ty))
    }
}

pub(super) fn valid(
    annotation: &ResolvedAnnotation,
    labels: &HashMap<String, Vec<(&E, &ResolvedAnnotation)>>,
    annotations: &[&ResolvedAnnotation],
    options: &AnnotationValidationOptions,
) -> bool {
    let (Some(term), Some(expression)) = (&annotation.term, &annotation.expression) else {
        return true;
    };
    let Some(ty) = term.ty() else {
        return false;
    };
    let mut checker = Checker {
        annotations,
        options,
        annotation,
        labels,
        active: HashSet::new(),
    };
    for path in &annotation.expression_paths {
        for segment in path.segments.iter() {
            if let AnnotationTarget::SelectedItem {
                selector: PathSelector::Key(args),
                ..
            } = segment
            {
                for arg in args {
                    let expected = Expected {
                        ty: property_type(&arg.property.ty),
                        collection: false,
                        nullable: false,
                        facets: arg.property.facets.clone(),
                    };
                    let valid = match &arg.value {
                        PathKeyValue::Literal(value) => checker.check(value, &expected),
                        PathKeyValue::Path(path) => path
                            .last()
                            .and_then(endpoint)
                            .is_some_and(|ty| !ty.collection && assignable(&ty.ty, &expected.ty)),
                    };
                    if !valid {
                        return false;
                    }
                }
            }
        }
    }
    checker.check(
        expression,
        &Expected {
            ty: ty.clone(),
            collection: term.is_collection,
            nullable: term.nullable.unwrap_or(true),
            facets: term.facets.clone(),
        },
    )
}

pub(super) fn labels<'a>(
    annotations: &[&'a ResolvedAnnotation],
) -> HashMap<String, Vec<(&'a E, &'a ResolvedAnnotation)>> {
    fn walk<'a>(
        e: &'a E,
        a: &'a ResolvedAnnotation,
        out: &mut HashMap<String, Vec<(&'a E, &'a ResolvedAnnotation)>>,
    ) {
        if let E::LabeledElement { name, expr } = e {
            if let Some(name) = a.expression_bindings.labels.get(name) {
                out.entry(name.clone()).or_default().push((expr, a));
            }
        }
        for child in children(e) {
            walk(child, a, out);
        }
    }
    let mut out = HashMap::new();
    for a in annotations {
        if let Some(e) = &a.expression {
            walk(e, a, &mut out);
        }
    }
    out
}

fn children(e: &E) -> Vec<&E> {
    match e {
        E::Annotated { expression, .. }
        | E::Faceted { expression, .. }
        | E::Not(expression)
        | E::Neg(expression)
        | E::UrlRef(expression)
        | E::Cast {
            expr: expression, ..
        }
        | E::IsOf {
            expr: expression, ..
        }
        | E::LabeledElement {
            expr: expression, ..
        } => vec![expression],
        E::BinaryExpression { lhs, rhs, .. } => vec![lhs, rhs],
        E::If { test, then_, else_ } => {
            let mut v = vec![test.as_ref(), then_.as_ref()];
            v.extend(else_.as_deref());
            v
        }
        E::Apply { args, .. } | E::Collection(args) => args.iter().collect(),
        E::Record { properties, .. } => {
            properties.iter().filter_map(|p| p.value.as_ref()).collect()
        }
        _ => Vec::new(),
    }
}

struct Checker<'a> {
    annotations: &'a [&'a ResolvedAnnotation],
    options: &'a AnnotationValidationOptions,
    annotation: &'a ResolvedAnnotation,
    labels: &'a HashMap<String, Vec<(&'a E, &'a ResolvedAnnotation)>>,
    active: HashSet<String>,
}
impl Checker<'_> {
    fn ordered(&self, e: &E) -> bool {
        match e {
            E::Collection(_) => true,
            E::Annotated { expression, .. } | E::Faceted { expression, .. } => {
                self.ordered(expression)
            }
            E::Path(source) => self
                .annotation
                .expression_paths
                .iter()
                .filter(|p| p.source == *source)
                .all(|p| p.is_ordered),
            E::Apply { function, args } if function == "odata.substring" => {
                args.first().is_some_and(|e| self.ordered(e))
            }
            E::Apply { function, .. } => self
                .options
                .client_functions
                .get(function)
                .is_some_and(|s| !s.is_empty() && s.iter().all(|s| s.return_type.is_ordered)),
            E::UrlRef(arg) => {
                matches!(arg.as_ref(), E::String(uri) if self.options.url_result_types.get(uri).is_some_and(|t| t.is_ordered))
            }
            _ => false,
        }
    }
    fn inherited_members(&self) -> HashSet<String> {
        fn members(e: &E, out: &mut HashSet<String>) {
            match e {
                E::Record { properties, .. } => {
                    out.extend(properties.iter().map(|p| p.property.clone()))
                }
                E::Annotated { expression, .. } | E::Faceted { expression, .. } => {
                    members(expression, out)
                }
                _ => {}
            }
        }
        let mut out = HashSet::new();
        let mut base = self
            .annotation
            .term
            .as_ref()
            .and_then(|t| t.base_term.get())
            .and_then(Option::as_ref)
            .and_then(|t| t.upgrade());
        let mut visited = HashSet::new();
        while let Some(term) = base {
            if !visited.insert(Arc::as_ptr(&term)) {
                break;
            }
            for a in self.annotations {
                if a.term.as_ref().is_some_and(|t| Arc::ptr_eq(t, &term))
                    && a.qualifier == self.annotation.qualifier
                    && a.targets.iter().any(|p| {
                        self.annotation.targets.iter().any(|q| {
                            super::annotations::identity(p) == super::annotations::identity(q)
                        })
                    })
                {
                    if let Some(e) = &a.expression {
                        members(e, &mut out);
                    }
                }
            }
            base = term
                .base_term
                .get()
                .and_then(Option::as_ref)
                .and_then(|t| t.upgrade());
        }
        out
    }
    fn check_item(&mut self, e: &E, expected: &Expected) -> bool {
        match e {
            E::Annotated { expression, .. } => self.check_item(expression, expected),
            E::If {
                test,
                then_,
                else_: None,
            } => {
                self.check(test, &Expected::primitive(PrimitiveType::Boolean))
                    && self.check(then_, expected)
            }
            _ => self.check(e, expected),
        }
    }
    fn check(&mut self, e: &E, expected: &Expected) -> bool {
        use PrimitiveType as P;
        match e {
            E::UrlRef(arg) => {
                if !self.check(arg, &Expected::primitive(P::String)) {
                    return false;
                }
                return match arg.as_ref() {
                    E::String(uri) => self.options.url_result_types.get(uri).is_none_or(|t| {
                        t.is_collection == expected.collection && assignable(&t.ty, &expected.ty)
                    }),
                    _ => true,
                };
            }
            E::Annotated { expression, .. } => return self.check(expression, expected),
            E::Faceted { expression, facets } => {
                let (E::Cast { type_, expr } | E::IsOf { type_, expr }) = expression.as_ref()
                else {
                    return false;
                };
                let Some((ty, collection)) = self.annotation.expression_bindings.types.get(type_)
                else {
                    return false;
                };
                let target = Expected {
                    ty: ty.clone(),
                    collection: *collection,
                    nullable: facets.nullable.unwrap_or(true),
                    facets: TypeFacets {
                        max_length: facets.max_length.clone(),
                        precision: facets.precision.clone(),
                        scale: facets.scale.clone(),
                        srid: facets.srid.clone(),
                        unicode: facets.unicode,
                    },
                };
                if !valid_facets(&target) {
                    return false;
                }
                if matches!(expression.as_ref(), E::Cast { .. })
                    && literal(expr).is_some()
                    && !literal(expr).is_some_and(|value| {
                        primitive(&target.ty).is_some_and(|p| {
                            super::values::primitive(&value, p, &effective_facets(&target))
                        })
                    })
                {
                    return false;
                }
                return self.check(expression, expected);
            }
            E::Null => return expected.nullable && !expected.collection,
            E::Collection(values) => {
                if !expected.collection {
                    return matches!(expected.ty, TermType::Builtin(BuiltinType::Untyped));
                }
                let mut item = expected.clone();
                item.collection = false;
                return values.iter().all(|v| self.check_item(v, &item));
            }
            E::If { test, then_, else_ } => {
                return self.check(test, &Expected::primitive(P::Boolean))
                    && self.check(then_, expected)
                    && else_.as_ref().is_some_and(|e| self.check(e, expected));
            }
            E::LabeledElement { name, expr } => {
                let Some(canonical) = self.annotation.expression_bindings.labels.get(name) else {
                    return false;
                };
                return crate::conformance::is_simple_identifier(name)
                    && self.labels.get(canonical).is_some_and(|v| v.len() == 1)
                    && self.check(expr, expected);
            }
            E::LabeledElementReference(name) => {
                if !crate::conformance::is_qualified_name(name) {
                    return false;
                }
                let Some(name) = self.annotation.expression_bindings.labels.get(name) else {
                    return false;
                };
                let Some(values) = self.labels.get(name).filter(|v| v.len() == 1) else {
                    return false;
                };
                if !self.active.insert(name.clone()) {
                    return false;
                }
                let saved = self.annotation;
                self.annotation = values[0].1;
                let valid = self.check(values[0].0, expected);
                self.annotation = saved;
                self.active.remove(name);
                return valid;
            }
            E::Record {
                type_, properties, ..
            } => {
                if expected.collection {
                    return false;
                }
                let actual = match type_ {
                    Some(name) => match self.annotation.expression_bindings.types.get(name) {
                        Some((ty, false)) if assignable(ty, &expected.ty) => ty,
                        _ => return false,
                    },
                    None => &expected.ty,
                };
                let (members, navigations, open, abstract_) = match actual {
                    TermType::Complex(t) => (
                        t.properties(),
                        t.navigation_properties(),
                        t.is_open,
                        t.is_abstract,
                    ),
                    TermType::Entity(t) => (
                        t.properties(),
                        t.navigation_properties(),
                        t.is_open,
                        t.is_abstract,
                    ),
                    _ => return false,
                };
                if abstract_ {
                    return false;
                }
                let mut seen = HashSet::new();
                for value in properties {
                    if !seen.insert(&value.property) {
                        return false;
                    }
                    let Some(e) = &value.value else {
                        return false;
                    };
                    let target = if let Some(p) = members.iter().find(|p| p.name == value.property)
                    {
                        Expected {
                            ty: property_type(&p.ty),
                            collection: p.is_collection,
                            nullable: p.nullable.unwrap_or(true),
                            facets: p.facets.clone(),
                        }
                    } else if let Some(p) = navigations.iter().find(|p| p.name == value.property) {
                        let Some(t) = p.target.upgrade() else {
                            return false;
                        };
                        Expected {
                            ty: TermType::Entity(t),
                            collection: p.is_collection,
                            nullable: p.nullable.unwrap_or(true),
                            facets: TypeFacets::default(),
                        }
                    } else if open {
                        let Some(t) = self.infer(e) else {
                            return false;
                        };
                        t
                    } else {
                        return false;
                    };
                    if !self.check(e, &target) {
                        return false;
                    }
                }
                // A derived term may complete the record inherited from its base term.
                fn root(e: &E) -> &E {
                    match e {
                        E::Annotated { expression, .. }
                        | E::Faceted { expression, .. }
                        | E::LabeledElement {
                            expr: expression, ..
                        } => root(expression),
                        _ => e,
                    }
                }
                let inherited = if self
                    .annotation
                    .expression
                    .as_ref()
                    .is_some_and(|value| std::ptr::eq(root(value), e))
                {
                    self.inherited_members()
                } else {
                    HashSet::new()
                };
                return members.iter().all(|p| {
                    p.is_collection
                        || p.nullable != Some(false)
                        || p.default_value.is_some()
                        || seen.contains(&p.name)
                        || inherited.contains(&p.name)
                }) && navigations.iter().all(|p| {
                    p.is_collection
                        || p.nullable != Some(false)
                        || seen.contains(&p.name)
                        || inherited.contains(&p.name)
                });
            }
            E::EnumMember(value) => {
                let TermType::Enum(t) = &expected.ty else {
                    return false;
                };
                return !expected.collection
                    && self
                        .annotation
                        .expression_bindings
                        .enum_members
                        .get(value)
                        .is_some_and(|members| {
                            !members.is_empty()
                                && (members.len() == 1 || t.is_flags)
                                && members.iter().all(|(owner, _)| Arc::ptr_eq(owner, t))
                        });
            }
            E::String(value) if matches!(expected.ty, TermType::Enum(_)) => {
                let TermType::Enum(t) = &expected.ty else {
                    unreachable!()
                };
                let names = value.split(',').map(str::trim).collect::<Vec<_>>();
                return !expected.collection
                    && (names.len() == 1 || t.is_flags)
                    && names.iter().all(|name| {
                        t.members.iter().any(|m| m.name == *name)
                            || name.parse::<i64>().is_ok_and(|n| {
                                (!t.is_flags || n >= 0)
                                    && super::values::primitive(
                                        name,
                                        &t.underlying_type,
                                        &TypeFacets::default(),
                                    )
                            })
                    });
            }
            _ => {}
        }
        if let Some(text) = literal(e) {
            if expected.collection {
                return false;
            }
            if matches!(expected.ty, TermType::Builtin(BuiltinType::Untyped)) {
                return true;
            }
            if let Some(p) = primitive(&expected.ty) {
                if self.annotation.source_format == Some(crate::CsdlFormat::Xml)
                    && *p != P::PrimitiveType
                {
                    match e {
                        E::String(_) if *p != P::String => return false,
                        E::Float(_) if !matches!(p, P::Single | P::Double) => return false,
                        E::Decimal(_) if *p != P::Decimal => return false,
                        _ => {}
                    }
                }
                if !super::annotations::compatible(e, &expected.ty, false, expected.nullable) {
                    return false;
                }
                return super::values::primitive(&text, p, &effective_facets(expected));
            }
        }
        let Some(actual) = self.infer(e) else {
            return false;
        };
        actual.collection == expected.collection && assignable(&actual.ty, &expected.ty)
    }

    fn infer(&mut self, e: &E) -> Option<Expected> {
        use PrimitiveType as P;
        let result = match e {
            E::Annotated { expression, .. } | E::Faceted { expression, .. } => {
                return self.infer(expression);
            }
            E::Bool(_) => Expected::primitive(P::Boolean),
            E::Int(_) => Expected::primitive(P::Int64),
            E::Float(_) => Expected::primitive(P::Double),
            E::String(_) => Expected::primitive(P::String),
            E::Decimal(_) => Expected::primitive(P::Decimal),
            E::Binary(_) => Expected::primitive(P::Binary),
            E::Date(_) => Expected::primitive(P::Date),
            E::DateTimeOffset(_) => Expected::primitive(P::DateTimeOffset),
            E::Duration(_) => Expected::primitive(P::Duration),
            E::Guid(_) => Expected::primitive(P::Guid),
            E::TimeOfDay(_) => Expected::primitive(P::TimeOfDay),
            E::PropertyPath(_) => Expected::scalar(TermType::Builtin(BuiltinType::PropertyPath)),
            E::NavigationPropertyPath(_) => {
                Expected::scalar(TermType::Builtin(BuiltinType::NavigationPropertyPath))
            }
            E::AnnotationPath(_) => {
                Expected::scalar(TermType::Builtin(BuiltinType::AnnotationPath))
            }
            E::ModelElementPath(_) => {
                Expected::scalar(TermType::Builtin(BuiltinType::ModelElementPath))
            }
            E::Path(source) => {
                let paths = self
                    .annotation
                    .expression_paths
                    .iter()
                    .filter(|p| p.kind == AnnotationPathKind::Value && p.source == *source)
                    .collect::<Vec<_>>();
                let mut result = None;
                for path in paths {
                    let mut ty = endpoint(path.segments.last()?)?;
                    ty.collection = path.is_collection;
                    if result.as_ref().is_some_and(|r: &Expected| {
                        r.collection != ty.collection || !assignable(&ty.ty, &r.ty)
                    }) {
                        return None;
                    }
                    result = Some(ty);
                }
                return result;
            }
            E::Not(expr) => {
                if !self.check(expr, &Expected::primitive(P::Boolean)) {
                    return None;
                }
                Expected::primitive(P::Boolean)
            }
            E::Neg(expr) => {
                let ty = self.infer(expr)?;
                if ty.collection
                    || !(numeric(&ty.ty) || matches!(primitive(&ty.ty), Some(P::Duration)))
                {
                    return None;
                }
                ty
            }
            E::UrlRef(expr) => {
                if !self.check(expr, &Expected::primitive(P::String)) {
                    return None;
                }
                match expr.as_ref() {
                    E::String(uri) => self
                        .options
                        .url_result_types
                        .get(uri)
                        .map(Expected::supplied)
                        .unwrap_or_else(|| {
                            Expected::scalar(TermType::Builtin(BuiltinType::Untyped))
                        }),
                    _ => Expected::scalar(TermType::Builtin(BuiltinType::Untyped)),
                }
            }
            E::BinaryExpression { op, lhs, rhs } => return self.binary(*op, lhs, rhs),
            E::If { test, then_, else_ } => {
                if !self.check(test, &Expected::primitive(P::Boolean)) {
                    return None;
                }
                let ty = self.infer(then_)?;
                if let Some(other) = else_ {
                    if !self.check(other, &ty) {
                        return None;
                    }
                }
                ty
            }
            E::Cast { type_, expr } | E::IsOf { type_, expr } => {
                let (ty, collection) = self.annotation.expression_bindings.types.get(type_)?;
                let target = Expected {
                    collection: *collection,
                    ..Expected::scalar(ty.clone())
                };
                if !matches!(expr.as_ref(), E::Null) {
                    let source = self.infer(expr)?;
                    if !matches!(e, E::IsOf { .. })
                        && (source.collection != target.collection
                            || !(assignable(&source.ty, ty)
                                || assignable(ty, &source.ty)
                                || numeric(&source.ty) && numeric(ty)
                                || matches!(primitive(&source.ty), Some(P::String))
                                    && primitive(ty).is_some()
                                || primitive(ty) == Some(&P::String)
                                    && matches!(
                                        source.ty,
                                        TermType::Primitive(_)
                                            | TermType::TypeDefinition(_)
                                            | TermType::Enum(_)
                                    )))
                    {
                        return None;
                    }
                }
                if matches!(e, E::IsOf { .. }) {
                    Expected::primitive(P::Boolean)
                } else {
                    target
                }
            }
            E::Apply { function, args } => return self.apply(function, args),
            E::Record {
                type_: Some(name), ..
            } => {
                let (ty, false) = self.annotation.expression_bindings.types.get(name)? else {
                    return None;
                };
                let expected = Expected::scalar(ty.clone());
                if !self.check(e, &expected) {
                    return None;
                }
                expected
            }
            E::Collection(values) => {
                let Some(first) = values.first() else {
                    let mut t = Expected::scalar(TermType::Builtin(BuiltinType::Untyped));
                    t.collection = true;
                    return Some(t);
                };
                let first = if let E::If {
                    then_, else_: None, ..
                } = first
                {
                    then_.as_ref()
                } else {
                    first
                };
                let mut ty = self.infer(first)?;
                if ty.collection || !values.iter().all(|v| self.check_item(v, &ty)) {
                    return None;
                }
                ty.collection = true;
                ty
            }
            E::EnumMember(value) => {
                let members = self
                    .annotation
                    .expression_bindings
                    .enum_members
                    .get(value)?;
                let expected = Expected::scalar(TermType::Enum(members.first()?.0.clone()));
                if !self.check(e, &expected) {
                    return None;
                }
                expected
            }
            E::LabeledElement { name, expr } => {
                let canonical = self.annotation.expression_bindings.labels.get(name)?;
                if !crate::conformance::is_simple_identifier(name)
                    || !self.labels.get(canonical).is_some_and(|v| v.len() == 1)
                {
                    return None;
                }
                return self.infer(expr);
            }
            E::LabeledElementReference(name) => {
                let canonical = self.annotation.expression_bindings.labels.get(name)?;
                let values = self.labels.get(canonical).filter(|v| v.len() == 1)?;
                if !self.active.insert(canonical.clone()) {
                    return None;
                }
                let saved = self.annotation;
                self.annotation = values[0].1;
                let ty = self.infer(values[0].0);
                self.annotation = saved;
                self.active.remove(canonical);
                return ty;
            }
            _ => return None,
        };
        if let Some(text) = literal(e) {
            if !super::values::primitive(
                &text,
                primitive(&result.ty)?,
                &TypeFacets {
                    precision: Some("12".into()),
                    ..Default::default()
                },
            ) {
                return None;
            }
        }
        Some(result)
    }

    fn binary(&mut self, op: B, lhs: &E, rhs: &E) -> Option<Expected> {
        use PrimitiveType as P;
        if matches!(op, B::Eq | B::Ne) && (matches!(lhs, E::Null) || matches!(rhs, E::Null)) {
            let other = if matches!(lhs, E::Null) { rhs } else { lhs };
            return (matches!(other, E::Null) || self.infer(other).is_some())
                .then(|| Expected::primitive(P::Boolean));
        }
        if matches!(op, B::And | B::Or) {
            let ty = Expected::primitive(P::Boolean);
            return (self.check(lhs, &ty) && self.check(rhs, &ty)).then_some(ty);
        }
        let left = self.infer(lhs)?;
        if left.collection {
            if matches!(op, B::Eq | B::Ne) {
                let right = self.infer(rhs)?;
                return (right.collection
                    && (assignable(&left.ty, &right.ty) || assignable(&right.ty, &left.ty)))
                .then(|| Expected::primitive(P::Boolean));
            }
            return None;
        }
        if op == B::In {
            let mut collection = left.clone();
            collection.collection = true;
            return self
                .check(rhs, &collection)
                .then(|| Expected::primitive(P::Boolean));
        }
        let right = self.infer(rhs)?;
        if right.collection {
            return None;
        }
        if op == B::Has {
            let (TermType::Enum(a), TermType::Enum(b)) = (&left.ty, &right.ty) else {
                return None;
            };
            return (a.is_flags && Arc::ptr_eq(a, b)).then(|| Expected::primitive(P::Boolean));
        }
        if matches!(op, B::Eq | B::Ne | B::Gt | B::Ge | B::Lt | B::Le) {
            if [&left.ty, &right.ty].iter().any(|t| {
                primitive(t).is_some_and(|p| {
                    matches!(p, P::Binary | P::Stream) || format!("{p:?}").starts_with("Geo")
                })
            }) {
                return None;
            }
            if !matches!(op, B::Eq | B::Ne)
                && (!matches!(
                    left.ty,
                    TermType::Primitive(_) | TermType::Enum(_) | TermType::TypeDefinition(_)
                ) || !matches!(
                    right.ty,
                    TermType::Primitive(_) | TermType::Enum(_) | TermType::TypeDefinition(_)
                ))
            {
                return None;
            }
            return (assignable(&left.ty, &right.ty)
                || assignable(&right.ty, &left.ty)
                || numeric(&left.ty) && numeric(&right.ty))
            .then(|| Expected::primitive(P::Boolean));
        }
        if numeric(&left.ty) && numeric(&right.ty) {
            let p = if op == B::DivBy
                && !matches!(primitive(&left.ty), Some(P::Single | P::Double))
                && !matches!(primitive(&right.ty), Some(P::Single | P::Double))
            {
                P::Decimal
            } else {
                [
                    P::Double,
                    P::Single,
                    P::Decimal,
                    P::Int64,
                    P::Int32,
                    P::Int16,
                ]
                .into_iter()
                .find(|p| primitive(&left.ty) == Some(p) || primitive(&right.ty) == Some(p))
                .unwrap_or(P::Int16)
            };
            return Some(Expected::primitive(p));
        }
        match (primitive(&left.ty)?, primitive(&right.ty)?, op) {
            (P::Duration, _, B::Mul | B::Div | B::DivBy) if numeric(&right.ty) => Some(left),
            (_, P::Duration, B::Mul) if numeric(&left.ty) => Some(right),
            (P::Date | P::DateTimeOffset, P::Duration, B::Add | B::Sub) => Some(left),
            (P::Duration, P::Duration, B::Add | B::Sub) => Some(left),
            (a, b, B::Sub) if a == b && matches!(a, P::Date | P::DateTimeOffset) => {
                Some(Expected::primitive(P::Duration))
            }
            _ => None,
        }
    }

    fn apply(&mut self, function: &str, args: &[E]) -> Option<Expected> {
        use PrimitiveType as P;
        if let Some(signatures) = self
            .options
            .client_functions
            .get(function)
            .filter(|_| !function.starts_with("odata."))
        {
            let mut result = None;
            for signature in signatures {
                if signature.parameters.len() == args.len()
                    && args
                        .iter()
                        .zip(&signature.parameters)
                        .all(|(e, t)| self.check(e, &Expected::supplied(t)))
                {
                    if result.is_some() {
                        return None;
                    }
                    result = Some(Expected::supplied(&signature.return_type));
                }
            }
            return result;
        }
        if matches!(function, "odata.cast" | "odata.isof") {
            if args.len() != 2 {
                return None;
            }
            let E::String(type_) = &args[1] else {
                return None;
            };
            let e = if function == "odata.cast" {
                E::Cast {
                    type_: type_.clone(),
                    expr: Box::new(args[0].clone()),
                }
            } else {
                E::IsOf {
                    type_: type_.clone(),
                    expr: Box::new(args[0].clone()),
                }
            };
            return self.infer(&e);
        }
        if function == "odata.case" {
            if args.is_empty() || args.len() % 2 != 0 {
                return None;
            }
            let result = self.infer(&args[1])?;
            return args
                .chunks_exact(2)
                .all(|pair| {
                    self.check(&pair[0], &Expected::primitive(P::Boolean))
                        && self.check(&pair[1], &result)
                })
                .then_some(result);
        }
        if matches!(
            function,
            "odata.geo.distance" | "odata.geo.intersects" | "odata.geo.length"
        ) {
            let first = self.infer(args.first()?)?;
            if first.collection {
                return None;
            }
            let p = primitive(&first.ty)?;
            if function == "odata.geo.length" {
                return (args.len() == 1
                    && matches!(p, P::GeographyLineString | P::GeometryLineString))
                .then(|| Expected::primitive(P::Double));
            }
            if args.len() != 2 {
                return None;
            }
            let other = self.infer(&args[1])?;
            if other.collection {
                return None;
            }
            let q = primitive(&other.ty)?;
            if function == "odata.geo.distance" {
                return (p == q && matches!(p, P::GeographyPoint | P::GeometryPoint))
                    .then(|| Expected::primitive(P::Double));
            }
            return matches!(
                (p, q),
                (P::GeographyPoint, P::GeographyPolygon) | (P::GeometryPoint, P::GeometryPolygon)
            )
            .then(|| Expected::primitive(P::Boolean));
        }
        if matches!(
            function,
            "odata.length"
                | "odata.contains"
                | "odata.startswith"
                | "odata.endswith"
                | "odata.indexof"
                | "odata.substring"
                | "odata.hassubset"
                | "odata.hassubsequence"
        ) {
            let first = self.infer(args.first()?)?;
            if first.collection {
                if function == "odata.length" {
                    return (args.len() == 1).then(|| Expected::primitive(P::Int32));
                }
                if function != "odata.hassubset" && !self.ordered(&args[0]) {
                    return None;
                }
                if function == "odata.substring" {
                    return ((2..=3).contains(&args.len())
                        && args[1..]
                            .iter()
                            .all(|e| self.check(e, &Expected::primitive(P::Int32))))
                    .then_some(first);
                }
                return (args.len() == 2
                    && (function == "odata.hassubset" || self.ordered(&args[1]))
                    && self.check(&args[1], &first))
                .then(|| {
                    Expected::primitive(if function == "odata.indexof" {
                        P::Int32
                    } else {
                        P::Boolean
                    })
                });
            }
            if matches!(function, "odata.hassubset" | "odata.hassubsequence") {
                return None;
            }
        }
        if function == "odata.concat" {
            if args.len() < 2 {
                return None;
            }
            for arg in args {
                let t = self.infer(arg)?;
                if t.collection
                    || !matches!(
                        t.ty,
                        TermType::Primitive(_) | TermType::Enum(_) | TermType::TypeDefinition(_)
                    )
                {
                    return None;
                }
            }
            return Some(Expected::primitive(P::String));
        }
        if function == "odata.fillUriTemplate" {
            if args.len() < 2 || !self.check(&args[0], &Expected::primitive(P::String)) {
                return None;
            }
            for arg in &args[1..] {
                if !matches!(arg, E::LabeledElement { .. }) || self.infer(arg).is_none() {
                    return None;
                }
            }
            return Some(Expected::primitive(P::String));
        }
        let signature = match function {
            "odata.contains" | "odata.startswith" | "odata.endswith" | "odata.matchesPattern" => {
                Some((2, P::String, P::Boolean))
            }
            "odata.indexof" => Some((2, P::String, P::Int32)),
            "odata.length" => Some((1, P::String, P::Int32)),
            "odata.tolower" | "odata.toupper" | "odata.trim" => Some((1, P::String, P::String)),
            _ => None,
        };
        if let Some((arity, input, output)) = signature {
            return (args.len() == arity
                && args
                    .iter()
                    .all(|e| self.check(e, &Expected::primitive(input.clone()))))
            .then(|| Expected::primitive(output));
        }
        if function == "odata.uriEncode" {
            let ty = self.infer(args.first()?)?;
            return (args.len() == 1 && !ty.collection && primitive(&ty.ty).is_some())
                .then(|| Expected::primitive(P::String));
        }
        if function == "odata.substring" {
            return ((2..=3).contains(&args.len())
                && self.check(&args[0], &Expected::primitive(P::String))
                && args[1..]
                    .iter()
                    .all(|e| self.check(e, &Expected::primitive(P::Int32))))
            .then(|| Expected::primitive(P::String));
        }
        if matches!(function, "odata.round" | "odata.floor" | "odata.ceiling") {
            if args.len() != 1 {
                return None;
            }
            let ty = self.infer(&args[0])?;
            return (!ty.collection
                && matches!(primitive(&ty.ty), Some(P::Decimal | P::Double | P::Single)))
            .then_some(ty);
        }
        if matches!(
            function,
            "odata.now" | "odata.mindatetime" | "odata.maxdatetime"
        ) {
            return args
                .is_empty()
                .then(|| Expected::primitive(P::DateTimeOffset));
        }
        if matches!(
            function,
            "odata.year"
                | "odata.month"
                | "odata.day"
                | "odata.hour"
                | "odata.minute"
                | "odata.second"
                | "odata.fractionalseconds"
                | "odata.date"
                | "odata.time"
                | "odata.totaloffsetminutes"
                | "odata.totalseconds"
        ) {
            if args.len() != 1 {
                return None;
            }
            let ty = self.infer(&args[0])?;
            if ty.collection {
                return None;
            }
            let p = primitive(&ty.ty)?;
            let output = match function {
                "odata.year" | "odata.month" | "odata.day"
                    if matches!(p, P::Date | P::DateTimeOffset) =>
                {
                    P::Int32
                }
                "odata.hour" | "odata.minute" | "odata.second"
                    if matches!(p, P::TimeOfDay | P::DateTimeOffset) =>
                {
                    P::Int32
                }
                "odata.fractionalseconds" if matches!(p, P::TimeOfDay | P::DateTimeOffset) => {
                    P::Decimal
                }
                "odata.date" if *p == P::DateTimeOffset => P::Date,
                "odata.time" if *p == P::DateTimeOffset => P::TimeOfDay,
                "odata.totaloffsetminutes" if *p == P::DateTimeOffset => P::Int32,
                "odata.totalseconds" if *p == P::Duration => P::Decimal,
                _ => return None,
            };
            return Some(Expected::primitive(output));
        }
        let functions = self
            .annotation
            .expression_bindings
            .functions
            .get(function)?;
        let mut result = None;
        for f in functions {
            if f.is_bound || f.parameters.len() != args.len() {
                continue;
            }
            if !args.iter().zip(&f.parameters).all(|(e, p)| {
                self.check(
                    e,
                    &Expected {
                        ty: p.ty.clone(),
                        collection: p.is_collection,
                        nullable: p.nullable.unwrap_or(true),
                        facets: p.facets.clone(),
                    },
                )
            }) {
                continue;
            }
            if result.is_some() {
                return None;
            }
            let r = f.return_type.as_ref()?;
            result = Some(Expected {
                ty: r.ty.clone(),
                collection: r.is_collection,
                nullable: r.nullable.unwrap_or(true),
                facets: r.facets.clone(),
            });
        }
        result
    }
}

fn primitive(ty: &TermType) -> Option<&PrimitiveType> {
    match ty {
        TermType::Primitive(p) => Some(p),
        TermType::TypeDefinition(t) => Some(&t.underlying_type),
        _ => None,
    }
}
fn numeric(ty: &TermType) -> bool {
    use crate::edm::PrimitiveType::*;
    matches!(
        primitive(ty),
        Some(Byte | SByte | Int16 | Int32 | Int64 | Decimal | Single | Double)
    )
}
fn assignable(source: &TermType, target: &TermType) -> bool {
    if matches!(target, TermType::Builtin(BuiltinType::Untyped)) {
        return true;
    }
    if let (Some(a), Some(b)) = (primitive(source), primitive(target)) {
        let rank = |p: &PrimitiveType| match p {
            PrimitiveType::Byte | PrimitiveType::SByte => 0,
            PrimitiveType::Int16 => 1,
            PrimitiveType::Int32 => 2,
            PrimitiveType::Int64 => 3,
            PrimitiveType::Decimal => 4,
            PrimitiveType::Single => 5,
            PrimitiveType::Double => 6,
            _ => -1,
        };
        return a == b || *b == PrimitiveType::PrimitiveType || rank(a) >= 0 && rank(b) > rank(a);
    }
    match (source, target) {
        (TermType::Builtin(a), TermType::Builtin(b)) => {
            a == b
                || *b == BuiltinType::ModelElementPath
                    && matches!(
                        a,
                        BuiltinType::PropertyPath
                            | BuiltinType::NavigationPropertyPath
                            | BuiltinType::AnyPropertyPath
                            | BuiltinType::AnnotationPath
                    )
                || matches!(
                    (a, b),
                    (
                        BuiltinType::PropertyPath | BuiltinType::NavigationPropertyPath,
                        BuiltinType::AnyPropertyPath
                    )
                )
        }
        (TermType::Enum(a), TermType::Enum(b)) => Arc::ptr_eq(a, b),
        (TermType::Complex(a), TermType::Complex(b)) => {
            let mut next = Some(a.clone());
            while let Some(t) = next {
                if Arc::ptr_eq(&t, b) {
                    return true;
                }
                next = t
                    .base_type
                    .get()
                    .and_then(Option::as_ref)
                    .and_then(|w| w.upgrade());
            }
            Arc::ptr_eq(b, abstract_complex_type())
        }
        (TermType::Entity(a), TermType::Entity(b)) => {
            let mut next = Some(a.clone());
            while let Some(t) = next {
                if Arc::ptr_eq(&t, b) {
                    return true;
                }
                next = t
                    .base_type
                    .get()
                    .and_then(Option::as_ref)
                    .and_then(|w| w.upgrade());
            }
            Arc::ptr_eq(b, abstract_entity_type())
        }
        _ => false,
    }
}
fn property_type(ty: &ResolvedType) -> TermType {
    match ty {
        ResolvedType::Primitive(p) => TermType::Primitive(p.clone()),
        ResolvedType::Builtin(p) => TermType::Builtin(*p),
        ResolvedType::Enum(p) => TermType::Enum(p.clone()),
        ResolvedType::Complex(p) => TermType::Complex(p.clone()),
        ResolvedType::TypeDefinition(p) => TermType::TypeDefinition(p.clone()),
    }
}
pub(super) fn model_path_endpoint(target: &AnnotationTarget, entity: bool) -> bool {
    endpoint(target).is_some_and(|e| {
        let is_entity = matches!(e.ty, TermType::Entity(_));
        is_entity == entity
    })
}

fn endpoint(target: &AnnotationTarget) -> Option<Expected> {
    Some(match target {
        AnnotationTarget::Count => Expected::primitive(PrimitiveType::Int64),
        AnnotationTarget::ImplicitMediaTerm(_) => Expected::primitive(PrimitiveType::String),
        AnnotationTarget::SelectedItem { collection, .. } => {
            let mut t = endpoint(collection)?;
            t.collection = false;
            t
        }
        AnnotationTarget::Element(e) => match e.as_ref() {
            SchemaElement::EntityType(t) => Expected::scalar(TermType::Entity(t.clone())),
            SchemaElement::ComplexType(t) => Expected::scalar(TermType::Complex(t.clone())),
            _ => return None,
        },
        AnnotationTarget::Resource(r) => match r.as_ref() {
            EntityContainerElement::EntitySet(s) => {
                let mut t = Expected::scalar(TermType::Entity(s.target.clone()));
                t.collection = true;
                t
            }
            EntityContainerElement::Singleton(s) => {
                Expected::scalar(TermType::Entity(s.target.clone()))
            }
            _ => return None,
        },
        AnnotationTarget::ReturnType(operation) => {
            let r = match operation.as_ref() {
                SchemaElement::Function(f) => f.return_type.as_ref()?,
                SchemaElement::Action(a) => a.return_type.as_ref()?,
                _ => return None,
            };
            Expected {
                ty: r.ty.clone(),
                collection: r.is_collection,
                nullable: r.nullable.unwrap_or(true),
                facets: r.facets.clone(),
            }
        }
        AnnotationTarget::Property(p) => Expected {
            ty: property_type(&p.ty),
            collection: p.is_collection,
            nullable: p.nullable.unwrap_or(true),
            facets: p.facets.clone(),
        },
        AnnotationTarget::NavigationProperty(p) => Expected {
            collection: p.is_collection,
            nullable: p.nullable.unwrap_or(true),
            ..Expected::scalar(TermType::Entity(p.target.upgrade()?))
        },
        AnnotationTarget::AppliedTerm { term, .. } => Expected {
            ty: term.ty()?.clone(),
            collection: term.is_collection,
            nullable: term.nullable.unwrap_or(true),
            facets: term.facets.clone(),
        },
        AnnotationTarget::Parameter { operation, index } => {
            let p = match operation.as_ref() {
                SchemaElement::Function(f) => f.parameters.get(*index)?,
                SchemaElement::Action(a) => a.parameters.get(*index)?,
                _ => return None,
            };
            Expected {
                ty: p.ty.clone(),
                collection: p.is_collection,
                nullable: p.nullable.unwrap_or(true),
                facets: p.facets.clone(),
            }
        }
        _ => return None,
    })
}
fn literal(e: &E) -> Option<String> {
    Some(match e {
        E::Binary(v) => String::from_utf8(v.clone()).ok()?,
        E::Bool(v) => v.to_string(),
        E::Int(v) => v.to_string(),
        E::Float(v) => crate::expr::float_literal(*v),
        E::String(v)
        | E::Date(v)
        | E::DateTimeOffset(v)
        | E::Decimal(v)
        | E::Duration(v)
        | E::Guid(v)
        | E::TimeOfDay(v) => v.clone(),
        _ => return None,
    })
}
fn effective_facets(expected: &Expected) -> TypeFacets {
    let mut f = match &expected.ty {
        TermType::TypeDefinition(t) => t.facets.clone(),
        _ => TypeFacets::default(),
    };
    let a = &expected.facets;
    f.max_length = a.max_length.clone().or(f.max_length);
    f.precision = a.precision.clone().or(f.precision);
    f.scale = a.scale.clone().or(f.scale);
    f.srid = a.srid.clone().or(f.srid);
    f.unicode = a.unicode.or(f.unicode);
    f
}
fn valid_facets(target: &Expected) -> bool {
    let mut errors = Vec::new();
    let definition = match &target.ty {
        TermType::TypeDefinition(t) => Some(t.as_ref()),
        _ => None,
    };
    super::types::check(
        "expression",
        primitive(&target.ty),
        definition,
        &target.facets,
        "4.01",
        &mut errors,
    );
    errors.is_empty()
}
