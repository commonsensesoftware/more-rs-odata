use super::*;

pub(super) fn normalize_xml_spatial(edmx: &mut Edmx) {
    normalize_annotations(edmx, true);
    let entities = edmx
        .schemas
        .iter()
        .flat_map(|s| {
            s.elements.iter().filter_map(move |e| {
                if let SchemaElement::EntityType(t) = e {
                    Some(format!("{}.{}", s.namespace, t.name))
                } else {
                    None
                }
            })
        })
        .collect::<std::collections::HashSet<_>>();
    let mut definitions = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for schema in &edmx.schemas {
        if let Some(alias) = &schema.alias {
            aliases.insert(alias.clone(), schema.namespace.clone());
        }
        for element in &schema.elements {
            if let SchemaElement::TypeDefinition(t) = element {
                definitions.insert(
                    format!("{}.{}", schema.namespace, t.name),
                    (t.underlying_type.clone(), t.srid),
                );
            }
        }
    }
    for schema in &mut edmx.schemas {
        for element in &mut schema.elements {
            let rt = match element {
                SchemaElement::Function(f) => f.return_type.as_mut(),
                SchemaElement::Action(a) => a.return_type.as_mut(),
                _ => None,
            };
            if let Some(rt) = rt {
                let name = rt.type_name.as_deref().unwrap_or("Edm.String");
                let name = name
                    .rsplit_once('.')
                    .map(|(ns, n)| {
                        format!("{}.{n}", aliases.get(ns).map(String::as_str).unwrap_or(ns))
                    })
                    .unwrap_or_else(|| format!("{}.{name}", schema.namespace));
                if !(rt.is_collection && (entities.contains(&name) || name == "Edm.EntityType")) {
                    rt.nullable = rt.nullable.or(rt.nullable_default);
                }
            }
        }
        let normalize =
            |name: Option<&str>, srid: Option<SridFacet>, default: &mut Option<String>| {
                let Some(value) = default.as_ref().filter(|v| v.starts_with('{')) else {
                    return;
                };
                let name = name.unwrap_or("Edm.String");
                let name = match name.rsplit_once('.') {
                    Some((prefix, short)) => format!(
                        "{}.{short}",
                        aliases.get(prefix).map(String::as_str).unwrap_or(prefix)
                    ),
                    None => format!("{}.{name}", schema.namespace),
                };
                let (ty, srid) = definitions
                    .get(&name)
                    .map(|(ty, inherited)| (ty.as_str(), inherited.or(srid)))
                    .unwrap_or((&name, srid));
                if !ty.starts_with("Edm.Geography") && !ty.starts_with("Edm.Geometry") {
                    return;
                }
                let srid = match srid {
                    Some(SridFacet::Number(n)) => n,
                    _ => {
                        if ty.starts_with("Edm.Geography") {
                            4326
                        } else {
                            0
                        }
                    }
                };
                if let Some(spatial) = crate::spatial::SpatialValue::parse(value, srid) {
                    *default = Some(spatial.to_wkt());
                }
            };
        for element in &mut schema.elements {
            match element {
                SchemaElement::EntityType(t) => {
                    for p in &mut t.properties {
                        normalize(p.type_name.as_deref(), p.srid, &mut p.default_value);
                    }
                }
                SchemaElement::ComplexType(t) => {
                    for p in &mut t.properties {
                        normalize(p.type_name.as_deref(), p.srid, &mut p.default_value);
                    }
                }
                SchemaElement::Term(t) => {
                    normalize(t.type_name.as_deref(), t.srid, &mut t.default_value)
                }
                SchemaElement::Function(f) => {
                    for p in &mut f.parameters {
                        normalize(p.type_name.as_deref(), p.srid, &mut p.default_value);
                    }
                }
                SchemaElement::Action(a) => {
                    for p in &mut a.parameters {
                        normalize(p.type_name.as_deref(), p.srid, &mut p.default_value);
                    }
                }
                _ => {}
            }
        }
    }
}

pub(super) fn materialize_annotations(edmx: &mut Edmx) {
    normalize_annotations(edmx, false);
}

