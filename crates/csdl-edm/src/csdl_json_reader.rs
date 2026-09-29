//! JSON CSDL reader. Emits the same [`SyntaxUnit`] stream as [`crate::reader::CsdlReader`]
//! so the shared parser (see `parser::from_units`) can build the model from either format.
//!
//! The CSDL JSON shape (OASIS CSDL JSON 4.01) differs from XML in several
//! structural ways:
//!
//! * Schema members are keyed object entries (`"Product": { ... }`) — the key
//!   carries what XML expresses as the `Name` attribute, and `$Kind` carries
//!   the would-be element name (and is often omitted, inferred from context).
//! * `$Reference`, `$NavigationPropertyBinding`, `$ReferentialConstraint`, `$Key`
//!   are keyed objects / string arrays — XML expresses them as sequences of
//!   child elements.
//! * Annotations are flat `@Term` / `@Term#Qualifier` keys on the parent object,
//!   with the expression as the value — no wrapping `<Annotation>` element.
//! * Annotation expressions use a different vocabulary (`$Path`, `$And: [lhs,rhs]`,
//!   `$Apply` + `$Function`, bare JSON literals for constants, …).
//!
//! All of this normalization is encapsulated here, so the parser stays
//! format-agnostic and the CSDL element-name vocabulary stays the single
//! source of truth for the syntactic shape downstream of the reader.

use serde_json::{Map, Value};

use crate::csdl::Annotation;
use crate::csdl_xml_reader::SyntaxUnit;
use crate::error::{Error, Result};
use crate::expr::{BinaryOperator, CsdlAnnotationExpression, PropertyValue};

pub struct JsonCsdlReader {
    value: Value,
}

impl JsonCsdlReader {
    pub fn new(value: Value) -> Self {
        Self { value }
    }

    /// Walks the document once and produces the full SyntaxUnit stream.
    /// We don't bother with streaming here — the JSON document is already
    /// fully in memory as a `serde_json::Value`.
    pub fn into_units(self) -> Result<Vec<SyntaxUnit>> {
        let mut out = Vec::new();
        let obj = expect_object(self.value, "document root")?;
        emit_edmx(&mut out, obj)?;
        Ok(out)
    }
}

fn emit_edmx(out: &mut Vec<SyntaxUnit>, mut root: Map<String, Value>) -> Result<()> {
    let version = root
        .shift_remove("$Version")
        .and_then(|v| match v {
            Value::String(s) => Some(s),
            _ => None,
        })
        .unwrap_or_else(|| "4.01".to_string());

    // $EntityContainer at the top is just a back-pointer; the container itself
    // lives inside the schema and we'll find it there.
    root.shift_remove("$EntityContainer");

    let references = root.shift_remove("$Reference");

    start(out, "Edmx", vec![("Version".into(), version)]);

    if let Some(refs) = references {
        emit_references(out, refs)?;
    }

    for (key, value) in root {
        if key.starts_with('$') || key.starts_with('@') {
            // Annotations on Edmx — drop silently; the XML model has nowhere
            // to attach them either.
            continue;
        }
        emit_schema(out, &key, expect_object(value, &key)?)?;
    }

    end(out, "Edmx");
    Ok(())
}

fn emit_references(out: &mut Vec<SyntaxUnit>, value: Value) -> Result<()> {
    let refs = expect_object(value, "$Reference")?;
    for (uri, body) in refs {
        let body = expect_object(body, &uri)?;
        start(out, "Reference", vec![("Uri".into(), uri.clone())]);
        for (k, v) in body {
            match k.as_str() {
                "$Include" => {
                    let arr = match v {
                        Value::Array(a) => a,
                        _ => return Err(csdl_err("$Include must be an array")),
                    };
                    for inc in arr {
                        emit_include(out, expect_object(inc, "$Include[i]")?)?;
                    }
                }
                "$IncludeAnnotations" => {
                    let arr = match v {
                        Value::Array(a) => a,
                        _ => return Err(csdl_err("$IncludeAnnotations must be an array")),
                    };
                    for ia in arr {
                        let mut o = expect_object(ia, "$IncludeAnnotations[i]")?;
                        let term_ns = take_string(&mut o, "$TermNamespace").unwrap_or_default();
                        let qualifier = take_string(&mut o, "$Qualifier");
                        let target_ns = take_string(&mut o, "$TargetNamespace");
                        let mut attrs = vec![("TermNamespace".into(), term_ns)];
                        if let Some(q) = qualifier {
                            attrs.push(("Qualifier".into(), q));
                        }
                        if let Some(t) = target_ns {
                            attrs.push(("TargetNamespace".into(), t));
                        }
                        start(out, "IncludeAnnotations", attrs);
                        end(out, "IncludeAnnotations");
                    }
                }
                _ if k.starts_with('@') => {
                    emit_annotation(out, &k, v)?;
                }
                _ => {}
            }
        }
        end(out, "Reference");
    }
    Ok(())
}

fn emit_include(out: &mut Vec<SyntaxUnit>, mut obj: Map<String, Value>) -> Result<()> {
    let namespace = take_string(&mut obj, "$Namespace").unwrap_or_default();
    let alias = take_string(&mut obj, "$Alias");
    let mut attrs = vec![("Namespace".into(), namespace)];
    if let Some(a) = alias {
        attrs.push(("Alias".into(), a));
    }
    start(out, "Include", attrs);
    for (k, v) in obj {
        if let Some(stripped) = k.strip_prefix('@') {
            let _ = stripped;
            emit_annotation(out, &k, v)?;
        }
    }
    end(out, "Include");
    Ok(())
}

