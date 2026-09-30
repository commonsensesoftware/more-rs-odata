use super::{RawElement, raw_attr};
use crate::conformance::{
    ValidationProfile, is_namespace, is_qualified_name, is_simple_identifier,
};
use quick_xml::{
    NsReader, Writer,
    events::{BytesEnd, BytesStart, Event},
    name::ResolveResult,
};
use std::{collections::HashSet, io};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

const EDM: &[u8] = b"http://docs.oasis-open.org/odata/ns/edm";
const EDMX: &[u8] = b"http://docs.oasis-open.org/odata/ns/edmx";

fn wrapper(name: &str) -> bool {
    matches!(
        name,
        "Edmx" | "DataServices" | "Reference" | "Include" | "IncludeAnnotations"
    )
}

fn declaration(name: &str) -> bool {
    wrapper(name)
        || matches!(
            name,
            "Schema"
                | "EntityType"
                | "ComplexType"
                | "EnumType"
                | "Member"
                | "TypeDefinition"
                | "Term"
                | "Property"
                | "NavigationProperty"
                | "Key"
                | "PropertyRef"
                | "ReferentialConstraint"
                | "OnDelete"
                | "Action"
                | "Function"
                | "Parameter"
                | "ReturnType"
                | "EntityContainer"
                | "EntitySet"
                | "Singleton"
                | "ActionImport"
                | "FunctionImport"
                | "NavigationPropertyBinding"
                | "Annotations"
                | "Annotation"
        )
}

fn expression(name: &str) -> bool {
    matches!(
        name,
        "Binary"
            | "Bool"
            | "Date"
            | "DateTimeOffset"
            | "Decimal"
            | "Duration"
            | "EnumMember"
            | "Float"
            | "Guid"
            | "Int"
            | "String"
            | "TimeOfDay"
            | "Null"
            | "Path"
            | "PropertyPath"
            | "NavigationPropertyPath"
            | "AnnotationPath"
            | "ModelElementPath"
            | "Not"
            | "And"
            | "Or"
            | "Eq"
            | "Ne"
            | "Gt"
            | "Ge"
            | "Lt"
            | "Le"
            | "Has"
            | "In"
            | "Add"
            | "Sub"
            | "Neg"
            | "Mul"
            | "Div"
            | "DivBy"
            | "Mod"
            | "If"
            | "Apply"
            | "Cast"
            | "IsOf"
            | "Record"
            | "PropertyValue"
            | "Collection"
            | "LabeledElement"
            | "LabeledElementReference"
            | "UrlRef"
    )
}