fn normalize_annotations(edmx: &mut Edmx, xml: bool) {
    let mut terms = BTreeMap::new();
    let mut types = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for schema in &edmx.schemas {
        if let Some(alias) = &schema.alias {
            aliases.insert(alias.clone(), schema.namespace.clone());
        }
        for element in &schema.elements {
            match element {
                SchemaElement::Term(t) => {
                    terms.insert(format!("{}.{}", schema.namespace, t.name), t.clone());
                }
                SchemaElement::TypeDefinition(t) => {
                    types.insert(
                        format!("{}.{}", schema.namespace, t.name),
                        t.underlying_type.clone(),
                    );
                }
                SchemaElement::ComplexType(t) => {
                    types.insert(format!("{}.{}", schema.namespace, t.name), "Complex".into());
                }
                SchemaElement::EntityType(t) => {
                    types.insert(format!("{}.{}", schema.namespace, t.name), "Complex".into());
                }
                SchemaElement::EnumType(t) => {
                    let name = format!("{}.{}", schema.namespace, t.name);
                    types.insert(name.clone(), format!("Enum:{name}"));
                }
                _ => {}
            }
        }
    }
    fn qualified(name: &str, ns: &str, aliases: &BTreeMap<String, String>) -> String {
        match name.rsplit_once('.') {
            Some((prefix, short)) => format!(
                "{}.{short}",
                aliases.get(prefix).map(String::as_str).unwrap_or(prefix)
            ),
            None => format!("{ns}.{name}"),
        }
    }
    for schema in &mut edmx.schemas {
        let ns = schema.namespace.clone();
        let materialize = |annotation: &mut Annotation| {
            if xml {
                if let (Some(term), Some(CsdlAnnotationExpression::String(value))) = (
                    terms.get(&qualified(&annotation.term, &ns, &aliases)),
                    &annotation.expression,
                ) {
                    let name = qualified(
                        term.type_name.as_deref().unwrap_or("Edm.String"),
                        &ns,
                        &aliases,
                    );
                    let underlying = types.get(&name).map(String::as_str).unwrap_or(&name);
                    if let Some(enum_name) = underlying.strip_prefix("Enum:") {
                        if value
                            .split(',')
                            .all(|v| crate::conformance::is_simple_identifier(v.trim()))
                        {
                            annotation.expression = Some(CsdlAnnotationExpression::EnumMember(
                                value
                                    .split(',')
                                    .map(|v| format!("{enum_name}/{}", v.trim()))
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            ));
                        }
                    } else if underlying.starts_with("Edm.") {
                        annotation.expression =
                            Some(crate::expr::default_literal(underlying, Some(value)));
                    }
                }
            } else if annotation.expression.is_none() {
                let term = terms.get(&qualified(&annotation.term, &ns, &aliases));
                annotation.expression = Some(match term {
                    Some(t) if t.is_collection => CsdlAnnotationExpression::Collection(Vec::new()),
                    Some(t) => {
                        let name = qualified(
                            t.type_name.as_deref().unwrap_or("Edm.String"),
                            &ns,
                            &aliases,
                        );
                        let underlying = types.get(&name).map(String::as_str).unwrap_or(&name);
                        if underlying == "Complex" {
                            CsdlAnnotationExpression::Record {
                                type_: None,
                                properties: Vec::new(),
                                annotations: Vec::new(),
                            }
                        } else {
                            crate::expr::default_literal(underlying, t.default_value.as_deref())
                        }
                    }
                    None => CsdlAnnotationExpression::Null,
                });
            }
        };
        let apply =
            |annotations: &mut Vec<Annotation>| visit_annotations(annotations, &materialize);
        apply(&mut schema.annotations);
        for group in &mut schema.external_annotations {
            apply(&mut group.annotations);
        }
        for element in &mut schema.elements {
            match element {
                SchemaElement::EntityType(t) => {
                    apply(&mut t.annotations);
                    for p in &mut t.properties {
                        apply(&mut p.annotations);
                    }
                    for n in &mut t.navigation_properties {
                        apply(&mut n.annotations);
                        for c in &mut n.referential_constraints {
                            apply(&mut c.annotations);
                        }
                    }
                }
                SchemaElement::ComplexType(t) => {
                    apply(&mut t.annotations);
                    for p in &mut t.properties {
                        apply(&mut p.annotations);
                    }
                    for n in &mut t.navigation_properties {
                        apply(&mut n.annotations);
                        for c in &mut n.referential_constraints {
                            apply(&mut c.annotations);
                        }
                    }
                }
                SchemaElement::EnumType(t) => {
                    apply(&mut t.annotations);
                    for m in &mut t.members {
                        apply(&mut m.annotations);
                    }
                }
                SchemaElement::TypeDefinition(t) => apply(&mut t.annotations),
                SchemaElement::Term(t) => apply(&mut t.annotations),
                SchemaElement::Function(t) => {
                    apply(&mut t.annotations);
                    for p in &mut t.parameters {
                        apply(&mut p.annotations);
                    }
                    if let Some(r) = &mut t.return_type {
                        apply(&mut r.annotations);
                    }
                }
                SchemaElement::Action(t) => {
                    apply(&mut t.annotations);
                    for p in &mut t.parameters {
                        apply(&mut p.annotations);
                    }
                    if let Some(r) = &mut t.return_type {
                        apply(&mut r.annotations);
                    }
                }
                SchemaElement::EntityContainer(c) => {
                    apply(&mut c.annotations);
                    for s in &mut c.entity_sets {
                        apply(&mut s.annotations);
                    }
                    for s in &mut c.singletons {
                        apply(&mut s.annotations);
                    }
                    for s in &mut c.function_imports {
                        apply(&mut s.annotations);
                    }
                    for s in &mut c.action_imports {
                        apply(&mut s.annotations);
                    }
                }
            }
        }
    }
}