fn emit_schema(
    out: &mut Vec<SyntaxUnit>,
    namespace: &str,
    mut obj: Map<String, Value>,
) -> Result<()> {
    let alias = take_string(&mut obj, "$Alias");
    let mut attrs = vec![("Namespace".into(), namespace.into())];
    if let Some(a) = alias.clone() {
        attrs.push(("Alias".into(), a));
    }
    start(out, "Schema", attrs);

    if let Some(groups) = obj.shift_remove("$Annotations") {
        for (target, annotations) in expect_object(groups, "$Annotations")? {
            start(out, "Annotations", vec![("Target".into(), target)]);
            let annotations = expect_object(annotations, "$Annotations target")?;
            for (key, value) in annotations {
                if !key.starts_with('@') {
                    return Err(csdl_err("External annotations require @Term keys"));
                }
                emit_annotation(out, &key, value)?;
            }
            end(out, "Annotations");
        }
    }

    // Two-pass partition: members first (mirrors XML element order), then
    // schema-level annotations — own (`@Term`) and sibling-target
    // (`MemberName@Term`).
    let mut annotations: Vec<(String, Value)> = Vec::new();
    let mut members: Vec<(String, Value)> = Vec::new();
    for (key, value) in obj {
        if key.starts_with('$') {
            continue;
        }
        if key.contains('@') {
            annotations.push((key, value));
            continue;
        }
        members.push((key, value));
    }

    for (key, value) in members {
        match value {
            Value::Array(overloads) => {
                // Function/Action overload group — name is the key, each
                // element carries $Kind.
                for overload in overloads {
                    let inner = expect_object(overload, &key)?;
                    emit_schema_member(out, &key, inner, namespace)?;
                }
            }
            Value::Object(inner) => {
                emit_schema_member(out, &key, inner, namespace)?;
            }
            _ => {}
        }
    }
    for (k, v) in annotations {
        emit_annotation(out, &k, v)?;
    }

    end(out, "Schema");
    Ok(())
}

fn emit_schema_member(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    let kind = take_string(&mut obj, "$Kind");
    match kind.as_deref() {
        Some("EntityType") => emit_entity_type(out, name, obj, namespace),
        Some("ComplexType") => emit_complex_type(out, name, obj, namespace),
        Some("EnumType") => emit_enum_type(out, name, obj),
        Some("TypeDefinition") => emit_type_definition(out, name, obj),
        Some("Term") => emit_term(out, name, obj, namespace),
        Some("EntityContainer") => emit_entity_container(out, name, obj, namespace),
        Some("Function") => emit_callable(out, "Function", name, obj, namespace),
        Some("Action") => emit_callable(out, "Action", name, obj, namespace),
        // Without $Kind, fall back to EntityType (matches CSDL JSON conventions
        // where $Kind is sometimes implicit for the primary type form).
        None => emit_entity_type(out, name, obj, namespace),
        Some(_) => Ok(()),
    }
}

fn emit_entity_type(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    let base_type = take_string(&mut obj, "$BaseType");
    let abstract_ = take_bool(&mut obj, "$Abstract");
    let open_type = take_bool(&mut obj, "$OpenType");
    let has_stream = take_bool(&mut obj, "$HasStream");
    let key = obj.shift_remove("$Key");

    let mut attrs = vec![("Name".into(), name.into())];
    if let Some(bt) = base_type {
        attrs.push(("BaseType".into(), bt));
    }
    push_opt_bool_attr(&mut attrs, "Abstract", abstract_);
    push_opt_bool_attr(&mut attrs, "OpenType", open_type);
    push_opt_bool_attr(&mut attrs, "HasStream", has_stream);
    start(out, "EntityType", attrs);

    if let Some(key) = key {
        let Value::Array(refs) = key else {
            return Err(Error::Csdl("$Key must be an array".into()));
        };
        start(out, "Key", vec![]);
        for r in refs {
            let attrs = match r {
                Value::String(path) => vec![("Name".into(), path)],
                Value::Object(alias) if alias.len() == 1 => {
                    let (alias, path) = alias.into_iter().next().unwrap();
                    let Value::String(path) = path else {
                        return Err(Error::Csdl("Key alias must name a string path".into()));
                    };
                    vec![("Name".into(), path), ("Alias".into(), alias)]
                }
                _ => {
                    return Err(Error::Csdl(
                        "Key member must be a string or one alias/path pair".into(),
                    ));
                }
            };
            start(out, "PropertyRef", attrs);
            end(out, "PropertyRef");
        }
        end(out, "Key");
    }

    emit_type_members(out, obj, namespace)?;
    end(out, "EntityType");
    Ok(())
}

fn emit_complex_type(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    let base_type = take_string(&mut obj, "$BaseType");
    let abstract_ = take_bool(&mut obj, "$Abstract");
    let open_type = take_bool(&mut obj, "$OpenType");

    let mut attrs = vec![("Name".into(), name.into())];
    if let Some(bt) = base_type {
        attrs.push(("BaseType".into(), bt));
    }
    push_opt_bool_attr(&mut attrs, "Abstract", abstract_);
    push_opt_bool_attr(&mut attrs, "OpenType", open_type);
    start(out, "ComplexType", attrs);
    emit_type_members(out, obj, namespace)?;
    end(out, "ComplexType");
    Ok(())
}