/// Resolve namespaces before the legacy token reader sees local names.
/// Unknown extension subtrees never contribute declarations or expressions.
pub(super) fn xml_source<R: io::BufRead>(
    source: R,
    profile: ValidationProfile,
) -> io::Result<Vec<u8>> {
    let mut reader = NsReader::from_reader(source);
    let mut writer = Writer::new(Vec::new());
    let mut buffer = Vec::new();
    let mut skipped = 0usize;
    let mut depth = 0usize;
    let mut roots = 0usize;
    let mut names = Vec::new();
    loop {
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(io::Error::other)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                if depth == 0 {
                    roots += 1;
                }
                if !empty {
                    depth += 1;
                }
                if skipped > 0 {
                    if !empty {
                        skipped += 1;
                    }
                    continue;
                }
                let (ns, local) = reader.resolver().resolve_element(e.name());
                let name = std::str::from_utf8(local.as_ref()).map_err(io::Error::other)?;
                let expected = if wrapper(name) { EDMX } else { EDM };
                let bound = matches!(&ns, ResolveResult::Bound(n) if n.as_ref() == expected);
                let known = declaration(name) || expression(name);
                if !bound || !known {
                    if profile == ValidationProfile::Strict
                        && (depth <= usize::from(!empty)
                            || (known
                                && matches!(
                                    ns,
                                    ResolveResult::Unbound | ResolveResult::Unknown(_)
                                )))
                    {
                        return Err(invalid(format!("Invalid CSDL namespace or root: {name}")));
                    }
                    if !empty {
                        skipped = 1;
                    }
                    continue;
                }
                let mut start = BytesStart::new(name);
                for attr in e.attributes() {
                    let attr = attr.map_err(io::Error::other)?;
                    let (ns, local) = reader.resolver().resolve_attribute(attr.key);
                    if matches!(ns, ResolveResult::Unbound)
                        && !attr.key.as_ref().starts_with(b"xmlns")
                    {
                        let key = std::str::from_utf8(local.as_ref()).map_err(io::Error::other)?;
                        let value = attr
                            .decoded_and_normalized_value(
                                quick_xml::XmlVersion::Implicit1_0,
                                reader.decoder(),
                            )
                            .map_err(io::Error::other)?;
                        start.push_attribute((key, value.as_ref()));
                    }
                }
                writer
                    .write_event(if empty {
                        Event::Empty(start)
                    } else {
                        Event::Start(start)
                    })
                    .map_err(io::Error::other)?;
                if !empty {
                    names.push(name.to_owned());
                }
            }
            Event::End(e) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("Unexpected closing element"))?;
                if skipped > 0 {
                    skipped -= 1;
                    continue;
                }
                let local = e.local_name();
                let name = std::str::from_utf8(local.as_ref()).map_err(io::Error::other)?;
                names.pop();
                writer
                    .write_event(Event::End(BytesEnd::new(name)))
                    .map_err(io::Error::other)?;
            }
            Event::Eof => break,
            Event::DocType(_) => {
                return Err(invalid("CSDL does not allow document type declarations"));
            }
            Event::Text(ref text) if skipped == 0 && profile == ValidationProfile::Strict => {
                if names.last().is_none_or(|name| declaration(name))
                    && !text.as_ref().iter().all(u8::is_ascii_whitespace)
                {
                    return Err(invalid("Text is not allowed in a CSDL declaration"));
                }
                writer.write_event(event).map_err(io::Error::other)?;
            }
            Event::CData(_)
                if skipped == 0
                    && profile == ValidationProfile::Strict
                    && names.last().is_none_or(|name| declaration(name)) =>
            {
                return Err(invalid(
                    "Character data is not allowed in a CSDL declaration",
                ));
            }
            e if skipped == 0 => {
                writer.write_event(e).map_err(io::Error::other)?;
            }
            _ => {}
        }
    }
    if roots != 1 || depth != 0 {
        return Err(invalid("CSDL requires one complete root element"));
    }
    Ok(writer.into_inner())
}

pub(super) fn document(roots: &[RawElement], json: bool) -> io::Result<()> {
    if roots.len() != 1 || roots[0].name != "Edmx" {
        return Err(invalid("CSDL requires one Edmx document"));
    }
    let root = &roots[0];
    if !matches!(raw_attr(root, "Version"), Some("4.0" | "4.01")) {
        return Err(invalid("Version must be 4.0 or 4.01"));
    }
    walk(root, None, json)?;
    let schemas = root
        .children
        .iter()
        .flat_map(|n| {
            if n.name == "DataServices" {
                n.children.iter().collect::<Vec<_>>()
            } else if n.name == "Schema" {
                vec![n]
            } else {
                vec![]
            }
        })
        .collect::<Vec<_>>();
    let includes = root
        .children
        .iter()
        .filter(|n| n.name == "Reference")
        .flat_map(|r| r.children.iter().filter(|n| n.name == "Include"))
        .collect::<Vec<_>>();
    let mut namespaces = HashSet::new();
    let mut aliases = HashSet::new();
    for schema in schemas.iter().chain(includes.iter()) {
        let ns = raw_attr(schema, "Namespace").unwrap();
        if !namespaces.insert(ns) {
            return Err(invalid(format!("Duplicate namespace: {ns}")));
        }
    }
    for schema in schemas.iter().chain(includes.iter()) {
        if let Some(alias) = raw_attr(schema, "Alias") {
            if matches!(alias, "Edm" | "odata" | "System" | "Transient")
                || namespaces.contains(alias)
                || !aliases.insert(alias)
            {
                return Err(invalid(format!("Reserved or conflicting alias: {alias}")));
            }
        }
    }
    let mut references = HashSet::new();
    for reference in root.children.iter().filter(|n| n.name == "Reference") {
        if !references.insert(raw_attr(reference, "Uri")) {
            return Err(invalid("Duplicate reference URI"));
        }
    }
    Ok(())
}

