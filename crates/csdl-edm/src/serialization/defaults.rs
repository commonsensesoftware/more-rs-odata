use super::*;

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
                    t.underlying_type.clone(),
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
    definitions: &BTreeMap<String, String>,
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
                .map(String::as_str)
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