fn emit_enum_type(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
) -> Result<()> {
    if obj.get("$IsFlags").is_some_and(|v| !v.is_boolean()) {
        return Err(Error::Csdl("$IsFlags must be boolean".into()));
    }
    if obj.get("$UnderlyingType").is_some_and(|v| !v.is_string()) {
        return Err(Error::Csdl("$UnderlyingType must be a string".into()));
    }
    let underlying = take_string(&mut obj, "$UnderlyingType");
    let is_flags = take_bool(&mut obj, "$IsFlags");

    let mut attrs = vec![("Name".into(), name.into())];
    if let Some(t) = underlying {
        attrs.push(("UnderlyingType".into(), t));
    }
    push_opt_bool_attr(&mut attrs, "IsFlags", is_flags);
    start(out, "EnumType", attrs);

    // Members are direct keys with integer (or numeric-like) values; type-level
    // annotations are `@Term`/`@Term#Qualifier`, and member-level annotations
    // use the sibling-target form `Red@Core.Description` — both buckets go
    // into the type-level annotation list so they're emitted as <Annotation>
    // children of <EnumType>. The sibling-target ones carry Target="Red"
    // so consumers can route them onto the right member.
    let mut type_annotations: Vec<(String, Value)> = Vec::new();
    for (key, value) in obj {
        if key.starts_with('$') {
            continue;
        }
        if key.contains('@') {
            type_annotations.push((key, value));
            continue;
        }
        let val_i64 = match value {
            Value::Number(n) => n.as_i64(),
            // CSDL JSON encodes large enum values as strings to dodge JS
            // precision loss; accept those too.
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        };
        let v = val_i64.ok_or_else(|| {
            Error::Csdl(format!("Enumeration member {key} requires an Int64 value"))
        })?;
        let member_attrs = vec![("Name".into(), key), ("Value".into(), v.to_string())];
        start(out, "Member", member_attrs);
        end(out, "Member");
    }

    for (k, v) in type_annotations {
        emit_annotation(out, &k, v)?;
    }

    end(out, "EnumType");
    Ok(())
}

fn emit_type_members(
    out: &mut Vec<SyntaxUnit>,
    obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    // Partition keys: properties/nav-properties (object value, name doesn't
    // start with $ or @) come first to mirror XML element order; annotations
    // attached to the type itself — own- (`@Term`) or sibling-target
    // (`Member@Term`) — are emitted last so they appear as children of the
    // EntityType/ComplexType in the SyntaxUnit stream.
    let mut annotations: Vec<(String, Value)> = Vec::new();
    let mut members: Vec<(String, Map<String, Value>)> = Vec::new();
    for (k, v) in obj {
        if k.starts_with('$') {
            continue;
        }
        if k.contains('@') {
            annotations.push((k, v));
            continue;
        }
        if let Value::Object(inner) = v {
            members.push((k, inner));
        }
    }

    for (name, inner) in members {
        emit_structural_or_nav(out, &name, inner, namespace)?;
    }
    for (k, v) in annotations {
        emit_annotation(out, &k, v)?;
    }
    Ok(())
}

fn emit_structural_or_nav(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    let kind = take_string(&mut obj, "$Kind");
    if kind.as_deref() == Some("NavigationProperty") {
        emit_navigation_property(out, name, obj, namespace)
    } else {
        emit_property(out, name, obj, namespace)
    }
}

fn emit_property(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    validate_callable_type(&obj)?;
    let is_collection = take_bool(&mut obj, "$Collection").unwrap_or(false);
    let raw_type = take_string(&mut obj, "$Type").or_else(|| Some("Edm.String".into()));
    let nullable = Some(take_bool(&mut obj, "$Nullable").unwrap_or(false));
    let max_length = take_string_or_number(&mut obj, "$MaxLength");
    let precision = take_string_or_number(&mut obj, "$Precision");
    let scale = take_string_or_number(&mut obj, "$Scale");
    let srid = take_string_or_number(&mut obj, "$SRID");
    let unicode = take_bool(&mut obj, "$Unicode");
    let default_value = take_default_value(&mut obj)?;

    // Materialize JSON's default type: the shared model cannot otherwise
    // distinguish an omitted JSON type from a missing required XML Type.
    // JSON nullability defaults to false; XML defaults to true.
    let mut attrs = vec![("Name".into(), name.into())];
    push_type_attrs(&mut attrs, raw_type.as_deref(), is_collection, namespace);
    if let Some(n) = nullable {
        attrs.push(("Nullable".into(), bool_str(n).into()));
    }
    if let Some(ml) = max_length {
        attrs.push(("MaxLength".into(), ml));
    }
    if let Some(p) = precision {
        attrs.push(("Precision".into(), p));
    }
    if let Some(s) = scale {
        attrs.push(("Scale".into(), s));
    }
    if let Some(s) = srid {
        attrs.push(("SRID".into(), s));
    }
    if let Some(u) = unicode {
        attrs.push(("Unicode".into(), bool_str(u).into()));
    }
    if let Some(dv) = default_value {
        attrs.push(("DefaultValue".into(), dv));
    }

    start(out, "Property", attrs);
    emit_inline_annotations(out, obj)?;
    end(out, "Property");
    Ok(())
}