fn walk(node: &RawElement, parent: Option<&str>, json: bool) -> io::Result<()> {
    let name = node.name.as_str();
    let required: &[&str] = match name {
        "Edmx" => &["Version"],
        "Schema" | "Include" => &["Namespace"],
        "Reference" => &["Uri"],
        "IncludeAnnotations" => &["TermNamespace"],
        "Annotations" => &["Target"],
        "Annotation" => &["Term"],
        "ReferentialConstraint" => &["Property", "ReferencedProperty"],
        "NavigationPropertyBinding" => &["Path", "Target"],
        "OnDelete" => &["Action"],
        "EntitySet" => &["Name", "EntityType"],
        "Singleton" | "NavigationProperty" => &["Name", "Type"],
        "TypeDefinition" => &["Name", "UnderlyingType"],
        "FunctionImport" => &["Name", "Function"],
        "ActionImport" => &["Name", "Action"],
        "Property" | "Parameter" | "Term" if !json => &["Name", "Type"],
        "ReturnType" if !json => &["Type"],
        "EntityType" | "ComplexType" | "EnumType" | "Member" | "EntityContainer" | "Function"
        | "Action" | "PropertyRef" | "Property" | "Parameter" | "Term" => &["Name"],
        _ => &[],
    };
    for key in required {
        if raw_attr(node, key).is_none_or(str::is_empty) {
            return Err(invalid(format!("{name} requires {key}")));
        }
    }
    for (key, value) in &node.attributes {
        if !attribute_allowed(name, key, json) {
            return Err(invalid(format!("{name} does not allow {key}")));
        }
        let valid = match key.as_str() {
            "Name" if name != "PropertyRef" => is_simple_identifier(value),
            "Alias" | "Qualifier" => is_simple_identifier(value),
            "Namespace" | "TermNamespace" | "TargetNamespace" => is_namespace(value),
            "Term" | "BaseTerm" | "BaseType" | "UnderlyingType" | "Extends" => {
                is_qualified_name(value)
            }
            "Function" if name == "FunctionImport" => is_qualified_name(value),
            "Action" if name == "ActionImport" => is_qualified_name(value),
            "Type" | "EntityType" => {
                let ty = value
                    .strip_prefix("Collection(")
                    .and_then(|s| s.strip_suffix(')'))
                    .unwrap_or(value);
                is_qualified_name(ty)
            }
            "Abstract"
            | "OpenType"
            | "HasStream"
            | "Nullable"
            | "Unicode"
            | "IsBound"
            | "IsComposable"
            | "ContainsTarget"
            | "IncludeInServiceDocument"
            | "IsFlags"
            | "Collection" => matches!(value.as_str(), "true" | "false"),
            _ => true,
        };
        if !valid {
            return Err(invalid(format!("Invalid {name}.{key}: {value}")));
        }
    }
    let children: &[&str] = match name {
        "Edmx" if json => &["Reference", "Schema"],
        "Edmx" => &["Reference", "DataServices"],
        "DataServices" => &["Schema"],
        "Reference" => &["Include", "IncludeAnnotations", "Annotation"],
        "Schema" => &[
            "EntityType",
            "ComplexType",
            "EnumType",
            "TypeDefinition",
            "Term",
            "Action",
            "Function",
            "EntityContainer",
            "Annotations",
            "Annotation",
        ],
        "EntityType" => &["Key", "Property", "NavigationProperty", "Annotation"],
        "ComplexType" => &["Property", "NavigationProperty", "Annotation"],
        "EnumType" => &["Member", "Annotation"],
        "Key" => &["PropertyRef"],
        "Action" | "Function" => &["Parameter", "ReturnType", "Annotation"],
        "EntityContainer" => &[
            "EntitySet",
            "Singleton",
            "ActionImport",
            "FunctionImport",
            "Annotation",
        ],
        "EntitySet" | "Singleton" => &["NavigationPropertyBinding", "Annotation"],
        "NavigationProperty" => &["ReferentialConstraint", "OnDelete", "Annotation"],
        "IncludeAnnotations" | "PropertyRef" | "NavigationPropertyBinding" => &[],
        _ => &["Annotation"],
    };
    for child in &node.children {
        if declaration(&child.name) && !children.contains(&child.name.as_str()) {
            return Err(invalid(format!("{} is not allowed in {name}", child.name)));
        }
        walk(child, Some(name), json)?;
    }
    let count = |kind: &str| node.children.iter().filter(|c| c.name == kind).count();
    if (name == "Edmx" && !json && count("DataServices") != 1)
        || (name == "DataServices" && count("Schema") == 0)
        || (name == "Reference" && count("Include") + count("IncludeAnnotations") == 0)
        || (name == "Annotations" && count("Annotation") == 0)
        || (name == "Key" && count("PropertyRef") == 0)
        || (name == "Function" && count("ReturnType") != 1)
        || (matches!(name, "Function" | "Action") && count("ReturnType") > 1)
        || (name == "EntityType" && count("Key") > 1)
        || (name == "NavigationProperty" && count("OnDelete") > 1)
        || (name == "Annotation" && node.annotation_expressions.len() > 1)
    {
        return Err(invalid(format!("Invalid child cardinality in {name}")));
    }
    // Keep operation overloads, but reject declaration collisions before a map can hide them.
    if matches!(
        name,
        "Schema"
            | "EntityType"
            | "ComplexType"
            | "EnumType"
            | "EntityContainer"
            | "Action"
            | "Function"
    ) {
        let mut seen = std::collections::HashMap::new();
        for child in &node.children {
            if let Some(n) = raw_attr(child, "Name") {
                if let Some(previous) = seen.insert(n, child.name.as_str()) {
                    if name != "Schema"
                        || previous != child.name
                        || !matches!(previous, "Action" | "Function")
                    {
                        return Err(invalid(format!("Duplicate {name} child: {n}")));
                    }
                }
            }
        }
    }
    let _ = parent;
    Ok(())
}

