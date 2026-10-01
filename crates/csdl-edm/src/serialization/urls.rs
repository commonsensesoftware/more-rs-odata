use super::*;

pub(super) fn schema(
    schema: &mut Schema,
    base: &url::Url,
    changes: &mut Vec<(String, String)>,
) -> io::Result<()> {
    let mut apply = |a: &mut Vec<Annotation>| annotations(a, base, changes);
    apply(&mut schema.annotations)?;
    for group in &mut schema.external_annotations {
        apply(&mut group.annotations)?;
    }
    for element in &mut schema.elements {
        match element {
            SchemaElement::EntityType(t) => {
                apply(&mut t.annotations)?;
                for p in &mut t.properties {
                    apply(&mut p.annotations)?;
                }
                for n in &mut t.navigation_properties {
                    apply(&mut n.annotations)?;
                    for c in &mut n.referential_constraints {
                        apply(&mut c.annotations)?;
                    }
                }
            }
            SchemaElement::ComplexType(t) => {
                apply(&mut t.annotations)?;
                for p in &mut t.properties {
                    apply(&mut p.annotations)?;
                }
                for n in &mut t.navigation_properties {
                    apply(&mut n.annotations)?;
                    for c in &mut n.referential_constraints {
                        apply(&mut c.annotations)?;
                    }
                }
            }
            SchemaElement::EnumType(t) => {
                apply(&mut t.annotations)?;
                for m in &mut t.members {
                    apply(&mut m.annotations)?;
                }
            }
            SchemaElement::TypeDefinition(t) => apply(&mut t.annotations)?,
            SchemaElement::Term(t) => apply(&mut t.annotations)?,
            SchemaElement::Function(t) => {
                apply(&mut t.annotations)?;
                for p in &mut t.parameters {
                    apply(&mut p.annotations)?;
                }
                if let Some(r) = &mut t.return_type {
                    apply(&mut r.annotations)?;
                }
            }
            SchemaElement::Action(t) => {
                apply(&mut t.annotations)?;
                for p in &mut t.parameters {
                    apply(&mut p.annotations)?;
                }
                if let Some(r) = &mut t.return_type {
                    apply(&mut r.annotations)?;
                }
            }
            SchemaElement::EntityContainer(c) => {
                apply(&mut c.annotations)?;
                for s in &mut c.entity_sets {
                    apply(&mut s.annotations)?;
                }
                for s in &mut c.singletons {
                    apply(&mut s.annotations)?;
                }
                for s in &mut c.function_imports {
                    apply(&mut s.annotations)?;
                }
                for s in &mut c.action_imports {
                    apply(&mut s.annotations)?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn annotations(
    values: &mut [Annotation],
    base: &url::Url,
    changes: &mut Vec<(String, String)>,
) -> io::Result<()> {
    for a in values {
        annotations(&mut a.annotations, base, changes)?;
        if let Some(e) = &mut a.expression {
            expression(e, base, changes)?;
        }
    }
    Ok(())
}

fn expression(
    node: &mut CsdlAnnotationExpression,
    base: &url::Url,
    changes: &mut Vec<(String, String)>,
) -> io::Result<()> {
    use CsdlAnnotationExpression as E;
    match node {
        E::Annotated {
            expression: e,
            annotations: a,
        } => {
            annotations(a, base, changes)?;
            expression(e, base, changes)?;
        }
        E::Record {
            annotations: a,
            properties,
            ..
        } => {
            annotations(a, base, changes)?;
            for p in properties {
                annotations(&mut p.annotations, base, changes)?;
                if let Some(e) = &mut p.value {
                    expression(e, base, changes)?;
                }
            }
        }
        E::Collection(values) | E::Apply { args: values, .. } => {
            for e in values {
                expression(e, base, changes)?;
            }
        }
        E::Faceted { expression: e, .. }
        | E::Not(e)
        | E::Neg(e)
        | E::Cast { expr: e, .. }
        | E::IsOf { expr: e, .. }
        | E::LabeledElement { expr: e, .. } => expression(e, base, changes)?,
        E::BinaryExpression { lhs, rhs, .. } => {
            expression(lhs, base, changes)?;
            expression(rhs, base, changes)?;
        }
        E::If { test, then_, else_ } => {
            expression(test, base, changes)?;
            expression(then_, base, changes)?;
            if let Some(e) = else_ {
                expression(e, base, changes)?;
            }
        }
        E::UrlRef(operand) => {
            if let E::String(uri) = operand.as_mut() {
                let absolute = base.join(uri).map_err(io::Error::other)?.to_string();
                changes.push((uri.clone(), absolute.clone()));
                *uri = absolute;
            } else {
                return Err(io::Error::other(
                    "Cannot preserve a dynamic UrlRef operand when rebasing; supply a literal URL",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}