fn emit_navigation_property(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    for key in ["$Type", "$Partner", "$OnDelete"] {
        if obj.get(key).is_some_and(|value| !value.is_string()) {
            return Err(Error::Csdl(format!(
                "NavigationProperty {key} must be a string"
            )));
        }
    }
    for key in ["$Collection", "$Nullable", "$ContainsTarget"] {
        if obj.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(Error::Csdl(format!(
                "NavigationProperty {key} must be a boolean"
            )));
        }
    }
    if obj
        .get("$ReferentialConstraint")
        .is_some_and(|value| !value.is_object())
    {
        return Err(Error::Csdl(
            "$ReferentialConstraint must be an object".into(),
        ));
    }
    let is_collection = take_bool(&mut obj, "$Collection").unwrap_or(false);
    let raw_type = take_string(&mut obj, "$Type");
    let nullable = take_bool(&mut obj, "$Nullable").or_else(|| (!is_collection).then_some(false));
    let partner = take_string(&mut obj, "$Partner");
    let contains_target = take_bool(&mut obj, "$ContainsTarget");
    let on_delete = take_string(&mut obj, "$OnDelete");
    let referential = obj.shift_remove("$ReferentialConstraint");

    // Literal pass-through, see emit_property comment above.
    let mut attrs = vec![("Name".into(), name.into())];
    push_type_attrs(&mut attrs, raw_type.as_deref(), is_collection, namespace);
    if let Some(n) = nullable {
        attrs.push(("Nullable".into(), bool_str(n).into()));
    }
    if let Some(p) = partner {
        attrs.push(("Partner".into(), p));
    }
    push_opt_bool_attr(&mut attrs, "ContainsTarget", contains_target);

    start(out, "NavigationProperty", attrs);

    if let Some(action) = on_delete {
        start(out, "OnDelete", vec![("Action".into(), action)]);
        end(out, "OnDelete");
    }

    // The $ReferentialConstraint object is flat: it mixes plain entries
    // (Property -> ReferencedProperty strings) with sibling-target annotation
    // keys (`Property@Term[#Qualifier]` -> annotation value). We emit the
    // plain entries as <ReferentialConstraint> elements, and the sibling-
    // target annotations as <Annotation Target="Property"> children of the
    // NavigationProperty. The parser preserves the Target on the model
    // Annotation; routing onto the named constraint is a downstream concern.
    let mut sibling_target_anns: Vec<(String, Value)> = Vec::new();
    if let Some(Value::Object(map)) = referential {
        for (prop, ref_prop) in map {
            if prop.contains('@') {
                sibling_target_anns.push((prop, ref_prop));
                continue;
            }
            let rp = match ref_prop {
                Value::String(s) => s,
                _ => return Err(Error::Csdl("ReferencedProperty must be a string".into())),
            };
            start(
                out,
                "ReferentialConstraint",
                vec![("Property".into(), prop), ("ReferencedProperty".into(), rp)],
            );
            end(out, "ReferentialConstraint");
        }
    }

    emit_inline_annotations(out, obj)?;
    for (k, v) in sibling_target_anns {
        emit_annotation(out, &k, v)?;
    }
    end(out, "NavigationProperty");
    Ok(())
}

fn emit_entity_container(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    let extends = take_string(&mut obj, "$Extends");
    let mut attrs = vec![("Name".into(), name.into())];
    if let Some(e) = extends {
        attrs.push(("Extends".into(), e));
    }
    start(out, "EntityContainer", attrs);

    let mut annotations: Vec<(String, Value)> = Vec::new();
    for (k, v) in obj {
        if k.starts_with('$') {
            continue;
        }
        if k.contains('@') {
            // Own- or sibling-target annotation on the container itself or on
            // one of its children.
            annotations.push((k, v));
            continue;
        }
        let inner = match v {
            Value::Object(m) => m,
            _ => continue,
        };
        emit_container_child(out, &k, inner, namespace)?;
    }

    for (k, v) in annotations {
        emit_annotation(out, &k, v)?;
    }
    end(out, "EntityContainer");
    Ok(())
}

fn emit_container_child(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    _namespace: &str,
) -> Result<()> {
    for key in ["$Function", "$Action", "$Type", "$EntitySet"] {
        if obj.get(key).is_some_and(|value| !value.is_string()) {
            return Err(Error::Csdl(format!(
                "Container child {key} must be a string"
            )));
        }
    }
    for key in ["$Collection", "$Nullable", "$IncludeInServiceDocument"] {
        if obj.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(Error::Csdl(format!(
                "Container child {key} must be boolean"
            )));
        }
    }
    if obj.contains_key("$Function") && obj.contains_key("$Action") {
        return Err(Error::Csdl(
            "Container import cannot specify both $Function and $Action".into(),
        ));
    }
    if obj
        .get("$NavigationPropertyBinding")
        .is_some_and(|value| !value.is_object())
    {
        return Err(Error::Csdl(
            "$NavigationPropertyBinding must be an object".into(),
        ));
    }
    let function_ref = take_string(&mut obj, "$Function");
    let action_ref = take_string(&mut obj, "$Action");
    let is_collection = take_bool(&mut obj, "$Collection").unwrap_or(false);
    let raw_type = take_string(&mut obj, "$Type");
    let entity_set = take_string(&mut obj, "$EntitySet");
    let include_in_service_doc = take_bool(&mut obj, "$IncludeInServiceDocument");
    let bindings = obj.shift_remove("$NavigationPropertyBinding");
    let nullable = take_bool(&mut obj, "$Nullable");

    if let Some(func) = function_ref {
        let mut attrs = vec![("Name".into(), name.into())];
        attrs.push(("Function".into(), func));
        if let Some(es) = entity_set {
            attrs.push(("EntitySet".into(), es));
        }
        if let Some(b) = include_in_service_doc {
            attrs.push(("IncludeInServiceDocument".into(), bool_str(b).into()));
        }
        start(out, "FunctionImport", attrs);
        emit_inline_annotations(out, obj)?;
        end(out, "FunctionImport");
        return Ok(());
    }

    if let Some(action) = action_ref {
        let mut attrs = vec![("Name".into(), name.into())];
        attrs.push(("Action".into(), action));
        if let Some(es) = entity_set {
            attrs.push(("EntitySet".into(), es));
        }
        if let Some(b) = include_in_service_doc {
            attrs.push(("IncludeInServiceDocument".into(), bool_str(b).into()));
        }
        start(out, "ActionImport", attrs);
        emit_inline_annotations(out, obj)?;
        end(out, "ActionImport");
        return Ok(());
    }

    let kind = if is_collection {
        "EntitySet"
    } else {
        "Singleton"
    };
    let mut attrs = vec![("Name".into(), name.into())];
    if kind == "Singleton" {
        push_opt_bool_attr(&mut attrs, "Nullable", nullable);
    }
    if let Some(t) = raw_type {
        let key = if kind == "EntitySet" {
            "EntityType"
        } else {
            "Type"
        };
        attrs.push((key.into(), t));
    }
    if let Some(b) = include_in_service_doc {
        // CSDL 13.4 / 13.5: the attribute applies to both EntitySet and Singleton.
        attrs.push(("IncludeInServiceDocument".into(), bool_str(b).into()));
    }
    start(out, kind, attrs);

    if let Some(Value::Object(map)) = bindings {
        for (path, target) in map {
            let target = match target {
                Value::String(s) => s,
                _ => {
                    return Err(Error::Csdl(
                        "NavigationPropertyBinding target must be a string".into(),
                    ));
                }
            };
            start(
                out,
                "NavigationPropertyBinding",
                vec![("Path".into(), path), ("Target".into(), target)],
            );
            end(out, "NavigationPropertyBinding");
        }
    }

    emit_inline_annotations(out, obj)?;
    end(out, kind);
    Ok(())
}

