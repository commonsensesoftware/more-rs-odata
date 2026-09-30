//! Checks that require the original JSON spelling, before syntax normalization.
use serde_json::Value;
use std::{collections::BTreeMap, io};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub(super) fn validate(root: &Value) -> io::Result<()> {
    let mut aliases = BTreeMap::new();
    let object = root
        .as_object()
        .ok_or_else(|| invalid("Document must be an object"))?;
    for (namespace, schema) in object {
        if namespace.starts_with(['$', '@']) {
            continue;
        }
        if let Some(alias) = schema.get("$Alias").and_then(Value::as_str) {
            aliases.insert(namespace.clone(), alias.to_owned());
        }
    }
    for reference in root
        .get("$Reference")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|m| m.values())
    {
        for include in reference
            .get("$Include")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(namespace), Some(alias)) = (
                include.get("$Namespace").and_then(Value::as_str),
                include.get("$Alias").and_then(Value::as_str),
            ) {
                aliases.insert(namespace.to_owned(), alias.to_owned());
            }
        }
    }
    visit(root, "Edmx", "", None, &aliases)
}

fn qualified(text: &str, aliases: &BTreeMap<String, String>) -> io::Result<()> {
    // These separators delimit qualified names in CSDL targets and signatures.
    // Quoted key literals are data and must never be interpreted as names.
    let mut token = String::new();
    let mut quoted = false;
    let check = |token: &str| {
        if let Some((namespace, _)) = token.rsplit_once('.') {
            if let Some(alias) = aliases.get(namespace) {
                return Err(invalid(format!(
                    "JSON reference {token} must use alias {alias}"
                )));
            }
        }
        Ok(())
    };
    for c in text.chars().chain(std::iter::once('/')) {
        if c == '\'' {
            check(&token)?;
            token.clear();
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        if matches!(c, '/' | '@' | '(' | ')' | ',' | '#' | '=' | ' ') {
            check(&token)?;
            token.clear();
        } else {
            token.push(c);
        }
    }
    Ok(())
}

fn visit(
    value: &Value,
    owner: &str,
    namespace: &str,
    container: Option<&str>,
    aliases: &BTreeMap<String, String>,
) -> io::Result<()> {
    if let Some(values) = value.as_array() {
        for value in values {
            visit(value, owner, namespace, container, aliases)?;
        }
        return Ok(());
    }
    let Some(map) = value.as_object() else {
        return if owner == "Expression" {
            Ok(())
        } else {
            Err(invalid(format!("{owner} must be an object")))
        };
    };
    for (key, value) in map {
        if owner != "Expression" && match key.as_str() {
            "$Name" => owner != "Parameter",
            "$Namespace" => owner != "Include",
            "$Uri" | "$EntityType" | "$Property" | "$ReferencedProperty" | "$Term" | "$Target" | "$Value" => true,
            _ => false,
        } {
            return Err(invalid(format!("{owner} does not allow {key}")));
        }
        if key.starts_with('$')
            && owner != "Expression"
            && !super::validation::attribute_allowed(owner, &key[1..], true)
        {
            return Err(invalid(format!("{owner} does not allow {key}")));
        }
        if key.contains('@') {
            qualified(key, aliases)?;
            visit(value, "Expression", namespace, container, aliases)?;
            continue;
        }
        if matches!(
            key.as_str(),
            "$Type"
                | "$UnderlyingType"
                | "$BaseType"
                | "$BaseTerm"
                | "$Extends"
                | "$Partner"
                | "$Action"
                | "$Function"
                | "$EntitySet"
                | "$EntitySetPath"
                | "$Path"
                | "$PropertyPath"
                | "$NavigationPropertyPath"
                | "$AnnotationPath"
                | "$ModelElementPath"
                | "$LabeledElementReference"
                | "$EnumMember"
        ) {
            if let Some(text) = value.as_str() {
                qualified(text, aliases)?;
            }
        }
        // Container identity is the one deliberate namespace-qualified exception.
        if key == "$EntityContainer" {
            continue;
        }
        if key == "$NavigationPropertyBinding" {
            for (path, target) in value
                .as_object()
                .ok_or_else(|| invalid("Bindings must be an object"))?
            {
                qualified(path, aliases)?;
                let target = target
                    .as_str()
                    .ok_or_else(|| invalid("Binding target must be a string"))?;
                if let Some(container) = container {
                    let local = format!("{namespace}.{container}/");
                    let alias = aliases
                        .get(namespace)
                        .map(|alias| format!("{alias}.{container}/"));
                    if target.starts_with(&local)
                        || alias
                            .as_ref()
                            .is_some_and(|prefix| target.starts_with(prefix))
                    {
                        return Err(invalid(
                            "JSON same-container binding targets must omit the container prefix",
                        ));
                    }
                }
                qualified(target, aliases)?;
            }
            continue;
        }
        if key == "$Annotations" {
            for (target, annotations) in value
                .as_object()
                .ok_or_else(|| invalid("Annotations must be an object"))?
            {
                qualified(target, aliases)?;
                visit(annotations, "Annotations", namespace, container, aliases)?;
            }
            continue;
        }
        if key == "$Reference" {
            for reference in value
                .as_object()
                .ok_or_else(|| invalid("References must be an object"))?
                .values()
            {
                visit(reference, "Reference", namespace, container, aliases)?;
            }
            continue;
        }
        let child_owner = match key.as_str() {
            "$Include" => "Include",
            "$IncludeAnnotations" => "IncludeAnnotations",
            "$Parameter" => "Parameter",
            "$ReturnType" => "ReturnType",
            _ => "",
        };
        if !child_owner.is_empty() {
            if key != "$ReturnType" && !value.is_array() {
                return Err(invalid(format!("{key} must be an array")));
            }
            visit(value, child_owner, namespace, container, aliases)?;
            continue;
        }
        if key == "$ReferentialConstraint" {
            for (property, target) in value
                .as_object()
                .ok_or_else(|| invalid("Constraints must be an object"))?
            {
                if property.contains('@') {
                    visit(target, "Expression", namespace, container, aliases)?;
                } else if !target.is_string() {
                    return Err(invalid("Referenced property must be a string"));
                }
            }
            continue;
        }
        if key == "$OnDelete"
            && !matches!(
                value.as_str(),
                Some("Cascade" | "None" | "SetNull" | "SetDefault")
            )
        {
            return Err(invalid("Invalid $OnDelete action"));
        }
        if key == "$AppliesTo"
            && !value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Err(invalid("$AppliesTo must contain strings"));
        }
        if key == "$Key" {
            let keys = value
                .as_array()
                .ok_or_else(|| invalid("$Key must be an array"))?;
            if keys.is_empty()
                || !keys.iter().all(|key| {
                    key.is_string()
                        || key
                            .as_object()
                            .is_some_and(|m| m.len() == 1 && m.values().all(Value::is_string))
                })
            {
                return Err(invalid(
                    "$Key must contain property names or single-entry alias objects",
                ));
            }
        }
        if key.starts_with('$') {
            if owner == "Expression" {
                visit(value, owner, namespace, container, aliases)?;
            }
            continue;
        }
        let kind = value.get("$Kind").and_then(Value::as_str);
        match owner {
            "Edmx" => visit(value, "Schema", key, None, aliases)?,
            "Schema" if value.is_array() => {
                for overload in value.as_array().unwrap() {
                    visit(
                        overload,
                        overload
                            .get("$Kind")
                            .and_then(Value::as_str)
                            .unwrap_or("Function"),
                        namespace,
                        None,
                        aliases,
                    )?;
                }
            }
            "Schema" => {
                let kind = kind.unwrap_or("");
                visit(
                    value,
                    kind,
                    namespace,
                    (kind == "EntityContainer").then_some(key.as_str()),
                    aliases,
                )?;
            }
            "EntityType" | "ComplexType" => {
                if !matches!(kind, None | Some("Property" | "NavigationProperty")) {
                    return Err(invalid("Structured type members must be properties"));
                }
                visit(
                    value,
                    kind.unwrap_or("Property"),
                    namespace,
                    container,
                    aliases,
                )?;
            }
            "EntityContainer" => {
                let kind = if value.get("$Action").is_some() {
                    "ActionImport"
                } else if value.get("$Function").is_some() {
                    "FunctionImport"
                } else if value.get("$Collection") == Some(&Value::Bool(true)) {
                    "EntitySet"
                } else {
                    "Singleton"
                };
                visit(value, kind, namespace, container, aliases)?;
            }
            "Expression" => visit(value, owner, namespace, container, aliases)?,
            _ => {}
        }
    }
    Ok(())
}