fn visit_annotations(annotations: &mut [Annotation], apply: &impl Fn(&mut Annotation)) {
    for annotation in annotations {
        apply(annotation);
        visit_annotations(&mut annotation.annotations, apply);
        if let Some(expression) = &mut annotation.expression {
            visit_expression(expression, apply);
        }
    }
}

fn visit_expression(expression: &mut CsdlAnnotationExpression, apply: &impl Fn(&mut Annotation)) {
    use CsdlAnnotationExpression as E;
    match expression {
        E::Annotated {
            expression,
            annotations,
        } => {
            visit_annotations(annotations, apply);
            visit_expression(expression, apply);
        }
        E::Faceted { expression, .. } | E::Neg(expression) => visit_expression(expression, apply),
        E::Record {
            annotations,
            properties,
            ..
        } => {
            visit_annotations(annotations, apply);
            for property in properties {
                visit_annotations(&mut property.annotations, apply);
                if let Some(value) = &mut property.value {
                    visit_expression(value, apply);
                }
            }
        }
        E::Collection(values) | E::Apply { args: values, .. } => {
            for value in values {
                visit_expression(value, apply);
            }
        }
        E::Not(e)
        | E::UrlRef(e)
        | E::Cast { expr: e, .. }
        | E::IsOf { expr: e, .. }
        | E::LabeledElement { expr: e, .. } => visit_expression(e, apply),
        E::BinaryExpression { lhs, rhs, .. } => {
            visit_expression(lhs, apply);
            visit_expression(rhs, apply);
        }
        E::If { test, then_, else_ } => {
            visit_expression(test, apply);
            visit_expression(then_, apply);
            if let Some(e) = else_ {
                visit_expression(e, apply);
            }
        }
        _ => {}
    }
}

/// Convert lexical defaults to their JSON primitive representation, including
/// alias-qualified type definitions, without requiring semantic resolution.
pub(super) fn normalize(out: &mut Map<String, Value>, edmx: &Edmx) {
    let mut definitions = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for schema in &edmx.schemas {
        if let Some(alias) = &schema.alias {
            aliases.insert(alias.clone(), schema.namespace.clone());
        }
        for element in &schema.elements {
            if let SchemaElement::TypeDefinition(t) = element {
                definitions.insert(
                    format!("{}.{}", schema.namespace, t.name),
                    (t.underlying_type.clone(), t.srid),
                );
            }
        }
    }
    for schema in &edmx.schemas {
        if let Some(value) = out.get_mut(&schema.namespace) {
            walk(value, &schema.namespace, &definitions, &aliases);
        }
    }
}

fn walk(
    value: &mut Value,
    ns: &str,
    definitions: &BTreeMap<String, (String, Option<SridFacet>)>,
    aliases: &BTreeMap<String, String>,
) {
    match value {
        Value::Array(values) => {
            for value in values {
                walk(value, ns, definitions, aliases);
            }
        }
        Value::Object(obj) => {
            let name = obj
                .get("$Type")
                .and_then(Value::as_str)
                .unwrap_or("Edm.String");
            let qualified = match name.rsplit_once('.') {
                Some(("self", short)) => format!("{ns}.{short}"),
                Some((prefix, short)) => format!(
                    "{}.{short}",
                    aliases.get(prefix).map(String::as_str).unwrap_or(prefix)
                ),
                None => format!("{ns}.{name}"),
            };
            let ty = definitions
                .get(&qualified)
                .map(|(name, _)| name.as_str())
                .unwrap_or(&qualified);
            if let Some(Value::String(default)) = obj.get("$DefaultValue") {
                let replacement = match ty {
                    "Edm.Boolean" => default.parse::<bool>().ok().map(Value::Bool),
                    "Edm.Byte" | "Edm.SByte" | "Edm.Int16" | "Edm.Int32" | "Edm.Int64" => {
                        default.parse::<i64>().ok().map(|n| Value::Number(n.into()))
                    }
                    // Decimal remains a string to retain arbitrary precision.
                    "Edm.Single" | "Edm.Double" => default
                        .parse::<f64>()
                        .ok()
                        .and_then(Number::from_f64)
                        .map(Value::Number),
                    spatial
                        if spatial.starts_with("Edm.Geography")
                            || spatial.starts_with("Edm.Geometry") =>
                    {
                        let inherited =
                            definitions
                                .get(&qualified)
                                .and_then(|(_, srid)| match srid {
                                    Some(SridFacet::Number(n)) => Some(*n),
                                    _ => None,
                                });
                        let srid = inherited
                            .or_else(|| obj.get("$SRID").and_then(Value::as_u64))
                            .unwrap_or(if spatial.starts_with("Edm.Geography") {
                                4326
                            } else {
                                0
                            });
                        crate::spatial::SpatialValue::parse(default, srid).map(|v| v.to_json(srid))
                    }
                    _ => None,
                };
                if let Some(value) = replacement {
                    obj.insert("$DefaultValue".into(), value);
                }
            }
            for (key, value) in obj {
                if !key.contains('@') && key != "$DefaultValue" {
                    walk(value, ns, definitions, aliases);
                }
            }
        }
        _ => {}
    }
}