/// Function and Action share the JSON shape exactly; the only difference is
/// the element name we emit (and that Action has no `$IsComposable`).
fn emit_callable(
    out: &mut Vec<SyntaxUnit>,
    element: &str,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    for key in ["$IsBound", "$IsComposable"] {
        if obj.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(Error::Csdl(format!("Operation {key} must be boolean")));
        }
    }
    for (key, valid) in [
        (
            "$Parameter",
            obj.get("$Parameter").is_none_or(Value::is_array),
        ),
        (
            "$ReturnType",
            obj.get("$ReturnType").is_none_or(Value::is_object),
        ),
        (
            "$EntitySetPath",
            obj.get("$EntitySetPath").is_none_or(Value::is_string),
        ),
    ] {
        if !valid {
            return Err(Error::Csdl(format!("Invalid operation {key} value")));
        }
    }
    let is_bound = take_bool(&mut obj, "$IsBound");
    let is_composable = take_bool(&mut obj, "$IsComposable");
    let entity_set_path = take_string(&mut obj, "$EntitySetPath");
    let parameters = obj.shift_remove("$Parameter");
    let return_type = obj.shift_remove("$ReturnType");

    let mut attrs = vec![("Name".into(), name.into())];
    push_opt_bool_attr(&mut attrs, "IsBound", is_bound);
    if let Some(p) = entity_set_path {
        attrs.push(("EntitySetPath".into(), p));
    }
    if element == "Function" {
        push_opt_bool_attr(&mut attrs, "IsComposable", is_composable);
    }
    start(out, element, attrs);

    if let Some(Value::Array(arr)) = parameters {
        for p in arr {
            let mut p = expect_object(p, "$Parameter[i]")?;
            validate_callable_type(&p)?;
            if p.get("$Name").is_none_or(|value| !value.is_string()) {
                return Err(Error::Csdl("Parameter requires a string $Name".into()));
            }
            let pname = take_string(&mut p, "$Name").unwrap_or_default();
            let is_collection = take_bool(&mut p, "$Collection").unwrap_or(false);
            let raw_type = take_string(&mut p, "$Type").or_else(|| Some("Edm.String".into()));
            let nullable =
                take_bool(&mut p, "$Nullable").or_else(|| (!is_collection).then_some(false));
            let max_length = take_string_or_number(&mut p, "$MaxLength");
            let precision = take_string_or_number(&mut p, "$Precision");
            let scale = take_string_or_number(&mut p, "$Scale");
            let srid = take_string_or_number(&mut p, "$SRID");
            let unicode = take_bool(&mut p, "$Unicode");

            let default_value = take_default_value(&mut p)?;

            let mut attrs = vec![("Name".into(), pname)];
            push_type_attrs(&mut attrs, raw_type.as_deref(), is_collection, namespace);
            if let Some(n) = nullable {
                attrs.push(("Nullable".into(), bool_str(n).into()));
            }
            if let Some(v) = max_length {
                attrs.push(("MaxLength".into(), v));
            }
            if let Some(v) = precision {
                attrs.push(("Precision".into(), v));
            }
            if let Some(v) = scale {
                attrs.push(("Scale".into(), v));
            }
            if let Some(v) = srid {
                attrs.push(("SRID".into(), v));
            }
            if let Some(u) = unicode {
                attrs.push(("Unicode".into(), bool_str(u).into()));
            }
            if let Some(dv) = default_value {
                attrs.push(("DefaultValue".into(), dv));
            }
            start(out, "Parameter", attrs);
            // Parameter-level @annotations
            emit_inline_annotations(out, p)?;
            end(out, "Parameter");
        }
    }

    if let Some(Value::Object(mut rt)) = return_type {
        validate_callable_type(&rt)?;
        let is_collection = take_bool(&mut rt, "$Collection").unwrap_or(false);
        let raw_type = take_string(&mut rt, "$Type").or_else(|| Some("Edm.String".into()));
        let nullable =
            take_bool(&mut rt, "$Nullable").or_else(|| (!is_collection).then_some(false));
        let max_length = take_string_or_number(&mut rt, "$MaxLength");
        let precision = take_string_or_number(&mut rt, "$Precision");
        let scale = take_string_or_number(&mut rt, "$Scale");
        let srid = take_string_or_number(&mut rt, "$SRID");
        let unicode = take_bool(&mut rt, "$Unicode");

        let mut attrs = Vec::new();
        push_type_attrs(&mut attrs, raw_type.as_deref(), is_collection, namespace);
        if let Some(n) = nullable {
            attrs.push(("Nullable".into(), bool_str(n).into()));
        }
        if let Some(v) = max_length {
            attrs.push(("MaxLength".into(), v));
        }
        if let Some(v) = precision {
            attrs.push(("Precision".into(), v));
        }
        if let Some(v) = scale {
            attrs.push(("Scale".into(), v));
        }
        if let Some(v) = srid {
            attrs.push(("SRID".into(), v));
        }
        if let Some(b) = unicode {
            attrs.push(("Unicode".into(), bool_str(b).into()));
        }
        start(out, "ReturnType", attrs);
        emit_inline_annotations(out, rt)?;
        end(out, "ReturnType");
    }

    end(out, element);
    Ok(())
}

