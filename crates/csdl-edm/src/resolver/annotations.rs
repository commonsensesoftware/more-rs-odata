use super::*;
use crate::expr::CsdlAnnotationExpression as Expr;

type Path = Arc<[AnnotationTarget]>;

struct Scope<'a> {
    roots: HashMap<String, Vec<AnnotationTarget>>,
    terms: HashMap<String, Arc<Term>>,
    aliases: &'a HashMap<String, String>,
}

pub(super) fn resolve(
    schemas: &[csdl::Schema],
    models: &[Model],
    aliases: &HashMap<String, String>,
    scopes: Option<&std::collections::BTreeMap<String, std::collections::BTreeSet<String>>>,
) {
    let mut scope = Scope {
        roots: HashMap::new(),
        terms: HashMap::new(),
        aliases,
    };
    for model in models {
        scope.roots.insert(
            model.namespace.clone(),
            vec![AnnotationTarget::Schema(model.namespace.clone())],
        );
        for element in &model.elements {
            let name = match element.as_ref() {
                SchemaElement::EntityType(t) => &t.name,
                SchemaElement::ComplexType(t) => &t.name,
                SchemaElement::EnumType(t) => &t.name,
                SchemaElement::TypeDefinition(t) => &t.name,
                SchemaElement::Term(t) => {
                    scope
                        .terms
                        .insert(format!("{}.{}", model.namespace, t.name), t.clone());
                    &t.name
                }
                SchemaElement::Function(t) => &t.name,
                SchemaElement::Action(t) => &t.name,
            };
            scope
                .roots
                .entry(format!("{}.{name}", model.namespace))
                .or_default()
                .push(AnnotationTarget::Element(element.clone()));
        }
        for container in model.containers() {
            scope.roots.insert(
                format!("{}.{}", model.namespace, container.name),
                vec![AnnotationTarget::Container(container.clone())],
            );
        }
    }
    // Register fully typed operation signatures, retaining overload identity.
    for (schema, model) in schemas.iter().zip(models) {
        let mut elements = model.elements.iter();
        for source in &schema.elements {
            if matches!(source, csdl::SchemaElement::EntityContainer(_)) {
                continue;
            }
            let element = elements.next().expect("resolved schema element");
            let operation = match source {
                csdl::SchemaElement::Function(f) => Some((&f.name, &f.parameters)),
                csdl::SchemaElement::Action(a) => Some((&a.name, &a.parameters)),
                _ => None,
            };
            if let Some((name, parameters)) = operation {
                let signature = signature(&schema.namespace, name, parameters, aliases);
                scope
                    .roots
                    .entry(signature)
                    .or_default()
                    .push(AnnotationTarget::Element(element.clone()));
            }
        }
    }
    for (schema, model) in schemas.iter().zip(models) {
        let visible = |key: &str| {
            scopes
                .and_then(|s| s.get(&schema.namespace))
                .is_none_or(|namespaces| {
                    let root = key.split('(').next().unwrap_or(key);
                    namespaces.contains(root)
                        || root
                            .rsplit_once('.')
                            .is_some_and(|(ns, _)| namespaces.contains(ns))
                })
        };
        let scope = Scope {
            roots: scope
                .roots
                .iter()
                .filter(|(k, _)| visible(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            terms: scope
                .terms
                .iter()
                .filter(|(k, _)| visible(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            aliases,
        };
        let mut annotations = Vec::new();
        scope.attach(
            &schema.namespace,
            &schema.namespace,
            &schema.annotations,
            &mut annotations,
        );
        for group in &schema.external_annotations {
            scope.attach(
                &schema.namespace,
                &group.target,
                &group.annotations,
                &mut annotations,
            );
        }
        for element in &schema.elements {
            match element {
                csdl::SchemaElement::EntityType(t) => {
                    let owner = format!("{}.{}", schema.namespace, t.name);
                    scope.attach(&schema.namespace, &owner, &t.annotations, &mut annotations);
                    scope.members(
                        &schema.namespace,
                        &owner,
                        &t.properties,
                        &t.navigation_properties,
                        &mut annotations,
                    );
                }
                csdl::SchemaElement::ComplexType(t) => {
                    let owner = format!("{}.{}", schema.namespace, t.name);
                    scope.attach(&schema.namespace, &owner, &t.annotations, &mut annotations);
                    scope.members(
                        &schema.namespace,
                        &owner,
                        &t.properties,
                        &t.navigation_properties,
                        &mut annotations,
                    );
                }
                csdl::SchemaElement::EnumType(t) => {
                    let owner = format!("{}.{}", schema.namespace, t.name);
                    scope.attach(&schema.namespace, &owner, &t.annotations, &mut annotations);
                    for member in &t.members {
                        scope.attach(
                            &schema.namespace,
                            &format!("{owner}/{}", member.name),
                            &member.annotations,
                            &mut annotations,
                        );
                    }
                }
                csdl::SchemaElement::TypeDefinition(t) => scope.attach(
                    &schema.namespace,
                    &format!("{}.{}", schema.namespace, t.name),
                    &t.annotations,
                    &mut annotations,
                ),
                csdl::SchemaElement::Term(t) => scope.attach(
                    &schema.namespace,
                    &format!("{}.{}", schema.namespace, t.name),
                    &t.annotations,
                    &mut annotations,
                ),
                csdl::SchemaElement::Function(f) => scope.operation(
                    &schema.namespace,
                    &f.name,
                    &f.parameters,
                    f.return_type.as_ref(),
                    &f.annotations,
                    &mut annotations,
                ),
                csdl::SchemaElement::Action(a) => scope.operation(
                    &schema.namespace,
                    &a.name,
                    &a.parameters,
                    a.return_type.as_ref(),
                    &a.annotations,
                    &mut annotations,
                ),
                csdl::SchemaElement::EntityContainer(c) => {
                    let owner = format!("{}.{}", schema.namespace, c.name);
                    scope.attach(&schema.namespace, &owner, &c.annotations, &mut annotations);
                    for (name, attached) in c
                        .entity_sets
                        .iter()
                        .map(|s| (&s.name, &s.annotations))
                        .chain(c.singletons.iter().map(|s| (&s.name, &s.annotations)))
                        .chain(c.function_imports.iter().map(|s| (&s.name, &s.annotations)))
                        .chain(c.action_imports.iter().map(|s| (&s.name, &s.annotations)))
                    {
                        scope.attach(
                            &schema.namespace,
                            &format!("{owner}/{name}"),
                            attached,
                            &mut annotations,
                        );
                    }
                }
            }
        }
        let _ = model.annotations.set(annotations);
    }
}

fn signature(
    ns: &str,
    name: &str,
    parameters: &[csdl::Parameter],
    aliases: &HashMap<String, String>,
) -> String {
    let types = parameters
        .iter()
        .map(|p| {
            let name = qualify_name(p.type_name.as_deref().unwrap_or("Edm.String"), ns, aliases)
                .unwrap()
                .into_owned();
            if p.is_collection {
                format!("Collection({name})")
            } else {
                name
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{ns}.{name}({types})")
}

impl Scope<'_> {
    fn bindings(&self, expression: Option<&Expr>, ns: &str) -> AnnotationExpressionBindings {
        let mut out = AnnotationExpressionBindings::default();
        if let Some(expression) = expression {
            self.bind_expression(expression, ns, &mut out);
        }
        out
    }

    fn bind_expression(&self, expression: &Expr, ns: &str, out: &mut AnnotationExpressionBindings) {
        let type_name = match expression {
            Expr::Record { type_, .. } => type_.as_ref(),
            Expr::Cast { type_, .. } | Expr::IsOf { type_, .. } => Some(type_),
            _ => None,
        };
        if let Some(name) = type_name {
            let inner = name
                .strip_prefix("Collection(")
                .and_then(|s| s.strip_suffix(')'));
            let canonical = self.canonical(inner.unwrap_or(name), ns);
            let ty = resolve_primitive(&canonical)
                .map(TermType::Primitive)
                .or_else(|| BuiltinType::parse(&canonical).map(TermType::Builtin))
                .or_else(|| match canonical.as_str() {
                    "Edm.EntityType" => Some(TermType::Entity(abstract_entity_type().clone())),
                    "Edm.ComplexType" => Some(TermType::Complex(abstract_complex_type().clone())),
                    _ => self.roots.get(&canonical).and_then(|roots| {
                        roots.iter().find_map(|root| {
                            let AnnotationTarget::Element(e) = root else {
                                return None;
                            };
                            Some(match e.as_ref() {
                                SchemaElement::EntityType(t) => TermType::Entity(t.clone()),
                                SchemaElement::ComplexType(t) => TermType::Complex(t.clone()),
                                SchemaElement::EnumType(t) => TermType::Enum(t.clone()),
                                SchemaElement::TypeDefinition(t) => {
                                    TermType::TypeDefinition(t.clone())
                                }
                                _ => return None,
                            })
                        })
                    }),
                });
            if let Some(ty) = ty {
                out.types.insert(name.clone(), (ty, inner.is_some()));
            }
        }
        match expression {
            Expr::Apply { function, .. } => {
                let candidates = self
                    .roots
                    .get(&self.canonical(function, ns))
                    .into_iter()
                    .flatten()
                    .filter_map(|r| {
                        let AnnotationTarget::Element(e) = r else {
                            return None;
                        };
                        if let SchemaElement::Function(f) = e.as_ref() {
                            Some(f.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                out.functions.insert(function.clone(), candidates);
            }
            Expr::EnumMember(value) => {
                let members = value
                    .split_whitespace()
                    .map(|s| {
                        let (type_name, member) = s.rsplit_once('/')?;
                        self.roots
                            .get(&self.canonical(type_name, ns))?
                            .iter()
                            .find_map(|r| {
                                let AnnotationTarget::Element(e) = r else {
                                    return None;
                                };
                                let SchemaElement::EnumType(t) = e.as_ref() else {
                                    return None;
                                };
                                Some((
                                    t.clone(),
                                    t.members.iter().find(|m| m.name == member)?.clone(),
                                ))
                            })
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(members) = members {
                    out.enum_members.insert(value.clone(), members);
                }
            }
            Expr::LabeledElement { name, .. } | Expr::LabeledElementReference(name) => {
                out.labels.insert(name.clone(), self.canonical(name, ns));
            }
            _ => {}
        }
        match expression {
            Expr::Annotated { expression, .. }
            | Expr::Faceted { expression, .. }
            | Expr::Not(expression)
            | Expr::Neg(expression)
            | Expr::UrlRef(expression)
            | Expr::Cast {
                expr: expression, ..
            }
            | Expr::IsOf {
                expr: expression, ..
            }
            | Expr::LabeledElement {
                expr: expression, ..
            } => self.bind_expression(expression, ns, out),
            Expr::BinaryExpression { lhs, rhs, .. } => {
                self.bind_expression(lhs, ns, out);
                self.bind_expression(rhs, ns, out);
            }
            Expr::If { test, then_, else_ } => {
                self.bind_expression(test, ns, out);
                self.bind_expression(then_, ns, out);
                if let Some(e) = else_ {
                    self.bind_expression(e, ns, out);
                }
            }
            Expr::Apply { args, .. } | Expr::Collection(args) => {
                for e in args {
                    self.bind_expression(e, ns, out);
                }
            }
            Expr::Record { properties, .. } => {
                for p in properties {
                    if let Some(e) = &p.value {
                        self.bind_expression(e, ns, out);
                    }
                }
            }
            _ => {}
        }
    }

    fn canonical(&self, name: &str, ns: &str) -> String {
        if let Some((operation, args)) = name.split_once('(') {
            // Commas cannot occur in CSDL type names; Collection has no commas.
            let operation = qualify_name(operation, ns, self.aliases).unwrap();
            let Some(args) = args.strip_suffix(')') else {
                return name.into();
            };
            let args = if args.is_empty() {
                String::new()
            } else {
                args.split(',')
                    .map(|t| {
                        if let Some(t) = t
                            .strip_prefix("Collection(")
                            .and_then(|t| t.strip_suffix(')'))
                        {
                            format!("Collection({})", qualify_name(t, ns, self.aliases).unwrap())
                        } else {
                            qualify_name(t, ns, self.aliases).unwrap().into_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            };
            format!("{operation}({args})")
        } else {
            qualify_name(name, ns, self.aliases).unwrap().into_owned()
        }
    }

    fn paths(&self, text: &str, ns: &str) -> Vec<Path> {
        let mut parts = text.split('/');
        let root = parts.next().unwrap_or("");
        let name = self.canonical(root, ns);
        let roots = self.roots.get(root).or_else(|| self.roots.get(&name));
        let Some(roots) = roots else {
            return vec![vec![AnnotationTarget::Unresolved(text.into())].into()];
        };
        let mut paths = roots.iter().map(|r| vec![r.clone()]).collect::<Vec<_>>();
        for part in parts {
            for path in &mut paths {
                let next = self.child(path.last().unwrap(), part, ns);
                path.push(next);
            }
        }
        if paths.iter().any(|p| {
            !p.iter()
                .any(|s| matches!(s, AnnotationTarget::Unresolved(_)))
        }) {
            paths.retain(|p| {
                !p.iter()
                    .any(|s| matches!(s, AnnotationTarget::Unresolved(_)))
            });
        }
        paths.into_iter().map(Arc::from).collect()
    }

    fn child(&self, parent: &AnnotationTarget, name: &str, ns: &str) -> AnnotationTarget {
        use AnnotationTarget as T;
        if let Some(term) = name.strip_prefix('@') {
            let (term, qualifier) = term
                .split_once('#')
                .map(|(t, q)| (t, Some(q.to_owned())))
                .unwrap_or((term, None));
            if let Some(term) = self.terms.get(&self.canonical(term, ns)) {
                return T::AppliedTerm {
                    term: term.clone(),
                    qualifier,
                };
            }
            return T::Unresolved(name.into());
        }
        let mut entity = None;
        let mut complex = None;
        match parent {
            T::Element(element) => match element.as_ref() {
                SchemaElement::EntityType(t) => entity = Some(t.clone()),
                SchemaElement::ComplexType(t) => complex = Some(t.clone()),
                SchemaElement::EnumType(t) => {
                    if let Some(m) = t.members.iter().find(|m| m.name == name) {
                        return T::EnumMember(m.clone());
                    }
                }
                SchemaElement::Function(f) => {
                    return operation_child(element, &f.parameters, f.return_type.is_some(), name);
                }
                SchemaElement::Action(a) => {
                    return operation_child(element, &a.parameters, a.return_type.is_some(), name);
                }
                _ => {}
            },
            T::Property(p) => {
                if let ResolvedType::Complex(t) = &p.ty {
                    complex = Some(t.clone());
                }
            }
            T::NavigationProperty(n) => {
                if name == "$OnDelete" && n.on_delete.is_some() {
                    return T::OnDelete(n.clone());
                }
                if let Some(index) = n
                    .referential_constraints
                    .iter()
                    .position(|c| c.property == name)
                {
                    return T::ReferentialConstraint {
                        navigation: n.clone(),
                        index,
                    };
                }
                entity = n.target.upgrade();
            }
            T::Container(c) => {
                if let Some(r) = c
                    .elements
                    .iter()
                    .find(|r| super::entity_container_element_name(r) == name)
                {
                    return T::Resource(r.clone());
                }
            }
            T::Resource(r) => match r.as_ref() {
                EntityContainerElement::EntitySet(s) => entity = Some(s.target.clone()),
                EntityContainerElement::Singleton(s) => entity = Some(s.target.clone()),
                _ => {}
            },
            _ => {}
        }
        if let Some(t) = entity {
            if let Some(p) = t.properties().iter().find(|p| p.name == name) {
                return T::Property(p.clone());
            }
            if let Some(n) = t.navigation_properties().iter().find(|n| n.name == name) {
                return T::NavigationProperty(n.clone());
            }
            if let Some(casts) = self.roots.get(&self.canonical(name, ns)) {
                for cast in casts {
                    if let T::Element(e) = cast {
                        if let SchemaElement::EntityType(derived) = e.as_ref() {
                            if Arc::ptr_eq(derived, &t) || entity_is_strict_subtype(derived, &t) {
                                return cast.clone();
                            }
                        }
                    }
                }
            }
            if t.is_open {
                return T::DynamicProperty(name.into());
            }
        }
        if let Some(t) = complex {
            if let Some(p) = t.properties().iter().find(|p| p.name == name) {
                return T::Property(p.clone());
            }
            if let Some(n) = t.navigation_properties().iter().find(|n| n.name == name) {
                return T::NavigationProperty(n.clone());
            }
            if let Some(casts) = self.roots.get(&self.canonical(name, ns)) {
                for cast in casts {
                    if let T::Element(e) = cast {
                        if let SchemaElement::ComplexType(derived) = e.as_ref() {
                            if Arc::ptr_eq(derived, &t) || complex_is_strict_subtype(derived, &t) {
                                return cast.clone();
                            }
                        }
                    }
                }
            }
            if t.is_open {
                return T::DynamicProperty(name.into());
            }
        }
        T::Unresolved(name.into())
    }

    fn attach(
        &self,
        ns: &str,
        owner: &str,
        source: &[csdl::Annotation],
        output: &mut Vec<ResolvedAnnotation>,
    ) {
        for annotation in source {
            let target_path = annotation
                .target
                .as_ref()
                .map(|target| {
                    if owner == ns {
                        format!("{ns}.{target}")
                    } else {
                        format!("{owner}/{target}")
                    }
                })
                .unwrap_or_else(|| owner.to_owned());
            let targets = self.paths(&target_path, ns);
            let term_name = self.canonical(&annotation.term, ns);
            let mut expression_paths = Vec::new();
            if let Some(expression) = &annotation.expression {
                for target in &targets {
                    let context = if matches!(
                        target.last(),
                        Some(
                            AnnotationTarget::Property(_) | AnnotationTarget::NavigationProperty(_)
                        )
                    ) {
                        &target[..target.len() - 1]
                    } else {
                        target.as_ref()
                    };
                    self.expression_paths(expression, ns, context, &mut expression_paths);
                }
            }
            let applied_target = target_path.clone();
            output.push(ResolvedAnnotation {
                term: self.terms.get(&term_name).cloned(),
                term_name: term_name.clone(),
                qualifier: annotation.qualifier.clone(),
                target_path,
                targets,
                expression: annotation.expression.clone().or_else(|| {
                    self.terms
                        .get(&term_name)
                        .and_then(|term| term.default_expression())
                }),
                expression_paths,
                expression_bindings: self.bindings(annotation.expression.as_ref(), ns),
            });
            if let Some(expression) = &annotation.expression {
                let resolved = output.last().unwrap();
                let anchor = resolved
                    .targets
                    .iter()
                    .map(|target| {
                        let mut path = target.to_vec();
                        path.push(match &resolved.term {
                            Some(term) => AnnotationTarget::AppliedTerm {
                                term: term.clone(),
                                qualifier: annotation.qualifier.clone(),
                            },
                            None => AnnotationTarget::Unresolved(annotation.term.clone()),
                        });
                        Arc::from(path)
                    })
                    .collect::<Vec<Path>>();
                self.nested(
                    expression,
                    ns,
                    &resolved.target_path.clone(),
                    &anchor,
                    &mut 0,
                    output,
                );
            }
            if !annotation.annotations.is_empty() {
                let mut applied = format!("{applied_target}/@{term_name}");
                if let Some(q) = &annotation.qualifier {
                    applied.push('#');
                    applied.push_str(q);
                }
                self.attach(ns, &applied, &annotation.annotations, output);
            }
        }
    }

    fn nested(
        &self,
        expression: &Expr,
        ns: &str,
        owner: &str,
        anchor: &[Path],
        ordinal: &mut usize,
        out: &mut Vec<ResolvedAnnotation>,
    ) {
        match expression {
            Expr::Annotated {
                expression,
                annotations,
            } => {
                self.expression_annotations(
                    annotations,
                    AnnotationTargetKind::parse(expression.kind_name()),
                    ns,
                    owner,
                    anchor,
                    ordinal,
                    out,
                );
                self.nested(expression, ns, owner, anchor, ordinal, out);
            }
            Expr::Faceted { expression, .. } | Expr::Neg(expression) => {
                self.nested(expression, ns, owner, anchor, ordinal, out)
            }
            Expr::Record {
                annotations,
                properties,
                ..
            } => {
                self.expression_annotations(
                    annotations,
                    AnnotationTargetKind::Record,
                    ns,
                    owner,
                    anchor,
                    ordinal,
                    out,
                );
                for property in properties {
                    self.expression_annotations(
                        &property.annotations,
                        AnnotationTargetKind::PropertyValue,
                        ns,
                        owner,
                        anchor,
                        ordinal,
                        out,
                    );
                    if let Some(value) = &property.value {
                        self.nested(value, ns, owner, anchor, ordinal, out);
                    }
                }
            }
            Expr::Collection(values) | Expr::Apply { args: values, .. } => {
                for value in values {
                    self.nested(value, ns, owner, anchor, ordinal, out);
                }
            }
            Expr::Not(value)
            | Expr::UrlRef(value)
            | Expr::Cast { expr: value, .. }
            | Expr::IsOf { expr: value, .. }
            | Expr::LabeledElement { expr: value, .. } => {
                self.nested(value, ns, owner, anchor, ordinal, out)
            }
            Expr::BinaryExpression { lhs, rhs, .. } => {
                self.nested(lhs, ns, owner, anchor, ordinal, out);
                self.nested(rhs, ns, owner, anchor, ordinal, out);
            }
            Expr::If { test, then_, else_ } => {
                self.nested(test, ns, owner, anchor, ordinal, out);
                self.nested(then_, ns, owner, anchor, ordinal, out);
                if let Some(value) = else_ {
                    self.nested(value, ns, owner, anchor, ordinal, out);
                }
            }
            _ => {}
        }
    }

    fn expression_annotations(
        &self,
        annotations: &[csdl::Annotation],
        kind: AnnotationTargetKind,
        ns: &str,
        owner: &str,
        anchor: &[Path],
        ordinal: &mut usize,
        out: &mut Vec<ResolvedAnnotation>,
    ) {
        let index = *ordinal;
        *ordinal += 1;
        for annotation in annotations {
            let term_name = self.canonical(&annotation.term, ns);
            let targets = anchor
                .iter()
                .map(|anchor| {
                    let mut path = anchor.to_vec();
                    if kind != AnnotationTargetKind::Annotation {
                        path.push(AnnotationTarget::Expression {
                            kind: kind.clone(),
                            ordinal: index,
                        });
                    }
                    Arc::from(path)
                })
                .collect::<Vec<Path>>();
            let mut expression_paths = Vec::new();
            if let Some(expression) = &annotation.expression {
                for target in anchor {
                    let end = target
                        .iter()
                        .position(|s| {
                            matches!(
                                s,
                                AnnotationTarget::AppliedTerm { .. }
                                    | AnnotationTarget::Expression { .. }
                            )
                        })
                        .unwrap_or(target.len());
                    let mut context = &target[..end];
                    if matches!(
                        context.last(),
                        Some(
                            AnnotationTarget::Property(_) | AnnotationTarget::NavigationProperty(_)
                        )
                    ) {
                        context = &context[..context.len() - 1];
                    }
                    self.expression_paths(expression, ns, context, &mut expression_paths);
                }
            }
            let applied = targets
                .iter()
                .map(|target| {
                    let mut path = target.to_vec();
                    path.push(
                        self.terms
                            .get(&term_name)
                            .map(|term| AnnotationTarget::AppliedTerm {
                                term: term.clone(),
                                qualifier: annotation.qualifier.clone(),
                            })
                            .unwrap_or_else(|| AnnotationTarget::Unresolved(term_name.clone())),
                    );
                    Arc::from(path)
                })
                .collect::<Vec<Path>>();
            out.push(ResolvedAnnotation {
                term: self.terms.get(&term_name).cloned(),
                term_name: term_name.clone(),
                qualifier: annotation.qualifier.clone(),
                target_path: format!("{owner}/expression[{index}]"),
                targets,
                expression: annotation.expression.clone().or_else(|| {
                    self.terms
                        .get(&term_name)
                        .and_then(|term| term.default_expression())
                }),
                expression_paths,
                expression_bindings: self.bindings(annotation.expression.as_ref(), ns),
            });
            if let Some(expression) = &annotation.expression {
                self.nested(expression, ns, owner, &applied, ordinal, out);
            }
            if !annotation.annotations.is_empty() {
                self.expression_annotations(
                    &annotation.annotations,
                    AnnotationTargetKind::Annotation,
                    ns,
                    owner,
                    &applied,
                    ordinal,
                    out,
                );
            }
        }
    }

    fn members(
        &self,
        ns: &str,
        owner: &str,
        properties: &[csdl::Property],
        navigations: &[csdl::NavigationProperty],
        out: &mut Vec<ResolvedAnnotation>,
    ) {
        for p in properties {
            self.attach(ns, &format!("{owner}/{}", p.name), &p.annotations, out);
        }
        for n in navigations {
            let path = format!("{owner}/{}", n.name);
            self.attach(ns, &path, &n.annotations, out);
            for c in &n.referential_constraints {
                self.attach(ns, &format!("{path}/{}", c.property), &c.annotations, out);
            }
        }
    }

    fn operation(
        &self,
        ns: &str,
        name: &str,
        parameters: &[csdl::Parameter],
        return_type: Option<&csdl::ReturnType>,
        annotations: &[csdl::Annotation],
        out: &mut Vec<ResolvedAnnotation>,
    ) {
        let owner = signature(ns, name, parameters, self.aliases);
        self.attach(ns, &owner, annotations, out);
        for p in parameters {
            self.attach(ns, &format!("{owner}/{}", p.name), &p.annotations, out);
        }
        if let Some(r) = return_type {
            self.attach(ns, &format!("{owner}/$ReturnType"), &r.annotations, out);
        }
    }

    fn expression_paths(
        &self,
        expression: &Expr,
        ns: &str,
        context: &[AnnotationTarget],
        output: &mut Vec<AnnotationExpressionPath>,
    ) {
        use AnnotationPathKind as K;
        let path = match expression {
            Expr::ModelElementPath(p) => Some((p, K::ModelElement)),
            Expr::Path(p) => Some((p, K::Value)),
            Expr::PropertyPath(p) => Some((p, K::Property)),
            Expr::NavigationPropertyPath(p) => Some((p, K::NavigationProperty)),
            Expr::AnnotationPath(p) => Some((p, K::Annotation)),
            _ => None,
        };
        if let Some((path, kind)) = path {
            if path.starts_with('/') {
                for segments in self.paths(path.trim_start_matches('/'), ns) {
                    output.push(AnnotationExpressionPath {
                        source: path.clone(),
                        kind,
                        segments,
                    });
                }
                return;
            }
            let mut segments = context.to_vec();
            for part in path.split('/') {
                let next = segments
                    .last()
                    .map(|last| self.child(last, part, ns))
                    .unwrap_or_else(|| AnnotationTarget::Unresolved(part.into()));
                segments.push(next);
            }
            output.push(AnnotationExpressionPath {
                source: path.clone(),
                kind,
                segments: segments.into(),
            });
        }
        match expression {
            Expr::Annotated {
                expression,
                annotations,
            } => {
                self.expression_paths(expression, ns, context, output);
                for a in annotations {
                    if let Some(e) = &a.expression {
                        self.expression_paths(e, ns, context, output);
                    }
                }
            }
            Expr::Faceted { expression, .. } | Expr::Neg(expression) => {
                self.expression_paths(expression, ns, context, output)
            }
            Expr::Not(e)
            | Expr::UrlRef(e)
            | Expr::Cast { expr: e, .. }
            | Expr::IsOf { expr: e, .. }
            | Expr::LabeledElement { expr: e, .. } => self.expression_paths(e, ns, context, output),
            Expr::BinaryExpression { lhs, rhs, .. } => {
                self.expression_paths(lhs, ns, context, output);
                self.expression_paths(rhs, ns, context, output);
            }
            Expr::If { test, then_, else_ } => {
                self.expression_paths(test, ns, context, output);
                self.expression_paths(then_, ns, context, output);
                if let Some(e) = else_ {
                    self.expression_paths(e, ns, context, output);
                }
            }
            Expr::Apply { args, .. } | Expr::Collection(args) => {
                for e in args {
                    self.expression_paths(e, ns, context, output);
                }
            }
            Expr::Record { properties, .. } => {
                for p in properties {
                    if let Some(e) = &p.value {
                        self.expression_paths(e, ns, context, output);
                    }
                }
            }
            _ => {}
        }
    }
}

fn operation_child(
    operation: &Arc<SchemaElement>,
    parameters: &[OperationParameter],
    has_return: bool,
    name: &str,
) -> AnnotationTarget {
    if name == "$ReturnType" && has_return {
        return AnnotationTarget::ReturnType(operation.clone());
    }
    parameters
        .iter()
        .position(|p| p.name == name)
        .map(|index| AnnotationTarget::Parameter {
            operation: operation.clone(),
            index,
        })
        .unwrap_or_else(|| AnnotationTarget::Unresolved(name.into()))
}
