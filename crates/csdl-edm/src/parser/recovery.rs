use super::ParseDiagnostic;
use serde_json::Value;

fn unavailable(value: &Value) -> Option<&str> {
    match value {
        Value::Array(items) => items.iter().find_map(unavailable),
        Value::Object(map) => map.iter().find_map(|(key, value)| {
            if key.contains('@') {
                return None;
            }
            if let Some(name) = key.strip_prefix('$') {
                if !super::validation::expression(name)
                    && !matches!(
                        name,
                        "Type"
                            | "Function"
                            | "Name"
                            | "Collection"
                            | "Nullable"
                            | "MaxLength"
                            | "Precision"
                            | "Scale"
                            | "SRID"
                            | "Unicode"
                            | "Annotations"
                    )
                {
                    return Some(key.as_str());
                }
            }
            unavailable(value)
        }),
        _ => None,
    }
}

/// Remove the containing annotation, never an operand or collection item.
/// Otherwise recovery could silently change a known expression's meaning.
pub(super) fn json(value: &mut Value, path: &str, diagnostics: &mut Vec<ParseDiagnostic>) {
    if let Some(items) = value.as_array_mut() {
        for (index, item) in items.iter_mut().enumerate() {
            json(item, &format!("{path}/{index}"), diagnostics);
        }
    }
    let Some(map) = value.as_object_mut() else {
        return;
    };
    let mut removed = Vec::<String>::new();
    for (key, control) in map.iter() {
        let Some(target) = key
            .strip_suffix("@type")
            .or_else(|| key.strip_suffix("@odata.type"))
            .filter(|target| target.contains('@'))
        else {
            continue;
        };
        let mut candidate = serde_json::Map::new();
        if let Some(value) = map.get(target) {
            candidate.insert(target.into(), value.clone());
        }
        candidate.insert(key.clone(), control.clone());
        if let Err(problem) =
            crate::csdl_json_reader::normalize_type_controls(&mut Value::Object(candidate))
        {
            diagnostics.push(ParseDiagnostic {
                location: format!("{path}/{}", target.replace('~', "~0").replace('/', "~1")),
                message: format!("Annotation omitted: {problem}"),
            });
            removed.push(target.into());
        }
    }
    for (key, child) in map.iter_mut() {
        if removed.iter().any(|parent| {
            key == parent || key.strip_prefix(parent).is_some_and(|s| s.starts_with('@'))
        }) {
            continue;
        }
        let location = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
        json(child, &location, diagnostics);
        if key.contains('@') && !key.ends_with("@type") && !key.ends_with("@odata.type") {
            let problem = unavailable(child)
                .map(|construct| format!("expression {construct} is unavailable"))
                .or_else(|| {
                    let mut normalized = child.clone();
                    crate::csdl_json_reader::normalize_type_controls(&mut normalized)
                        .and_then(|()| crate::csdl_json_reader::json_to_expr(normalized))
                        .err()
                        .map(|e| e.to_string())
                });
            if let Some(problem) = problem {
                diagnostics.push(ParseDiagnostic {
                    location,
                    message: format!("Annotation omitted: {problem}"),
                });
                removed.push(key.clone());
            }
        }
    }
    // Also remove controls and annotations belonging to a removed annotation.
    map.retain(|key, _| {
        !removed.iter().any(|parent| {
            key == parent
                || key
                    .strip_prefix(parent)
                    .is_some_and(|suffix| suffix.starts_with('@'))
        })
    });
}