fn emit_type_definition(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
) -> Result<()> {
    validate_callable_type(&obj)?;
    let underlying = take_string(&mut obj, "$UnderlyingType").unwrap_or_default();
    let max_length = take_string_or_number(&mut obj, "$MaxLength");
    let precision = take_string_or_number(&mut obj, "$Precision");
    let scale = take_string_or_number(&mut obj, "$Scale");
    let srid = take_string_or_number(&mut obj, "$SRID");
    let unicode = take_bool(&mut obj, "$Unicode");

    let mut attrs = vec![
        ("Name".into(), name.into()),
        ("UnderlyingType".into(), underlying),
    ];
    if let Some(v) = max_length {
        attrs.push(("MaxLength".into(), v));
    }
    if let Some(v) = precision {
        attrs.push(("Precision".into(), v));
    }
    if let Some(v) = scale {
        attrs.push(("Scale".into(), v));
    }
    if let Some(v) = srid {
        attrs.push(("SRID".into(), v));
    }
    if let Some(b) = unicode {
        attrs.push(("Unicode".into(), bool_str(b).into()));
    }
    start(out, "TypeDefinition", attrs);
    emit_inline_annotations(out, obj)?;
    end(out, "TypeDefinition");
    Ok(())
}

fn validate_callable_type(value: &Map<String, Value>) -> Result<()> {
    validate_facets(value)?;
    if value.get("$Type").is_some_and(|value| !value.is_string()) {
        return Err(Error::Csdl("Operation type must be a string".into()));
    }
    for key in ["$Nullable", "$Collection"] {
        if value.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(Error::Csdl(format!("Operation {key} must be boolean")));
        }
    }
    Ok(())
}

fn validate_facets(value: &Map<String, Value>) -> Result<()> {
    for key in ["$MaxLength", "$Precision", "$Scale", "$SRID"] {
        if value
            .get(key)
            .is_some_and(|v| !v.is_string() && !v.is_u64())
        {
            return Err(Error::Csdl(format!(
                "{key} requires a non-negative integer or supported symbolic value"
            )));
        }
    }
    for key in ["$Unicode", "$Nullable", "$Collection"] {
        if value.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(Error::Csdl(format!("{key} must be boolean")));
        }
    }
    Ok(())
}

fn take_default_value(obj: &mut Map<String, Value>) -> Result<Option<String>> {
    match obj.shift_remove("$DefaultValue") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(Value::Bool(value)) => Ok(Some(value.to_string())),
        Some(Value::Number(value)) => Ok(Some(value.to_string())),
        Some(value @ Value::Object(_))
            if crate::spatial::SpatialValue::from_json(value.clone(), 0).is_some() =>
        {
            Ok(Some(value.to_string()))
        }
        Some(_) => Err(csdl_err(
            "$DefaultValue must be a primitive or enumeration value",
        )),
    }
}

fn emit_term(
    out: &mut Vec<SyntaxUnit>,
    name: &str,
    mut obj: Map<String, Value>,
    namespace: &str,
) -> Result<()> {
    validate_callable_type(&obj)?;
    let is_collection = take_bool(&mut obj, "$Collection").unwrap_or(false);
    let raw_type = take_string(&mut obj, "$Type");
    let base_term = take_string(&mut obj, "$BaseTerm");
    let default_value = take_default_value(&mut obj)?;
    let nullable = take_bool(&mut obj, "$Nullable");
    let max_length = take_string_or_number(&mut obj, "$MaxLength");
    let precision = take_string_or_number(&mut obj, "$Precision");
    let scale = take_string_or_number(&mut obj, "$Scale");
    let srid = take_string_or_number(&mut obj, "$SRID");
    let unicode = take_bool(&mut obj, "$Unicode");
    let applies_to = obj.shift_remove("$AppliesTo");

    let mut attrs = vec![("Name".into(), name.into())];
    // Literal pass-through: only emit Type when the source carried it. The
    // CSDL Edm.String default lives in the semantic layer, not the model.
    push_type_attrs(&mut attrs, raw_type.as_deref(), is_collection, namespace);
    if let Some(bt) = base_term {
        attrs.push(("BaseTerm".into(), bt));
    }
    if let Some(d) = default_value {
        attrs.push(("DefaultValue".into(), d));
    }
    if let Some(n) = nullable {
        attrs.push(("Nullable".into(), bool_str(n).into()));
    }
    if let Some(v) = max_length {
        attrs.push(("MaxLength".into(), v));
    }
    if let Some(v) = precision {
        attrs.push(("Precision".into(), v));
    }
    if let Some(v) = scale {
        attrs.push(("Scale".into(), v));
    }
    if let Some(v) = srid {
        attrs.push(("SRID".into(), v));
    }
    if let Some(b) = unicode {
        attrs.push(("Unicode".into(), bool_str(b).into()));
    }
    if let Some(Value::Array(arr)) = applies_to {
        let joined = arr
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        if !joined.is_empty() {
            attrs.push(("AppliesTo".into(), joined));
        }
    }
    start(out, "Term", attrs);
    emit_inline_annotations(out, obj)?;
    end(out, "Term");
    Ok(())
}

fn emit_inline_annotations(out: &mut Vec<SyntaxUnit>, obj: Map<String, Value>) -> Result<()> {
    for (k, v) in obj {
        // Three accepted forms: "@Term", "@Term#Q", "Target@Term[#Q]".
        // Anything else (no `@` at all, or `$`-prefixed housekeeping) is
        // intentionally ignored here — those are dispatched by the caller.
        if k.contains('@') && !k.starts_with('$') {
            emit_annotation(out, &k, v)?;
        }
    }
    Ok(())
}

fn emit_annotation(out: &mut Vec<SyntaxUnit>, key: &str, value: Value) -> Result<()> {
    // Accept three forms:
    //   "@Term"
    //   "@Term#Qualifier"
    //   "Target@Term[#Qualifier]"  (sibling-target on a named sibling)
    let Some((target, term, qualifier)) = parse_annotation_key(key) else {
        return Ok(());
    };
    let mut attrs = vec![("Term".into(), term)];
    if let Some(q) = qualifier {
        attrs.push(("Qualifier".into(), q));
    }
    if let Some(t) = target {
        attrs.push(("Target".into(), t));
    }
    start(out, "Annotation", attrs);
    let expr = json_to_expr(value)?;
    out.push(SyntaxUnit::AnnotationExpression(expr));
    end(out, "Annotation");
    Ok(())
}

