use super::*;

/// Split only outside quoted literals and balanced key predicates.
fn split(source: &str, separator: char) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let (mut start, mut depth, mut quoted) = (0, 0usize, false);
    for (index, ch) in source.char_indices() {
        if ch == '\'' {
            quoted = !quoted;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            _ if ch == separator && depth == 0 => {
                parts.push(&source[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if quoted || depth != 0 {
        return None;
    }
    parts.push(&source[start..]);
    Some(parts)
}

pub(super) fn collection(target: &AnnotationTarget) -> bool {
    match target {
        AnnotationTarget::Property(p) => p.is_collection,
        AnnotationTarget::NavigationProperty(n) => n.is_collection,
        AnnotationTarget::Resource(r) => matches!(r.as_ref(), EntityContainerElement::EntitySet(_)),
        AnnotationTarget::AppliedTerm { term, .. } => term.is_collection,
        AnnotationTarget::Parameter { operation, index } => match operation.as_ref() {
            SchemaElement::Function(f) => f.parameters.get(*index).is_some_and(|p| p.is_collection),
            SchemaElement::Action(a) => a.parameters.get(*index).is_some_and(|p| p.is_collection),
            _ => false,
        },
        AnnotationTarget::ReturnType(operation) => match operation.as_ref() {
            SchemaElement::Function(f) => f.return_type.as_ref().is_some_and(|r| r.is_collection),
            SchemaElement::Action(a) => a.return_type.as_ref().is_some_and(|r| r.is_collection),
            _ => false,
        },
        _ => false,
    }
}

impl Scope<'_> {
    pub(super) fn resolve_expression_path(
        &self,
        source: &str,
        kind: AnnotationPathKind,
        ns: &str,
        context: &[AnnotationTarget],
        depth: usize,
    ) -> Vec<AnnotationExpressionPath> {
        let invalid = || {
            vec![AnnotationExpressionPath {
                source: source.into(),
                kind,
                is_collection: false,
                is_ordered: false,
                segments: vec![AnnotationTarget::Unresolved(source.into())].into(),
            }]
        };
        if depth > 32 {
            return invalid();
        }
        let absolute = source.starts_with('/');
        let Some(mut parts) = split(if absolute { &source[1..] } else { source }, '/') else {
            return invalid();
        };
        let mut starts = vec![context.to_vec()];
        if absolute {
            let root = parts.remove(0);
            let canonical = self.canonical(root, ns);
            let Some(roots) = self.roots.get(&canonical).or_else(|| self.roots.get(root)) else {
                return invalid();
            };
            starts = roots.iter().map(|r| vec![r.clone()]).collect();
        } else if source.is_empty() {
            parts.clear();
        }
        starts.into_iter().flat_map(|mut segments| {
            let mut ordered = segments.last().is_some_and(|t| matches!(t, AnnotationTarget::Property(p) if self.ordered.contains(&(Arc::as_ptr(p) as usize))));
            let mut unselected = usize::from(segments.last().is_some_and(collection));
            for (index, part) in parts.iter().enumerate() {
                let Some(parent) = segments.last().cloned() else { segments.push(AnnotationTarget::Unresolved((*part).into())); break; };
                if let AnnotationTarget::Resource(r) = &parent {
                    if let EntityContainerElement::FunctionImport(import) = r.as_ref() {
                        if !import.overloads.is_empty() {
                            let remaining = parts[index..].join("/");
                            return import.overloads.iter().flat_map(|f| {
                                let mut context = segments.clone();
                                context.push(AnnotationTarget::Element(Arc::new(SchemaElement::Function(f.clone()))));
                                self.resolve_expression_path(&remaining, kind, ns, &context, depth + 1)
                            }).map(|mut p| { p.source = source.into(); p }).collect::<Vec<_>>();
                        }
                    }
                }
                let container_start = index == 0 && matches!(parent, AnnotationTarget::Container(_));
                let mut next = if *part == "$count" {
                    if kind == AnnotationPathKind::Value && collection(&parent) && unselected == 1 && index + 1 == parts.len() {
                        unselected = unselected.saturating_sub(1);
                        AnnotationTarget::Count
                    } else { AnnotationTarget::Unresolved((*part).into()) }
                } else if let Ok(i) = part.parse::<i64>() {
                    if kind == AnnotationPathKind::Value && matches!(&parent, AnnotationTarget::Property(p) if p.is_collection && self.ordered.contains(&(Arc::as_ptr(p) as usize))) {
                        unselected = unselected.saturating_sub(1);
                        AnnotationTarget::SelectedItem { collection: Box::new(parent), selector: PathSelector::Index(i) }
                    } else { AnnotationTarget::Unresolved((*part).into()) }
                } else if let Some((name, predicate)) = part.split_once('(') {
                    let child = self.child(&parent, name, ns);
                    if kind == AnnotationPathKind::Value {
                        predicate.strip_suffix(')').and_then(|p| self.key_selector(&child, p, ns, context, depth + 1))
                            .map(|selector| AnnotationTarget::SelectedItem { collection: Box::new(child), selector })
                            .unwrap_or_else(|| AnnotationTarget::Unresolved((*part).into()))
                    } else { AnnotationTarget::Unresolved((*part).into()) }
                } else if let Some((property, term)) = part.split_once('@').filter(|(p, _)| !p.is_empty()) {
                    let child = self.child(&parent, property, ns);
                    if kind != AnnotationPathKind::Value && matches!(child, AnnotationTarget::NavigationProperty(_)) {
                        segments.push(child.clone());
                        self.child(&child, &format!("@{term}"), ns)
                    } else { AnnotationTarget::Unresolved((*part).into()) }
                } else if part.is_empty() { AnnotationTarget::Unresolved(String::new()) }
                else { self.child(&parent, part, ns) };
                if container_start
                    && !matches!(&next, AnnotationTarget::Resource(_))
                    && !matches!(&next, AnnotationTarget::SelectedItem { collection, .. }
                        if matches!(collection.as_ref(), AnnotationTarget::Resource(_)))
                {
                    next = AnnotationTarget::Unresolved((*part).into());
                }
                if collection(&next) { unselected += 1; ordered = matches!(&next, AnnotationTarget::Property(p) if self.ordered.contains(&(Arc::as_ptr(p) as usize))); }
                segments.push(next);
            }
            if kind == AnnotationPathKind::Value && unselected > 1 {
                segments.push(AnnotationTarget::Unresolved("Multiple unselected collection segments".into()));
            }
            vec![AnnotationExpressionPath { is_ordered: ordered && unselected > 0, source: source.into(), kind, is_collection: kind == AnnotationPathKind::Value && unselected > 0, segments: segments.into() }]
        }).collect()
    }

    fn key_selector(
        &self,
        target: &AnnotationTarget,
        source: &str,
        ns: &str,
        context: &[AnnotationTarget],
        depth: usize,
    ) -> Option<PathSelector> {
        let entity = match target {
            AnnotationTarget::NavigationProperty(n) if n.is_collection => n.target.upgrade()?,
            AnnotationTarget::Resource(r) => match r.as_ref() {
                EntityContainerElement::EntitySet(s) => s.target.clone(),
                _ => return None,
            },
            _ => return None,
        };
        let keys = entity.keys();
        let args = split(source, ',')?;
        if args.len() != keys.len() || keys.is_empty() {
            return None;
        }
        let mut used = std::collections::HashSet::new();
        let mut values = Vec::new();
        for arg in args {
            let assignment = split(arg, '=')?;
            let (key_index, value) = if let [name, value] = assignment.as_slice() {
                let index = keys.iter().enumerate().position(|(i, key)| {
                    entity
                        .key_aliases
                        .get()
                        .and_then(|a| a.get(i))
                        .and_then(Option::as_deref)
                        .map(|alias| alias == *name)
                        .unwrap_or_else(|| key.last().is_some_and(|s| s.display_name() == *name))
                })?;
                (index, *value)
            } else if assignment.len() == 1 && keys.len() == 1 {
                (0, arg)
            } else {
                return None;
            };
            if !used.insert(key_index) {
                return None;
            }
            let KeyPathSegment::Property(property) = keys[key_index].last()? else {
                return None;
            };
            let property = property.upgrade()?;
            let literal =
                if let Some(s) = value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
                    if s.replace("''", "").contains('\'') {
                        return None;
                    }
                    Some(Expr::String(s.replace("''", "'")))
                } else if let Ok(n) = value.parse::<i64>() {
                    Some(Expr::Int(n))
                } else if value == "true" || value == "false" {
                    Some(Expr::Bool(value == "true"))
                } else if value.parse::<f64>().is_ok() {
                    Some(Expr::Decimal(value.into()))
                } else if value.starts_with(|c: char| c.is_ascii_digit() || c == '-')
                    || value.starts_with("duration'")
                    || value.starts_with("binary'")
                {
                    let primitive = match &property.ty {
                        ResolvedType::Primitive(p) => p,
                        ResolvedType::TypeDefinition(t) => &t.underlying_type,
                        _ => return None,
                    };
                    let value = value
                        .strip_prefix("duration'")
                        .or_else(|| value.strip_prefix("binary'"))
                        .and_then(|s| s.strip_suffix('\''))
                        .unwrap_or(value);
                    Some(crate::expr::default_literal(
                        &format!("Edm.{primitive:?}"),
                        Some(value),
                    ))
                } else {
                    None
                };
            let value = if let Some(literal) = literal {
                PathKeyValue::Literal(literal)
            } else {
                let mut paths = self.resolve_expression_path(
                    value,
                    AnnotationPathKind::Value,
                    ns,
                    context,
                    depth,
                );
                if paths.len() != 1 {
                    return None;
                }
                let path = paths.remove(0);
                if path.is_collection
                    || path
                        .segments
                        .iter()
                        .any(|s| matches!(s, AnnotationTarget::Unresolved(_)))
                {
                    return None;
                }
                PathKeyValue::Path(path.segments)
            };
            values.push(PathKeyArgument { property, value });
        }
        Some(PathSelector::Key(values))
    }
}