fn attribute_allowed(name: &str, key: &str, json: bool) -> bool {
    match key {
        "Name" => matches!(
            name,
            "EntityType"
                | "ComplexType"
                | "EnumType"
                | "Member"
                | "TypeDefinition"
                | "Term"
                | "Property"
                | "NavigationProperty"
                | "PropertyRef"
                | "EntityContainer"
                | "EntitySet"
                | "Singleton"
                | "Action"
                | "Function"
                | "Parameter"
                | "ActionImport"
                | "FunctionImport"
                | "LabeledElement"
        ),
        "Namespace" => matches!(name, "Schema" | "Include"),
        "Alias" => matches!(name, "Schema" | "Include" | "PropertyRef"),
        "Version" => name == "Edmx",
        "Uri" => name == "Reference",
        "Abstract" | "BaseType" | "OpenType" => matches!(name, "EntityType" | "ComplexType"),
        "HasStream" => name == "EntityType",
        "IsFlags" => name == "EnumType",
        "UnderlyingType" => matches!(name, "EnumType" | "TypeDefinition"),
        "Type" => matches!(
            name,
            "Property"
                | "NavigationProperty"
                | "Term"
                | "Parameter"
                | "ReturnType"
                | "Singleton"
                | "Cast"
                | "IsOf"
                | "Record"
        ),
        "EntityType" => name == "EntitySet",
        "Nullable" => matches!(
            name,
            "Property"
                | "NavigationProperty"
                | "Term"
                | "Parameter"
                | "ReturnType"
                | "Singleton"
                | "Cast"
                | "IsOf"
        ),
        "MaxLength" | "Precision" | "Scale" | "SRID" | "Unicode" => matches!(
            name,
            "Property" | "TypeDefinition" | "Term" | "Parameter" | "ReturnType" | "Cast" | "IsOf"
        ),
        "DefaultValue" => matches!(name, "Property" | "Term"),
        "ContainsTarget" | "Partner" => name == "NavigationProperty",
        "Property" | "ReferencedProperty" => name == "ReferentialConstraint",
        "IsBound" | "EntitySetPath" => matches!(name, "Action" | "Function"),
        "IsComposable" => name == "Function",
        "Extends" => name == "EntityContainer",
        "EntitySet" => matches!(name, "ActionImport" | "FunctionImport"),
        "IncludeInServiceDocument" => matches!(name, "EntitySet" | "FunctionImport"),
        "Function" => matches!(name, "FunctionImport" | "Apply"),
        "Action" => matches!(name, "ActionImport" | "OnDelete"),
        "BaseTerm" | "AppliesTo" => name == "Term",
        "Term" => name == "Annotation",
        "Qualifier" => matches!(name, "Annotation" | "Annotations" | "IncludeAnnotations"),
        "Target" => {
            matches!(name, "Annotations" | "NavigationPropertyBinding")
                || (json && name == "Annotation")
        }
        "Path" => matches!(name, "NavigationPropertyBinding" | "Annotation"),
        "TermNamespace" | "TargetNamespace" => name == "IncludeAnnotations",
        "Value" => name == "Member",
        "Collection" => json,
        // Future attributes are ignored; known attributes on the wrong element are errors.
        _ => true,
    }
}