/// Decode a JSON CSDL annotation key into `(target?, term, qualifier?)`.
/// Splits at the first `@`: anything before is the sibling target (empty →
/// own-annotation), anything after is `Term[#Qualifier]`. Returns `None` for
/// keys that don't contain `@` at all.
fn parse_annotation_key(key: &str) -> Option<(Option<String>, String, Option<String>)> {
    let at = key.find('@')?;
    let target = if at == 0 {
        None
    } else {
        Some(key[..at].to_string())
    };
    let rest = &key[at + 1..];
    let (term, qualifier) = match rest.split_once('#') {
        Some((t, q)) => (t.to_string(), Some(q.to_string())),
        None => (rest.to_string(), None),
    };
    Some((target, term, qualifier))
}

fn json_to_expr(value: Value) -> Result<CsdlAnnotationExpression> {
    Ok(match value {
        Value::Null => CsdlAnnotationExpression::Null,
        Value::Bool(b) => CsdlAnnotationExpression::Bool(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                CsdlAnnotationExpression::Int(i)
            } else if let Some(f) = n.as_f64() {
                CsdlAnnotationExpression::Float(f)
            } else {
                CsdlAnnotationExpression::String(n.to_string())
            }
        }
        Value::String(s) => CsdlAnnotationExpression::String(s),
        Value::Array(items) => CsdlAnnotationExpression::Collection(
            items
                .into_iter()
                .map(json_to_expr)
                .collect::<Result<Vec<_>>>()?,
        ),
        Value::Object(map) => object_to_expr(map)?,
    })
}

fn object_to_expr(mut map: Map<String, Value>) -> Result<CsdlAnnotationExpression> {
    // Look for the single-`$Foo` dynamic-expression discriminator first.
    let dynamic_key = map
        .keys()
        .find(|k| {
            k.starts_with('$')
                && !matches!(
                    k.as_str(),
                    "$Type" | "$Function" | "$Name" | "$Null" | "$Annotations"
                )
        })
        .cloned();

    if let Some(key) = dynamic_key.as_deref() {
        let v = map.shift_remove(key).unwrap();
        return match key {
            "$Path" => Ok(CsdlAnnotationExpression::Path(string_or_default(v))),
            "$PropertyPath" => Ok(CsdlAnnotationExpression::PropertyPath(string_or_default(v))),
            "$NavigationPropertyPath" => Ok(CsdlAnnotationExpression::NavigationPropertyPath(
                string_or_default(v),
            )),
            "$AnnotationPath" => Ok(CsdlAnnotationExpression::AnnotationPath(string_or_default(
                v,
            ))),
            "$LabeledElementReference" => Ok(CsdlAnnotationExpression::LabeledElementReference(
                string_or_default(v),
            )),
            "$And" | "$Or" | "$Eq" | "$Ne" | "$Gt" | "$Ge" | "$Lt" | "$Le" => {
                let pair = match v {
                    Value::Array(a) if a.len() == 2 => a,
                    _ => return Err(csdl_err(format!("{key} expects [lhs, rhs]"))),
                };
                let mut it = pair.into_iter();
                let lhs = json_to_expr(it.next().unwrap())?;
                let rhs = json_to_expr(it.next().unwrap())?;
                let op = match key {
                    "$And" => BinaryOperator::And,
                    "$Or" => BinaryOperator::Or,
                    "$Eq" => BinaryOperator::Eq,
                    "$Ne" => BinaryOperator::Ne,
                    "$Gt" => BinaryOperator::Gt,
                    "$Ge" => BinaryOperator::Ge,
                    "$Lt" => BinaryOperator::Lt,
                    "$Le" => BinaryOperator::Le,
                    _ => unreachable!(),
                };
                Ok(CsdlAnnotationExpression::BinaryExpression {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                })
            }
            "$Not" => Ok(CsdlAnnotationExpression::Not(Box::new(json_to_expr(v)?))),
            "$If" => {
                let arr = match v {
                    Value::Array(a) if a.len() == 2 || a.len() == 3 => a,
                    _ => return Err(csdl_err("$If expects [test, then] or [test, then, else]")),
                };
                let mut it = arr.into_iter();
                let test = Box::new(json_to_expr(it.next().unwrap())?);
                let then_ = Box::new(json_to_expr(it.next().unwrap())?);
                let else_ = match it.next() {
                    Some(e) => Some(Box::new(json_to_expr(e)?)),
                    None => None,
                };
                Ok(CsdlAnnotationExpression::If { test, then_, else_ })
            }
            "$Apply" => {
                let function = map
                    .shift_remove("$Function")
                    .and_then(|x| match x {
                        Value::String(s) => Some(s),
                        _ => None,
                    })
                    .unwrap_or_default();
                let args = match v {
                    Value::Array(a) => a
                        .into_iter()
                        .map(json_to_expr)
                        .collect::<Result<Vec<_>>>()?,
                    _ => return Err(csdl_err("$Apply expects an array of arguments")),
                };
                Ok(CsdlAnnotationExpression::Apply { function, args })
            }
            "$Cast" => {
                let type_ = take_string(&mut map, "$Type").unwrap_or_default();
                Ok(CsdlAnnotationExpression::Cast {
                    type_,
                    expr: Box::new(json_to_expr(v)?),
                })
            }
            "$IsOf" => {
                let type_ = take_string(&mut map, "$Type").unwrap_or_default();
                Ok(CsdlAnnotationExpression::IsOf {
                    type_,
                    expr: Box::new(json_to_expr(v)?),
                })
            }
            "$LabeledElement" => {
                let label = take_string(&mut map, "$Name").unwrap_or_default();
                Ok(CsdlAnnotationExpression::LabeledElement {
                    name: label,
                    expr: Box::new(json_to_expr(v)?),
                })
            }
            "$UrlRef" => Ok(CsdlAnnotationExpression::UrlRef(Box::new(json_to_expr(v)?))),
            "$Binary" => Ok(CsdlAnnotationExpression::Binary(
                string_or_default(v).into_bytes(),
            )),
            "$Date" => Ok(CsdlAnnotationExpression::Date(string_or_default(v))),
            "$DateTimeOffset" => Ok(CsdlAnnotationExpression::DateTimeOffset(string_or_default(
                v,
            ))),
            "$Decimal" => Ok(CsdlAnnotationExpression::Decimal(string_or_default(v))),
            "$Duration" => Ok(CsdlAnnotationExpression::Duration(string_or_default(v))),
            "$EnumMember" => Ok(CsdlAnnotationExpression::EnumMember(string_or_default(v))),
            "$Guid" => Ok(CsdlAnnotationExpression::Guid(string_or_default(v))),
            "$TimeOfDay" => Ok(CsdlAnnotationExpression::TimeOfDay(string_or_default(v))),
            other => Err(csdl_err(format!(
                "unknown JSON annotation-expression discriminator {other}"
            ))),
        };
    }

    // No dynamic-expression discriminator → it's a Record.
    //
    // Three kinds of keys:
    //   "$Type"             - the optional record type (already taken below)
    //   "@Term[#Q]"         - record-level own-annotation
    //   "Prop@Term[#Q]"     - sibling-target annotation on PropertyValue Prop
    //   <anything else>     - PropertyValue named by the key, value = json_to_expr
    //
    // We do this in two passes: build the PropertyValue list from plain keys,
    // then route annotations. Sibling-target ones attach to the matching
    // PropertyValue's own `annotations` (target=None at that point — the
    // string before `@` is the PropertyValue name, not a sibling target
    // further down the tree).
    let type_ = take_string(&mut map, "$Type");
    let mut properties: Vec<PropertyValue> = Vec::new();
    let mut record_annotations: Vec<Annotation> = Vec::new();
    let mut pv_annotations: Vec<(String, Annotation)> = Vec::new();
    for (k, v) in map {
        if k.starts_with('$') {
            continue;
        }
        if let Some((maybe_pv, term, qualifier)) = parse_annotation_key(&k) {
            let ann = Annotation {
                term,
                qualifier,
                target: None,
                expression: Some(json_to_expr(v)?),
            };
            match maybe_pv {
                Some(pv_name) => pv_annotations.push((pv_name, ann)),
                None => record_annotations.push(ann),
            }
            continue;
        }
        properties.push(PropertyValue {
            property: k,
            value: Some(json_to_expr(v)?),
            annotations: Vec::new(),
        });
    }
    for (pv_name, ann) in pv_annotations {
        if let Some(pv) = properties.iter_mut().find(|p| p.property == pv_name) {
            pv.annotations.push(ann);
        } else {
            // No matching PropertyValue — keep as record-level annotation
            // with target set so the source intent is preserved.
            record_annotations.push(Annotation {
                target: Some(pv_name),
                ..ann
            });
        }
    }
    Ok(CsdlAnnotationExpression::Record {
        type_,
        properties,
        annotations: record_annotations,
    })
}

