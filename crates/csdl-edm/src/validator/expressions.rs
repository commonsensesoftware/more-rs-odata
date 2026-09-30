//! Recursive checks consume resolver bindings; authored paths are never reparsed here.
use crate::edm::*;
use crate::expr::{BinaryOperator as B, CsdlAnnotationExpression as E};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone)]
struct Expected {
    ty: TermType,
    collection: bool,
    nullable: bool,
    facets: TypeFacets,
}
impl Expected {
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
) -> bool {
    let (Some(term), Some(expression)) = (&annotation.term, &annotation.expression) else {
        return true;
    };
    let Some(ty) = term.ty() else {
        return false;
    };
    Checker {
        annotation,
        labels,
        active: HashSet::new(),
    }
    .check(
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
    annotation: &'a ResolvedAnnotation,
    labels: &'a HashMap<String, Vec<(&'a E, &'a ResolvedAnnotation)>>,
    active: HashSet<String>,
}
impl Checker<'_> {
    fn check(&mut self, e: &E, expected: &Expected) -> bool {
        use PrimitiveType as P;
        match e {
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
                    && !self.check(expr, &target)
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
                return values.iter().all(|v| self.check(v, &item));
            }
            E::If { test, then_, else_ } => {
                return self.check(test, &Expected::primitive(P::Boolean))
                    && self.check(then_, expected)
                    && else_
                        .as_ref()
                        .map_or(expected.nullable, |e| self.check(e, expected));
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
                let inherited = self.annotation.term.as_ref().is_some_and(|t| {
                    t.base_term
                        .get()
                        .and_then(Option::as_ref)
                        .and_then(|w| w.upgrade())
                        .is_some()
                });
                return inherited
                    || (members.iter().all(|p| {
                        p.is_collection
                            || p.nullable != Some(false)
                            || p.default_value.is_some()
                            || seen.contains(&p.name)
                    }) && navigations.iter().all(|p| {
                        p.is_collection || p.nullable != Some(false) || seen.contains(&p.name)
                    }));
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
                    && names
                        .iter()
                        .all(|name| t.members.iter().any(|m| m.name == *name));
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
                    ty.collection |= path.segments.iter().any(
                        |s| matches!(s, AnnotationTarget::NavigationProperty(p) if p.is_collection),
                    );
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
                Expected::primitive(P::String)
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
                    if source.collection != target.collection
                        || !(assignable(&source.ty, ty)
                            || assignable(ty, &source.ty)
                            || numeric(&source.ty) && numeric(ty))
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
                let mut ty = self.infer(values.first()?)?;
                if ty.collection || !values.iter().all(|v| self.check(v, &ty)) {
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
            return (assignable(&left.ty, &right.ty)
                || assignable(&right.ty, &left.ty)
                || numeric(&left.ty) && numeric(&right.ty))
            .then(|| Expected::primitive(P::Boolean));
        }
        if numeric(&left.ty) && numeric(&right.ty) {
            let p = if op == B::DivBy {
                P::Decimal
            } else if matches!(primitive(&left.ty), Some(P::Double | P::Single))
                || matches!(primitive(&right.ty), Some(P::Double | P::Single))
            {
                P::Double
            } else if matches!(primitive(&left.ty), Some(P::Decimal))
                || matches!(primitive(&right.ty), Some(P::Decimal))
            {
                P::Decimal
            } else {
                P::Int64
            };
            return Some(Expected::primitive(p));
        }
        match (primitive(&left.ty)?, primitive(&right.ty)?, op) {
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
        return a == b || *b == PrimitiveType::PrimitiveType;
    }
    match (source, target) {
        (TermType::Builtin(a), TermType::Builtin(b)) => {
            a == b
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
fn endpoint(target: &AnnotationTarget) -> Option<Expected> {
    Some(match target {
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