pub(super) fn json_source<R: io::Read>(
    reader: R,
    profile: ValidationProfile,
) -> io::Result<serde_json::Value> {
    let value: serde_json::Value = if profile == ValidationProfile::Strict {
        serde_json::from_reader::<_, UniqueValue>(reader)
            .map_err(io::Error::other)?
            .0
    } else {
        serde_json::from_reader(reader).map_err(io::Error::other)?
    };
    if profile == ValidationProfile::Strict {
        if !matches!(
            value.get("$Version").and_then(|v| v.as_str()),
            Some("4.0" | "4.01")
        ) {
            return Err(invalid("$Version must be a string containing 4.0 or 4.01"));
        }
        json_shapes(&value)?;
        for (ns, schema) in value
            .as_object()
            .ok_or_else(|| invalid("Document must be an object"))?
        {
            if ns.starts_with(['$', '@']) {
                continue;
            }
            for (name, member) in schema
                .as_object()
                .ok_or_else(|| invalid("Schema must be an object"))?
            {
                if name.starts_with('$') || name.contains('@') {
                    continue;
                }
                let declarations = if let Some(overloads) = member.as_array() {
                    if overloads.is_empty() {
                        return Err(invalid("Operation overload array must not be empty"));
                    }
                    overloads.iter().collect::<Vec<_>>()
                } else {
                    vec![member]
                };
                for declaration in declarations {
                    let kind = declaration
                        .get("$Kind")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| invalid(format!("Schema member {name} requires $Kind")))?;
                    if matches!(kind, "Action" | "Function") != member.is_array() {
                        return Err(invalid(
                            "Operations require an array; other declarations require an object",
                        ));
                    }
                }
            }
        }
    }
    Ok(value)
}

/// serde_json::Value normally overwrites duplicate object members. Strict
/// authoring must diagnose them before constructing that lossy representation.
struct UniqueValue(serde_json::Value);

impl<'de> serde::Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON value with unique object members")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(serde_json::Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(serde_json::Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
                    if values.insert(key.clone(), value.0).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "Duplicate JSON member: {key}"
                        )));
                    }
                }
                Ok(UniqueValue(serde_json::Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn json_shapes(value: &serde_json::Value) -> io::Result<()> {
    use serde_json::Value as V;
    match value {
        V::Object(map) => {
            for (key, value) in map {
                // Annotation values follow expression grammar, handled separately.
                if key.contains('@') {
                    continue;
                }
                if let (Some(kind), Some(attribute)) = (map.get("$Kind").and_then(V::as_str), key.strip_prefix('$')) {
                    if !attribute_allowed(kind, attribute, true) {
                        return Err(invalid(format!("{kind} does not allow {key}")));
                    }
                }
                let valid = match key.as_str() {
                    "$Version" | "$EntityContainer" | "$Kind" | "$Name" | "$Namespace"
                    | "$Alias" | "$Type" | "$BaseType" | "$UnderlyingType" | "$BaseTerm"
                    | "$Extends" | "$TermNamespace" | "$TargetNamespace" | "$Qualifier"
                    | "$Partner" | "$Action" | "$Function" | "$EntitySet" | "$EntitySetPath" => {
                        value.is_string()
                    }
                    "$Nullable"
                    | "$Abstract"
                    | "$OpenType"
                    | "$HasStream"
                    | "$IsFlags"
                    | "$IsBound"
                    | "$IsComposable"
                    | "$Collection"
                    | "$Unicode"
                    | "$ContainsTarget"
                    | "$IncludeInServiceDocument" => value.is_boolean(),
                    "$Reference"
                    | "$Annotations"
                    | "$NavigationPropertyBinding"
                    | "$ReferentialConstraint"
                    | "$ReturnType" => value.is_object(),
                    "$Include" | "$IncludeAnnotations" | "$Key" | "$Parameter" | "$AppliesTo" => {
                        value.is_array()
                    }
                    _ => true,
                };
                if !valid {
                    return Err(invalid(format!("Invalid JSON shape for {key}")));
                }
                json_shapes(value)?;
            }
        }
        V::Array(values) => {
            for value in values {
                json_shapes(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