/// Push `Type` and `Collection` attributes into the SyntaxUnit attribute list
/// based on the JSON-side pieces (`$Type` + `$Collection`). The two stay
/// separate in the stream — the XML reader's combined `Collection(X)`
/// wrapping is handled symmetrically by the parser, which splits either form.
fn push_type_attrs(
    attrs: &mut Vec<(String, String)>,
    raw_type: Option<&str>,
    is_collection: bool,
    namespace: &str,
) {
    let _ = namespace;
    if let Some(t) = raw_type {
        attrs.push(("Type".into(), t.to_string()));
    }
    if is_collection {
        attrs.push(("Collection".into(), "true".into()));
    }
}

/// Append `(key, "true" | "false")` to the SyntaxUnit attribute list only
/// when the source carried the bool. `None` leaves the attribute absent so
/// the downstream parser stores `None` (the model's "use spec default"
/// signal).
fn push_opt_bool_attr(attrs: &mut Vec<(String, String)>, key: &str, v: Option<bool>) {
    if let Some(b) = v {
        attrs.push((key.into(), bool_str(b).into()));
    }
}

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn take_string(map: &mut Map<String, Value>, key: &str) -> Option<String> {
    match map.shift_remove(key)? {
        Value::String(s) => Some(s),
        other => {
            map.insert(key.to_string(), other);
            None
        }
    }
}

fn take_string_or_number(map: &mut Map<String, Value>, key: &str) -> Option<String> {
    match map.shift_remove(key)? {
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
        other => {
            map.insert(key.to_string(), other);
            None
        }
    }
}

fn take_bool(map: &mut Map<String, Value>, key: &str) -> Option<bool> {
    match map.shift_remove(key)? {
        Value::Bool(b) => Some(b),
        other => {
            map.insert(key.to_string(), other);
            None
        }
    }
}

fn string_or_default(v: Value) -> String {
    match v {
        Value::String(s) => s,
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn expect_object(value: Value, ctx: &str) -> Result<Map<String, Value>> {
    match value {
        Value::Object(m) => Ok(m),
        _ => Err(csdl_err(format!("expected object at {ctx}"))),
    }
}

fn start(out: &mut Vec<SyntaxUnit>, name: &str, attributes: Vec<(String, String)>) {
    out.push(SyntaxUnit::StartElement {
        name: name.into(),
        attributes,
    });
}

fn end(out: &mut Vec<SyntaxUnit>, name: &str) {
    out.push(SyntaxUnit::EndElement { name: name.into() });
}

fn csdl_err(msg: impl Into<String>) -> Error {
    Error::Csdl(msg.into())
}
