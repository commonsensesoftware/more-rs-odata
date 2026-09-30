use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::Path;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use serde_json::{Map, Number, Value};

use crate::csdl::*;
use crate::expr::{BinaryOperator, CsdlAnnotationExpression, PropertyValue};
mod defaults;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsdlFormat {
    Json,
    Xml,
}

impl CsdlDocument {
    pub fn to_json_value(&self) -> Value {
        match &self.edmx {
            Some(edmx) => {
                let mut edmx = edmx.clone();
                defaults::materialize_annotations(&mut edmx);
                edmx_to_json(&edmx)
            }
            None => {
                let mut out = Map::new();
                out.insert("$Version".to_string(), Value::String("4.01".to_string()));
                Value::Object(out)
            }
        }
    }

    pub fn write_to<P: AsRef<Path>>(&self, path: P, format: CsdlFormat) -> io::Result<()> {
        let file = File::create(path)?;
        self.write_as(file, format)
    }

    pub fn write_as<W: io::Write>(&self, mut writer: W, format: CsdlFormat) -> io::Result<()> {
        match format {
            CsdlFormat::Json => {
                serde_json::to_writer_pretty(&mut writer, &self.to_json_value())
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
                writer.write_all(b"\n")
            }
            CsdlFormat::Xml => {
                let mut document = self.clone();
                if let Some(edmx) = &mut document.edmx {
                    defaults::normalize_xml_spatial(edmx);
                }
                write_xml_document(&mut writer, &document)
            }
        }
    }
}

fn write_xml_document<W: io::Write>(writer: &mut W, doc: &CsdlDocument) -> io::Result<()> {
    const EDMX_NS: &str = "http://docs.oasis-open.org/odata/ns/edmx";
    const EDM_NS: &str = "http://docs.oasis-open.org/odata/ns/edm";

    let mut xml = quick_xml::Writer::new_with_indent(writer, b' ', 4);
    xml.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    match &doc.edmx {
        Some(edmx) => {
            let mut edmx_attrs = vec![
                ("xmlns:edmx".to_string(), EDMX_NS.to_string()),
                ("xmlns".to_string(), EDM_NS.to_string()),
            ];
            if let Some(v) = &edmx.version {
                edmx_attrs.push(("Version".to_string(), v.clone()));
            }
            write_start(&mut xml, "edmx:Edmx", &edmx_attrs)?;

            for reference in &edmx.references {
                write_reference(&mut xml, reference)?;
            }

            write_start(&mut xml, "edmx:DataServices", &[])?;
            for schema in &edmx.schemas {
                write_schema(&mut xml, schema)?;
            }
            write_end(&mut xml, "edmx:DataServices")?;

            write_end(&mut xml, "edmx:Edmx")?;
        }
        None => {
            write_start(
                &mut xml,
                "edmx:Edmx",
                &[
                    ("xmlns:edmx".to_string(), EDMX_NS.to_string()),
                    ("xmlns".to_string(), EDM_NS.to_string()),
                    ("Version".to_string(), "4.01".to_string()),
                ],
            )?;
            write_empty(&mut xml, "edmx:DataServices", &[])?;
            write_end(&mut xml, "edmx:Edmx")?;
        }
    }

    Ok(())
}

fn write_reference<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    reference: &Reference,
) -> io::Result<()> {
    let mut attrs = Vec::new();
    if !reference.uri.is_empty() {
        attrs.push(("Uri".to_string(), reference.uri.clone()));
    }

    if reference.includes.is_empty() && reference.include_annotations.is_empty() {
        write_empty(xml, "edmx:Reference", &attrs)?;
        return Ok(());
    }

    write_start(xml, "edmx:Reference", &attrs)?;
    for include in &reference.includes {
        write_include(xml, include)?;
    }
    for ia in &reference.include_annotations {
        let mut ia_attrs = vec![("TermNamespace".to_string(), ia.term_namespace.clone())];
        if let Some(q) = &ia.qualifier {
            ia_attrs.push(("Qualifier".to_string(), q.clone()));
        }
        if let Some(t) = &ia.target_namespace {
            ia_attrs.push(("TargetNamespace".to_string(), t.clone()));
        }
        write_empty(xml, "edmx:IncludeAnnotations", &ia_attrs)?;
    }
    write_end(xml, "edmx:Reference")
}

fn write_include<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    include: &Include,
) -> io::Result<()> {
    let mut attrs = vec![("Namespace".to_string(), include.namespace.clone())];
    if let Some(alias) = &include.alias {
        attrs.push(("Alias".to_string(), alias.clone()));
    }

    if include.annotations.is_empty() {
        return write_empty(xml, "edmx:Include", &attrs);
    }

    write_start(xml, "edmx:Include", &attrs)?;
    for annotation in &include.annotations {
        write_annotation(xml, annotation)?;
    }
    write_end(xml, "edmx:Include")
}

fn write_schema<W: io::Write>(xml: &mut quick_xml::Writer<W>, schema: &Schema) -> io::Result<()> {
    let mut attrs = vec![("Namespace".to_string(), schema.namespace.clone())];
    if let Some(alias) = &schema.alias {
        attrs.push(("Alias".to_string(), alias.clone()));
    }

    write_start(xml, "Schema", &attrs)?;
    for group in &schema.external_annotations {
        write_start(
            xml,
            "Annotations",
            &[("Target".into(), group.target.clone())],
        )?;
        for annotation in &group.annotations {
            write_annotation(xml, annotation)?;
        }
        write_end(xml, "Annotations")?;
    }
    for element in &schema.elements {
        match element {
            SchemaElement::EntityType(entity) => write_entity_type(xml, entity)?,
            SchemaElement::ComplexType(complex) => write_complex_type(xml, complex)?,
            SchemaElement::EnumType(enum_) => write_enum_type(xml, enum_)?,
            SchemaElement::TypeDefinition(td) => write_type_definition(xml, td)?,
            SchemaElement::Term(term) => write_term(xml, term)?,
            SchemaElement::Function(function) => write_callable(
                xml,
                "Function",
                &function.name,
                function.is_bound,
                function.is_composable,
                function.entity_set_path.as_deref(),
                &function.parameters,
                function.return_type.as_ref(),
                &function.annotations,
            )?,
            SchemaElement::Action(action) => write_callable(
                xml,
                "Action",
                &action.name,
                action.is_bound,
                None,
                action.entity_set_path.as_deref(),
                &action.parameters,
                action.return_type.as_ref(),
                &action.annotations,
            )?,
            SchemaElement::EntityContainer(container) => write_entity_container(xml, container)?,
        }
    }
    // Schema-level own- and sibling-target annotations as <Annotation>
    // children of <Schema>.
    for ann in &schema.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "Schema")
}

fn write_entity_type<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    entity: &EntityType,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), entity.name.clone())];
    if let Some(bt) = &entity.base_type {
        attrs.push(("BaseType".to_string(), bt.clone()));
    }
    push_optional_bool(&mut attrs, "Abstract", entity.abstract_);
    push_optional_bool(&mut attrs, "OpenType", entity.open_type);
    push_optional_bool(&mut attrs, "HasStream", entity.has_stream);

    if entity.key.is_none()
        && entity.properties.is_empty()
        && entity.navigation_properties.is_empty()
        && entity.annotations.is_empty()
    {
        return write_empty(xml, "EntityType", &attrs);
    }

    write_start(xml, "EntityType", &attrs)?;
    if let Some(key) = &entity.key {
        write_key(xml, key)?;
    }
    for property in &entity.properties {
        write_property(xml, property)?;
    }
    for navigation_property in &entity.navigation_properties {
        write_navigation_property(xml, navigation_property)?;
    }
    for ann in &entity.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "EntityType")
}

fn write_complex_type<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    complex: &ComplexType,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), complex.name.clone())];
    if let Some(bt) = &complex.base_type {
        attrs.push(("BaseType".to_string(), bt.clone()));
    }
    push_optional_bool(&mut attrs, "Abstract", complex.abstract_);
    push_optional_bool(&mut attrs, "OpenType", complex.open_type);
    if complex.properties.is_empty()
        && complex.navigation_properties.is_empty()
        && complex.annotations.is_empty()
    {
        return write_empty(xml, "ComplexType", &attrs);
    }

    write_start(xml, "ComplexType", &attrs)?;
    for property in &complex.properties {
        write_property(xml, property)?;
    }
    for navigation_property in &complex.navigation_properties {
        write_navigation_property(xml, navigation_property)?;
    }
    for ann in &complex.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "ComplexType")
}

fn write_enum_type<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    enum_: &EnumType,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), enum_.name.clone())];
    if let Some(t) = &enum_.underlying_type {
        attrs.push(("UnderlyingType".to_string(), t.clone()));
    }
    push_optional_bool(&mut attrs, "IsFlags", enum_.is_flags);

    if enum_.members.is_empty() && enum_.annotations.is_empty() {
        return write_empty(xml, "EnumType", &attrs);
    }

    write_start(xml, "EnumType", &attrs)?;
    for member in &enum_.members {
        let mut member_attrs = vec![("Name".to_string(), member.name.clone())];
        if let Some(v) = member.value {
            member_attrs.push(("Value".to_string(), v.to_string()));
        }
        if member.annotations.is_empty() {
            write_empty(xml, "Member", &member_attrs)?;
        } else {
            write_start(xml, "Member", &member_attrs)?;
            for annotation in &member.annotations {
                write_annotation(xml, annotation)?;
            }
            write_end(xml, "Member")?;
        }
    }
    for ann in &enum_.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "EnumType")
}

fn write_key<W: io::Write>(xml: &mut quick_xml::Writer<W>, key: &Key) -> io::Result<()> {
    if key.property_refs.is_empty() {
        return write_empty(xml, "Key", &[]);
    }

    write_start(xml, "Key", &[])?;
    for property_ref in &key.property_refs {
        let mut attrs = vec![("Name".to_string(), property_ref.name.clone())];
        if let Some(alias) = &property_ref.alias {
            attrs.push(("Alias".into(), alias.clone()));
        }
        write_empty(xml, "PropertyRef", &attrs)?;
    }
    write_end(xml, "Key")
}

/// If `v` is `Some`, push `(attr, "true" | "false")` onto `attrs`. Use this
/// for fields whose model type is `Option<bool>` and where the source's
/// explicit-vs-absent distinction matters (i.e. CSDL has a defined default).
fn push_optional_bool(attrs: &mut Vec<(String, String)>, attr: &str, v: Option<bool>) {
    if let Some(b) = v {
        attrs.push((
            attr.to_string(),
            if b { "true" } else { "false" }.to_string(),
        ));
    }
}

/// JSON-side counterpart of [`push_optional_bool`]: insert `(key, Bool(v))`
/// into the map when `v` is `Some`. Omits the key entirely on `None`.
fn insert_optional_bool(obj: &mut Map<String, Value>, key: &str, v: Option<bool>) {
    if let Some(b) = v {
        obj.insert(key.to_string(), Value::Bool(b));
    }
}

/// XML expresses collection-ness inline as `Collection(X)`. The model carries
/// the two pieces separately, so the XML writer re-wraps on emit.
fn xml_wrap_type(type_name: Option<&str>, is_collection: bool) -> Option<String> {
    match (type_name, is_collection) {
        (Some(t), true) => Some(format!("Collection({t})")),
        (Some(t), false) => Some(t.to_string()),
        // No type_name but is_collection=true is a malformed model — we'd
        // need *something* to wrap. Skip rather than emit Collection().
        (None, _) => None,
    }
}

fn write_property<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    property: &Property,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), property.name.clone())];
    if let Some(t) = xml_wrap_type(property.type_name.as_deref(), property.is_collection) {
        attrs.push(("Type".to_string(), t));
    }
    if let Some(nullable) = property.nullable {
        attrs.push((
            "Nullable".to_string(),
            if nullable { "true" } else { "false" }.to_string(),
        ));
    }
    if let Some(max_length) = property.max_length {
        attrs.push(("MaxLength".to_string(), max_length.as_str()));
    }
    if let Some(precision) = &property.precision {
        attrs.push(("Precision".to_string(), precision.clone()));
    }
    if let Some(scale) = property.scale {
        attrs.push(("Scale".to_string(), scale.as_str()));
    }
    if let Some(srid) = property.srid {
        attrs.push(("SRID".to_string(), srid.as_str()));
    }
    if let Some(unicode) = property.unicode {
        attrs.push((
            "Unicode".to_string(),
            if unicode { "true" } else { "false" }.to_string(),
        ));
    }
    if let Some(dv) = &property.default_value {
        attrs.push(("DefaultValue".to_string(), dv.clone()));
    }

    if property.annotations.is_empty() {
        return write_empty(xml, "Property", &attrs);
    }

    write_start(xml, "Property", &attrs)?;
    for annotation in &property.annotations {
        write_annotation(xml, annotation)?;
    }
    write_end(xml, "Property")
}

fn write_navigation_property<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    navigation_property: &NavigationProperty,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), navigation_property.name.clone())];
    if let Some(t) = xml_wrap_type(
        navigation_property.type_name.as_deref(),
        navigation_property.is_collection,
    ) {
        attrs.push(("Type".to_string(), t));
    }
    if let Some(nullable) = navigation_property.nullable {
        attrs.push((
            "Nullable".to_string(),
            if nullable { "true" } else { "false" }.to_string(),
        ));
    }
    if let Some(partner) = &navigation_property.partner {
        attrs.push(("Partner".to_string(), partner.clone()));
    }
    push_optional_bool(
        &mut attrs,
        "ContainsTarget",
        navigation_property.contains_target,
    );

    if navigation_property.on_delete.is_none()
        && navigation_property.referential_constraints.is_empty()
        && navigation_property.annotations.is_empty()
    {
        return write_empty(xml, "NavigationProperty", &attrs);
    }

    write_start(xml, "NavigationProperty", &attrs)?;
    if let Some(on_delete) = &navigation_property.on_delete {
        write_empty(
            xml,
            "OnDelete",
            &[("Action".to_string(), on_delete.as_str().to_string())],
        )?;
    }
    for constraint in &navigation_property.referential_constraints {
        let constraint_attrs = vec![
            ("Property".to_string(), constraint.property.clone()),
            (
                "ReferencedProperty".to_string(),
                constraint.referenced_property.clone(),
            ),
        ];
        if constraint.annotations.is_empty() {
            write_empty(xml, "ReferentialConstraint", &constraint_attrs)?;
        } else {
            write_start(xml, "ReferentialConstraint", &constraint_attrs)?;
            for ann in &constraint.annotations {
                write_annotation(xml, ann)?;
            }
            write_end(xml, "ReferentialConstraint")?;
        }
    }
    for ann in &navigation_property.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "NavigationProperty")
}

/// Function and Action share their XML shape exactly. Element name is the only
/// difference.
fn write_callable<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    element: &str,
    name: &str,
    is_bound: Option<bool>,
    is_composable: Option<bool>,
    entity_set_path: Option<&str>,
    parameters: &[Parameter],
    return_type: Option<&ReturnType>,
    annotations: &[Annotation],
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), name.to_string())];
    push_optional_bool(&mut attrs, "IsBound", is_bound);
    if let Some(p) = entity_set_path {
        attrs.push(("EntitySetPath".to_string(), p.to_string()));
    }
    if element == "Function" {
        push_optional_bool(&mut attrs, "IsComposable", is_composable);
    }
    if parameters.is_empty() && return_type.is_none() && annotations.is_empty() {
        return write_empty(xml, element, &attrs);
    }

    write_start(xml, element, &attrs)?;
    for parameter in parameters {
        let mut parameter_attrs = vec![("Name".to_string(), parameter.name.clone())];
        if let Some(t) = xml_wrap_type(parameter.type_name.as_deref(), parameter.is_collection) {
            parameter_attrs.push(("Type".to_string(), t));
        }
        if let Some(nullable) = parameter.nullable {
            parameter_attrs.push((
                "Nullable".to_string(),
                if nullable { "true" } else { "false" }.to_string(),
            ));
        }
        if let Some(v) = parameter.max_length {
            parameter_attrs.push(("MaxLength".to_string(), v.as_str()));
        }
        if let Some(v) = &parameter.precision {
            parameter_attrs.push(("Precision".to_string(), v.clone()));
        }
        if let Some(v) = parameter.scale {
            parameter_attrs.push(("Scale".to_string(), v.as_str()));
        }
        if let Some(v) = parameter.srid {
            parameter_attrs.push(("SRID".to_string(), v.as_str()));
        }
        if let Some(u) = parameter.unicode {
            parameter_attrs.push((
                "Unicode".to_string(),
                if u { "true" } else { "false" }.to_string(),
            ));
        }
        if let Some(dv) = &parameter.default_value {
            parameter_attrs.push(("DefaultValue".to_string(), dv.clone()));
        }
        if parameter.annotations.is_empty() {
            write_empty(xml, "Parameter", &parameter_attrs)?;
        } else {
            write_start(xml, "Parameter", &parameter_attrs)?;
            for ann in &parameter.annotations {
                write_annotation(xml, ann)?;
            }
            write_end(xml, "Parameter")?;
        }
    }

    if let Some(rt) = return_type {
        let mut return_type_attrs = Vec::new();
        if let Some(t) = xml_wrap_type(rt.type_name.as_deref(), rt.is_collection) {
            return_type_attrs.push(("Type".to_string(), t));
        }
        if let Some(n) = rt.nullable {
            return_type_attrs.push((
                "Nullable".to_string(),
                if n { "true" } else { "false" }.to_string(),
            ));
        }
        if let Some(v) = rt.max_length {
            return_type_attrs.push(("MaxLength".to_string(), v.as_str()));
        }
        if let Some(v) = &rt.precision {
            return_type_attrs.push(("Precision".to_string(), v.clone()));
        }
        if let Some(v) = rt.scale {
            return_type_attrs.push(("Scale".to_string(), v.as_str()));
        }
        if let Some(v) = rt.srid {
            return_type_attrs.push(("SRID".to_string(), v.as_str()));
        }
        if let Some(b) = rt.unicode {
            return_type_attrs.push((
                "Unicode".to_string(),
                if b { "true" } else { "false" }.to_string(),
            ));
        }
        if rt.annotations.is_empty() {
            write_empty(xml, "ReturnType", &return_type_attrs)?;
        } else {
            write_start(xml, "ReturnType", &return_type_attrs)?;
            for ann in &rt.annotations {
                write_annotation(xml, ann)?;
            }
            write_end(xml, "ReturnType")?;
        }
    }

    for ann in annotations {
        write_annotation(xml, ann)?;
    }

    write_end(xml, element)
}

fn write_type_definition<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    td: &TypeDefinition,
) -> io::Result<()> {
    let mut attrs = vec![
        ("Name".to_string(), td.name.clone()),
        ("UnderlyingType".to_string(), td.underlying_type.clone()),
    ];
    if let Some(v) = td.max_length {
        attrs.push(("MaxLength".to_string(), v.as_str()));
    }
    if let Some(v) = &td.precision {
        attrs.push(("Precision".to_string(), v.clone()));
    }
    if let Some(v) = td.scale {
        attrs.push(("Scale".to_string(), v.as_str()));
    }
    if let Some(v) = td.srid {
        attrs.push(("SRID".to_string(), v.as_str()));
    }
    if let Some(u) = td.unicode {
        attrs.push((
            "Unicode".to_string(),
            if u { "true" } else { "false" }.to_string(),
        ));
    }

    if td.annotations.is_empty() {
        return write_empty(xml, "TypeDefinition", &attrs);
    }
    write_start(xml, "TypeDefinition", &attrs)?;
    for a in &td.annotations {
        write_annotation(xml, a)?;
    }
    write_end(xml, "TypeDefinition")
}

fn write_term<W: io::Write>(xml: &mut quick_xml::Writer<W>, term: &Term) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), term.name.clone())];
    if let Some(t) = xml_wrap_type(term.type_name.as_deref(), term.is_collection) {
        attrs.push(("Type".to_string(), t));
    }
    if let Some(b) = &term.base_term {
        attrs.push(("BaseTerm".to_string(), b.clone()));
    }
    if let Some(d) = &term.default_value {
        attrs.push(("DefaultValue".to_string(), d.clone()));
    }
    if !term.applies_to.is_empty() {
        attrs.push(("AppliesTo".to_string(), term.applies_to.join(" ")));
    }
    if let Some(n) = term.nullable {
        attrs.push((
            "Nullable".to_string(),
            if n { "true" } else { "false" }.to_string(),
        ));
    }
    if let Some(v) = term.max_length {
        attrs.push(("MaxLength".to_string(), v.as_str()));
    }
    if let Some(v) = &term.precision {
        attrs.push(("Precision".to_string(), v.clone()));
    }
    if let Some(v) = term.scale {
        attrs.push(("Scale".to_string(), v.as_str()));
    }
    if let Some(v) = term.srid {
        attrs.push(("SRID".to_string(), v.as_str()));
    }
    if let Some(b) = term.unicode {
        attrs.push((
            "Unicode".to_string(),
            if b { "true" } else { "false" }.to_string(),
        ));
    }
    if term.annotations.is_empty() {
        return write_empty(xml, "Term", &attrs);
    }
    write_start(xml, "Term", &attrs)?;
    for a in &term.annotations {
        write_annotation(xml, a)?;
    }
    write_end(xml, "Term")
}

fn write_entity_container<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    container: &EntityContainer,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), container.name.clone())];
    if let Some(ext) = &container.extends {
        attrs.push(("Extends".to_string(), ext.clone()));
    }
    if container.entity_sets.is_empty()
        && container.singletons.is_empty()
        && container.function_imports.is_empty()
        && container.action_imports.is_empty()
        && container.annotations.is_empty()
    {
        return write_empty(xml, "EntityContainer", &attrs);
    }

    write_start(xml, "EntityContainer", &attrs)?;
    for entity_set in &container.entity_sets {
        write_entity_set(xml, entity_set)?;
    }
    for singleton in &container.singletons {
        write_singleton(xml, singleton)?;
    }
    for function_import in &container.function_imports {
        write_function_import(xml, function_import)?;
    }
    for action_import in &container.action_imports {
        write_action_import(xml, action_import)?;
    }
    for ann in &container.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "EntityContainer")
}

fn write_action_import<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    import: &ActionImport,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), import.name.clone())];
    if let Some(action) = &import.action {
        attrs.push(("Action".to_string(), action.clone()));
    }
    if let Some(es) = &import.entity_set {
        attrs.push(("EntitySet".to_string(), es.clone()));
    }
    if let Some(b) = import.include_in_service_document {
        attrs.push((
            "IncludeInServiceDocument".to_string(),
            if b { "true" } else { "false" }.to_string(),
        ));
    }
    if import.annotations.is_empty() {
        return write_empty(xml, "ActionImport", &attrs);
    }
    write_start(xml, "ActionImport", &attrs)?;
    for ann in &import.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "ActionImport")
}

fn write_entity_set<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    entity_set: &EntitySet,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), entity_set.name.clone())];
    if let Some(entity_type) = &entity_set.entity_type {
        attrs.push(("EntityType".to_string(), entity_type.clone()));
    }
    if let Some(b) = entity_set.include_in_service_document {
        attrs.push((
            "IncludeInServiceDocument".to_string(),
            if b { "true" } else { "false" }.to_string(),
        ));
    }

    if entity_set.navigation_property_bindings.is_empty() && entity_set.annotations.is_empty() {
        return write_empty(xml, "EntitySet", &attrs);
    }

    write_start(xml, "EntitySet", &attrs)?;
    for binding in &entity_set.navigation_property_bindings {
        write_empty(
            xml,
            "NavigationPropertyBinding",
            &[
                ("Path".to_string(), binding.path.clone()),
                ("Target".to_string(), binding.target.clone()),
            ],
        )?;
    }
    for annotation in &entity_set.annotations {
        write_annotation(xml, annotation)?;
    }
    write_end(xml, "EntitySet")
}

fn write_singleton<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    singleton: &Singleton,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), singleton.name.clone())];
    if let Some(nullable) = singleton.nullable {
        attrs.push(("Nullable".into(), nullable.to_string()));
    }
    if let Some(type_name) = &singleton.type_name {
        attrs.push(("Type".to_string(), type_name.clone()));
    }
    if let Some(b) = singleton.include_in_service_document {
        attrs.push((
            "IncludeInServiceDocument".to_string(),
            if b { "true" } else { "false" }.to_string(),
        ));
    }

    if singleton.navigation_property_bindings.is_empty() && singleton.annotations.is_empty() {
        return write_empty(xml, "Singleton", &attrs);
    }

    write_start(xml, "Singleton", &attrs)?;
    for binding in &singleton.navigation_property_bindings {
        write_empty(
            xml,
            "NavigationPropertyBinding",
            &[
                ("Path".to_string(), binding.path.clone()),
                ("Target".to_string(), binding.target.clone()),
            ],
        )?;
    }
    for annotation in &singleton.annotations {
        write_annotation(xml, annotation)?;
    }
    write_end(xml, "Singleton")
}

fn write_function_import<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    function_import: &FunctionImport,
) -> io::Result<()> {
    let mut attrs = vec![("Name".to_string(), function_import.name.clone())];
    if let Some(entity_set) = &function_import.entity_set {
        attrs.push(("EntitySet".to_string(), entity_set.clone()));
    }
    if let Some(function) = &function_import.function {
        attrs.push(("Function".to_string(), function.clone()));
    }
    if let Some(b) = function_import.include_in_service_document {
        attrs.push((
            "IncludeInServiceDocument".to_string(),
            if b { "true" } else { "false" }.to_string(),
        ));
    }

    if function_import.annotations.is_empty() {
        return write_empty(xml, "FunctionImport", &attrs);
    }
    write_start(xml, "FunctionImport", &attrs)?;
    for ann in &function_import.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "FunctionImport")
}

fn write_annotation<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    annotation: &Annotation,
) -> io::Result<()> {
    let mut attrs = vec![("Term".to_string(), annotation.term.clone())];
    if let Some(qualifier) = &annotation.qualifier {
        attrs.push(("Qualifier".to_string(), qualifier.clone()));
    }
    if let Some(target) = &annotation.target {
        attrs.push(("Target".to_string(), target.clone()));
    }

    if let Some(expr) = &annotation.expression {
        if let Some((key, value)) = inline_attr_expr(expr) {
            attrs.push((key.to_string(), value));
            return write_empty(xml, "Annotation", &attrs);
        }

        write_start(xml, "Annotation", &attrs)?;
        write_expr(xml, expr)?;
        return write_end(xml, "Annotation");
    }

    write_empty(xml, "Annotation", &attrs)
}

fn inline_attr_expr(expr: &CsdlAnnotationExpression) -> Option<(&'static str, String)> {
    match expr {
        CsdlAnnotationExpression::Binary(v) => {
            Some(("Binary", String::from_utf8_lossy(v.as_slice()).into_owned()))
        }
        CsdlAnnotationExpression::Bool(v) => {
            Some(("Bool", if *v { "true" } else { "false" }.to_string()))
        }
        CsdlAnnotationExpression::Date(v) => Some(("Date", v.clone())),
        CsdlAnnotationExpression::DateTimeOffset(v) => Some(("DateTimeOffset", v.clone())),
        CsdlAnnotationExpression::Decimal(v) => Some(("Decimal", v.clone())),
        CsdlAnnotationExpression::Duration(v) => Some(("Duration", v.clone())),
        CsdlAnnotationExpression::EnumMember(v) => Some(("EnumMember", v.clone())),
        CsdlAnnotationExpression::Float(v) => Some(("Float", v.to_string())),
        CsdlAnnotationExpression::Guid(v) => Some(("Guid", v.clone())),
        CsdlAnnotationExpression::Int(v) => Some(("Int", v.to_string())),
        CsdlAnnotationExpression::String(v) => Some(("String", v.clone())),
        CsdlAnnotationExpression::TimeOfDay(v) => Some(("TimeOfDay", v.clone())),
        CsdlAnnotationExpression::Path(v) => Some(("Path", v.clone())),
        CsdlAnnotationExpression::PropertyPath(v) => Some(("PropertyPath", v.clone())),
        CsdlAnnotationExpression::NavigationPropertyPath(v) => {
            Some(("NavigationPropertyPath", v.clone()))
        }
        CsdlAnnotationExpression::AnnotationPath(v) => Some(("AnnotationPath", v.clone())),
        _ => None,
    }
}

fn write_expr<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    expr: &CsdlAnnotationExpression,
) -> io::Result<()> {
    match expr {
        CsdlAnnotationExpression::Binary(v) => {
            write_text(xml, "Binary", &String::from_utf8_lossy(v.as_slice()))
        }
        CsdlAnnotationExpression::Bool(v) => {
            write_text(xml, "Bool", if *v { "true" } else { "false" })
        }
        CsdlAnnotationExpression::Date(v) => write_text(xml, "Date", v),
        CsdlAnnotationExpression::DateTimeOffset(v) => write_text(xml, "DateTimeOffset", v),
        CsdlAnnotationExpression::Decimal(v) => write_text(xml, "Decimal", v),
        CsdlAnnotationExpression::Duration(v) => write_text(xml, "Duration", v),
        CsdlAnnotationExpression::EnumMember(v) => write_text(xml, "EnumMember", v),
        CsdlAnnotationExpression::Float(v) => write_text(xml, "Float", &v.to_string()),
        CsdlAnnotationExpression::Guid(v) => write_text(xml, "Guid", v),
        CsdlAnnotationExpression::Int(v) => write_text(xml, "Int", &v.to_string()),
        CsdlAnnotationExpression::String(v) => write_text(xml, "String", v),
        CsdlAnnotationExpression::TimeOfDay(v) => write_text(xml, "TimeOfDay", v),
        CsdlAnnotationExpression::Null => write_empty(xml, "Null", &[]),
        CsdlAnnotationExpression::Path(v) => write_text(xml, "Path", v),
        CsdlAnnotationExpression::PropertyPath(v) => write_text(xml, "PropertyPath", v),
        CsdlAnnotationExpression::NavigationPropertyPath(v) => {
            write_text(xml, "NavigationPropertyPath", v)
        }
        CsdlAnnotationExpression::AnnotationPath(v) => write_text(xml, "AnnotationPath", v),
        CsdlAnnotationExpression::Not(inner) => {
            write_start(xml, "Not", &[])?;
            write_expr(xml, inner)?;
            write_end(xml, "Not")
        }
        CsdlAnnotationExpression::BinaryExpression { op, lhs, rhs } => {
            let op_name = match op {
                BinaryOperator::And => "And",
                BinaryOperator::Or => "Or",
                BinaryOperator::Eq => "Eq",
                BinaryOperator::Ne => "Ne",
                BinaryOperator::Gt => "Gt",
                BinaryOperator::Ge => "Ge",
                BinaryOperator::Lt => "Lt",
                BinaryOperator::Le => "Le",
            };
            write_start(xml, op_name, &[])?;
            write_expr(xml, lhs)?;
            write_expr(xml, rhs)?;
            write_end(xml, op_name)
        }
        CsdlAnnotationExpression::If { test, then_, else_ } => {
            write_start(xml, "If", &[])?;
            write_expr(xml, test)?;
            write_expr(xml, then_)?;
            if let Some(else_expr) = else_ {
                write_expr(xml, else_expr)?;
            }
            write_end(xml, "If")
        }
        CsdlAnnotationExpression::Apply { function, args } => {
            write_start(xml, "Apply", &[("Function".to_string(), function.clone())])?;
            for arg in args {
                write_expr(xml, arg)?;
            }
            write_end(xml, "Apply")
        }
        CsdlAnnotationExpression::Cast { type_, expr } => {
            write_start(xml, "Cast", &[("Type".to_string(), type_.clone())])?;
            write_expr(xml, expr)?;
            write_end(xml, "Cast")
        }
        CsdlAnnotationExpression::IsOf { type_, expr } => {
            write_start(xml, "IsOf", &[("Type".to_string(), type_.clone())])?;
            write_expr(xml, expr)?;
            write_end(xml, "IsOf")
        }
        CsdlAnnotationExpression::Record {
            type_,
            properties,
            annotations,
        } => {
            let mut attrs = Vec::new();
            if let Some(type_name) = type_ {
                attrs.push(("Type".to_string(), type_name.clone()));
            }

            if properties.is_empty() && annotations.is_empty() {
                return write_empty(xml, "Record", &attrs);
            }

            write_start(xml, "Record", &attrs)?;
            for property in properties {
                write_property_value(xml, property)?;
            }
            for ann in annotations {
                write_annotation(xml, ann)?;
            }
            write_end(xml, "Record")
        }
        CsdlAnnotationExpression::Collection(items) => {
            if items.is_empty() {
                return write_empty(xml, "Collection", &[]);
            }

            write_start(xml, "Collection", &[])?;
            for item in items {
                write_expr(xml, item)?;
            }
            write_end(xml, "Collection")
        }
        CsdlAnnotationExpression::LabeledElement { name, expr } => {
            write_start(xml, "LabeledElement", &[("Name".to_string(), name.clone())])?;
            write_expr(xml, expr)?;
            write_end(xml, "LabeledElement")
        }
        CsdlAnnotationExpression::LabeledElementReference(v) => {
            write_text(xml, "LabeledElementReference", v)
        }
        CsdlAnnotationExpression::UrlRef(inner) => {
            write_start(xml, "UrlRef", &[])?;
            write_expr(xml, inner)?;
            write_end(xml, "UrlRef")
        }
    }
}

fn write_property_value<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    property: &PropertyValue,
) -> io::Result<()> {
    let mut attrs = vec![("Property".to_string(), property.property.clone())];

    let has_annotations = !property.annotations.is_empty();

    if let Some(value) = &property.value {
        if let Some((key, value_text)) = inline_attr_expr(value) {
            attrs.push((key.to_string(), value_text));
            if !has_annotations {
                return write_empty(xml, "PropertyValue", &attrs);
            }
            // Inline-attribute value AND nested annotations: emit the value
            // as an attribute and the annotations as children.
            write_start(xml, "PropertyValue", &attrs)?;
            for ann in &property.annotations {
                write_annotation(xml, ann)?;
            }
            return write_end(xml, "PropertyValue");
        }

        write_start(xml, "PropertyValue", &attrs)?;
        write_expr(xml, value)?;
        for ann in &property.annotations {
            write_annotation(xml, ann)?;
        }
        return write_end(xml, "PropertyValue");
    }

    if !has_annotations {
        return write_empty(xml, "PropertyValue", &attrs);
    }
    write_start(xml, "PropertyValue", &attrs)?;
    for ann in &property.annotations {
        write_annotation(xml, ann)?;
    }
    write_end(xml, "PropertyValue")
}

fn write_start<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    name: &str,
    attrs: &[(String, String)],
) -> io::Result<()> {
    let mut start = BytesStart::new(name);
    for (key, value) in attrs {
        start.push_attribute((key.as_str(), value.as_str()));
    }
    xml.write_event(Event::Start(start))
}

fn write_empty<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    name: &str,
    attrs: &[(String, String)],
) -> io::Result<()> {
    let mut start = BytesStart::new(name);
    for (key, value) in attrs {
        start.push_attribute((key.as_str(), value.as_str()));
    }
    xml.write_event(Event::Empty(start))
}

fn write_end<W: io::Write>(xml: &mut quick_xml::Writer<W>, name: &str) -> io::Result<()> {
    xml.write_event(Event::End(BytesEnd::new(name)))
}

fn write_text<W: io::Write>(
    xml: &mut quick_xml::Writer<W>,
    name: &str,
    value: &str,
) -> io::Result<()> {
    write_start(xml, name, &[])?;
    xml.write_event(Event::Text(BytesText::new(value)))?;
    write_end(xml, name)
}

fn edmx_to_json(edmx: &Edmx) -> Value {
    let mut out = Map::new();
    if let Some(v) = &edmx.version {
        out.insert("$Version".to_string(), Value::String(v.clone()));
    }

    if !edmx.references.is_empty() {
        let mut refs = Map::new();
        for reference in &edmx.references {
            let mut item = Map::new();
            item.insert(
                "$Include".to_string(),
                Value::Array(
                    reference
                        .includes
                        .iter()
                        .map(include_to_json)
                        .collect::<Vec<_>>(),
                ),
            );
            if !reference.include_annotations.is_empty() {
                let arr = reference
                    .include_annotations
                    .iter()
                    .map(|ia| {
                        let mut o = Map::new();
                        o.insert(
                            "$TermNamespace".to_string(),
                            Value::String(ia.term_namespace.clone()),
                        );
                        if let Some(q) = &ia.qualifier {
                            o.insert("$Qualifier".to_string(), Value::String(q.clone()));
                        }
                        if let Some(t) = &ia.target_namespace {
                            o.insert("$TargetNamespace".to_string(), Value::String(t.clone()));
                        }
                        Value::Object(o)
                    })
                    .collect::<Vec<_>>();
                item.insert("$IncludeAnnotations".to_string(), Value::Array(arr));
            }
            refs.insert(reference.uri.clone(), Value::Object(item));
        }
        out.insert("$Reference".to_string(), Value::Object(refs));
    }

    for schema in &edmx.schemas {
        out.insert(schema.namespace.clone(), schema_to_json(schema));
    }
    if let Some(container) = &edmx.entity_container {
        out.insert("$EntityContainer".into(), Value::String(container.clone()));
    }

    defaults::normalize(&mut out, edmx);
    Value::Object(out)
}

fn include_to_json(include: &Include) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "$Namespace".to_string(),
        Value::String(include.namespace.clone()),
    );
    if let Some(alias) = &include.alias {
        obj.insert("$Alias".to_string(), Value::String(alias.clone()));
    }

    for ann in &include.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }

    Value::Object(obj)
}

fn schema_to_json(schema: &Schema) -> Value {
    let mut obj = Map::new();
    let mut groups = Map::new();
    for group in &schema.external_annotations {
        let target = groups
            .entry(group.target.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        let target = target.as_object_mut().expect("annotation target object");
        for annotation in &group.annotations {
            if let Some((key, value)) = annotation_member(annotation) {
                target.insert(key, value);
            }
        }
    }
    if !groups.is_empty() {
        obj.insert("$Annotations".into(), Value::Object(groups));
    }
    if let Some(alias) = &schema.alias {
        obj.insert("$Alias".to_string(), Value::String(alias.clone()));
    }

    let mut function_groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();

    for element in &schema.elements {
        match element {
            SchemaElement::EntityType(et) => {
                obj.insert(et.name.clone(), entity_type_to_json(et, &schema.namespace));
            }
            SchemaElement::ComplexType(ct) => {
                obj.insert(ct.name.clone(), complex_type_to_json(ct, &schema.namespace));
            }
            SchemaElement::EnumType(et) => {
                obj.insert(et.name.clone(), enum_type_to_json(et));
            }
            SchemaElement::TypeDefinition(td) => {
                obj.insert(td.name.clone(), type_definition_to_json(td));
            }
            SchemaElement::Term(term) => {
                obj.insert(term.name.clone(), term_to_json(term, &schema.namespace));
            }
            SchemaElement::Function(fun) => {
                function_groups
                    .entry(fun.name.clone())
                    .or_default()
                    .push(callable_to_json(
                        "Function",
                        fun.is_bound,
                        fun.is_composable,
                        fun.entity_set_path.as_deref(),
                        &fun.parameters,
                        fun.return_type.as_ref(),
                        &fun.annotations,
                        &schema.namespace,
                    ));
            }
            SchemaElement::Action(act) => {
                function_groups
                    .entry(act.name.clone())
                    .or_default()
                    .push(callable_to_json(
                        "Action",
                        act.is_bound,
                        None,
                        act.entity_set_path.as_deref(),
                        &act.parameters,
                        act.return_type.as_ref(),
                        &act.annotations,
                        &schema.namespace,
                    ));
            }
            SchemaElement::EntityContainer(ec) => {
                obj.insert(
                    ec.name.clone(),
                    entity_container_to_json(ec, &schema.namespace),
                );
            }
        }
    }

    for (name, overloads) in function_groups {
        obj.insert(name, Value::Array(overloads));
    }

    for ann in &schema.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }

    Value::Object(obj)
}

fn entity_type_to_json(entity: &EntityType, namespace: &str) -> Value {
    let mut obj = Map::new();
    obj.insert("$Kind".to_string(), Value::String("EntityType".to_string()));
    if let Some(bt) = &entity.base_type {
        obj.insert(
            "$BaseType".to_string(),
            Value::String(rewrite_type(bt, namespace)),
        );
    }
    insert_optional_bool(&mut obj, "$Abstract", entity.abstract_);
    insert_optional_bool(&mut obj, "$OpenType", entity.open_type);
    insert_optional_bool(&mut obj, "$HasStream", entity.has_stream);

    if let Some(key) = &entity.key {
        let keys = key
            .property_refs
            .iter()
            .map(|p| match &p.alias {
                Some(alias) => Value::Object(Map::from_iter([(
                    alias.clone(),
                    Value::String(p.name.clone()),
                )])),
                None => Value::String(p.name.clone()),
            })
            .collect::<Vec<_>>();
        obj.insert("$Key".to_string(), Value::Array(keys));
    }

    for prop in &entity.properties {
        obj.insert(prop.name.clone(), property_to_json(prop, namespace));
    }

    for nav in &entity.navigation_properties {
        obj.insert(
            nav.name.clone(),
            navigation_property_to_json(nav, namespace),
        );
    }

    for ann in &entity.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }

    Value::Object(obj)
}

fn enum_type_to_json(enum_: &EnumType) -> Value {
    let mut obj = Map::new();
    obj.insert("$Kind".to_string(), Value::String("EnumType".to_string()));
    if let Some(t) = &enum_.underlying_type {
        // CSDL JSON omits $UnderlyingType when it matches the default Edm.Int32.
        if t != "Edm.Int32" {
            obj.insert("$UnderlyingType".to_string(), Value::String(t.clone()));
        }
    }
    insert_optional_bool(&mut obj, "$IsFlags", enum_.is_flags);
    for (index, member) in enum_.members.iter().enumerate() {
        let v = match member.value {
            Some(v) => Value::Number(Number::from(v)),
            None => Value::Number(Number::from(index as u64)),
        };
        if !obj.contains_key(&member.name) {
            obj.insert(member.name.clone(), v);
        }
        // Per-member annotations: emit as `MemberName@Term` sibling-target
        // keys so they round-trip with `EnumType.annotations[target=Some(...)]`
        // on the JSON read path.
        for ann in &member.annotations {
            if let Some((k, v)) = annotation_member(ann) {
                let ann_key = format!("{}{k}", member.name);
                if !obj.contains_key(&ann_key) {
                    obj.insert(ann_key, v);
                }
            }
        }
    }
    // Type-level own- and sibling-target annotations land as top-level keys.
    for ann in &enum_.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn complex_type_to_json(complex: &ComplexType, namespace: &str) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "$Kind".to_string(),
        Value::String("ComplexType".to_string()),
    );
    if let Some(bt) = &complex.base_type {
        obj.insert(
            "$BaseType".to_string(),
            Value::String(rewrite_type(bt, namespace)),
        );
    }
    insert_optional_bool(&mut obj, "$Abstract", complex.abstract_);
    insert_optional_bool(&mut obj, "$OpenType", complex.open_type);

    for prop in &complex.properties {
        obj.insert(prop.name.clone(), property_to_json(prop, namespace));
    }

    for nav in &complex.navigation_properties {
        obj.insert(
            nav.name.clone(),
            navigation_property_to_json(nav, namespace),
        );
    }
    for ann in &complex.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }

    Value::Object(obj)
}

fn property_to_json(property: &Property, namespace: &str) -> Value {
    let mut obj = Map::new();

    write_type_facets(
        &mut obj,
        property.type_name.as_deref(),
        property.is_collection,
        property
            .nullable
            .or_else(|| (!property.is_collection).then_some(true)),
        property.max_length,
        namespace,
    );

    if let Some(v) = &property.precision {
        obj.insert("$Precision".to_string(), facet_value(v));
    }
    if let Some(v) = &property.scale {
        obj.insert("$Scale".to_string(), scale_facet_value(*v));
    }
    if let Some(v) = &property.srid {
        obj.insert("$SRID".to_string(), srid_facet_value(*v));
    }
    if let Some(b) = property.unicode {
        obj.insert("$Unicode".to_string(), Value::Bool(b));
    }
    if let Some(dv) = &property.default_value {
        obj.insert("$DefaultValue".to_string(), Value::String(dv.clone()));
    }

    for ann in &property.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }

    Value::Object(obj)
}

fn navigation_property_to_json(nav: &NavigationProperty, namespace: &str) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "$Kind".to_string(),
        Value::String("NavigationProperty".to_string()),
    );

    if nav.is_collection {
        obj.insert("$Collection".to_string(), Value::Bool(true));
    }
    if let Some(t) = &nav.type_name {
        obj.insert(
            "$Type".to_string(),
            Value::String(rewrite_type(t, namespace)),
        );
    }

    // Preserve XML's implicit true when writing JSON, whose default is false.
    if let Some(n) = nav
        .nullable
        .or_else(|| (!nav.is_collection).then_some(true))
    {
        obj.insert("$Nullable".to_string(), Value::Bool(n));
    }

    if let Some(partner) = &nav.partner {
        obj.insert("$Partner".to_string(), Value::String(partner.clone()));
    }
    insert_optional_bool(&mut obj, "$ContainsTarget", nav.contains_target);

    if let Some(on_delete) = &nav.on_delete {
        obj.insert(
            "$OnDelete".to_string(),
            Value::String(on_delete.as_str().to_string()),
        );
    }

    // Partition nav-level annotations: those whose `target` names one of our
    // constraints are routed into the $ReferentialConstraint map so they end
    // up as sibling-target keys *next to* the constraint they describe; the
    // rest stay at the NavigationProperty level.
    let constraint_names: std::collections::HashSet<&str> = nav
        .referential_constraints
        .iter()
        .map(|c| c.property.as_str())
        .collect();
    let (rc_targeted, top_level): (Vec<_>, Vec<_>) = nav
        .annotations
        .iter()
        .partition(|a| matches!(&a.target, Some(t) if constraint_names.contains(t.as_str())));

    if !nav.referential_constraints.is_empty() || !rc_targeted.is_empty() {
        // CSDL JSON shape is a flat map {Property: ReferencedProperty, …}.
        // Per-constraint own-annotations and nav-level annotations targeting
        // a constraint both end up as sibling-target keys here.
        let mut constraints = Map::new();
        for c in &nav.referential_constraints {
            constraints.insert(
                c.property.clone(),
                Value::String(c.referenced_property.clone()),
            );
            for ann in &c.annotations {
                if let Some((k, v)) = annotation_member(ann) {
                    // annotation_member returns the own-annotation form
                    // (`@Term` / `@Term#Q`); splice the property name to make
                    // it sibling-target relative to this constraint entry.
                    let key = format!("{}{k}", c.property);
                    constraints.insert(key, v);
                }
            }
        }
        for ann in rc_targeted {
            if let Some((k, v)) = annotation_member(ann) {
                constraints.insert(k, v);
            }
        }
        obj.insert(
            "$ReferentialConstraint".to_string(),
            Value::Object(constraints),
        );
    }

    for ann in top_level {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }

    Value::Object(obj)
}

fn callable_to_json(
    kind: &str,
    is_bound: Option<bool>,
    is_composable: Option<bool>,
    entity_set_path: Option<&str>,
    parameters: &[Parameter],
    return_type: Option<&ReturnType>,
    annotations: &[Annotation],
    namespace: &str,
) -> Value {
    let mut obj = Map::new();
    obj.insert("$Kind".to_string(), Value::String(kind.to_string()));
    insert_optional_bool(&mut obj, "$IsBound", is_bound);
    if let Some(p) = entity_set_path {
        obj.insert("$EntitySetPath".to_string(), Value::String(p.to_string()));
    }
    if kind == "Function" {
        insert_optional_bool(&mut obj, "$IsComposable", is_composable);
    }

    if !parameters.is_empty() {
        obj.insert(
            "$Parameter".to_string(),
            Value::Array(
                parameters
                    .iter()
                    .map(|p| parameter_to_json(p, namespace))
                    .collect::<Vec<_>>(),
            ),
        );
    }

    if let Some(rt) = return_type {
        let mut rt_obj = Map::new();
        write_type_facets(
            &mut rt_obj,
            rt.type_name.as_deref(),
            rt.is_collection,
            rt.nullable.or_else(|| (!rt.is_collection).then_some(true)),
            rt.max_length,
            namespace,
        );
        if let Some(v) = &rt.precision {
            rt_obj.insert("$Precision".to_string(), facet_value(v));
        }
        if let Some(v) = &rt.scale {
            rt_obj.insert("$Scale".to_string(), scale_facet_value(*v));
        }
        if let Some(v) = &rt.srid {
            rt_obj.insert("$SRID".to_string(), srid_facet_value(*v));
        }
        if let Some(b) = rt.unicode {
            rt_obj.insert("$Unicode".to_string(), Value::Bool(b));
        }
        for ann in &rt.annotations {
            if let Some((k, v)) = annotation_member(ann) {
                rt_obj.insert(k, v);
            }
        }
        obj.insert("$ReturnType".to_string(), Value::Object(rt_obj));
    }

    for ann in annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }

    Value::Object(obj)
}

fn type_definition_to_json(td: &TypeDefinition) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "$Kind".to_string(),
        Value::String("TypeDefinition".to_string()),
    );
    obj.insert(
        "$UnderlyingType".to_string(),
        Value::String(td.underlying_type.clone()),
    );
    if let Some(v) = &td.max_length {
        obj.insert("$MaxLength".to_string(), max_length_facet_value(*v));
    }
    if let Some(v) = &td.precision {
        obj.insert("$Precision".to_string(), facet_value(v));
    }
    if let Some(v) = &td.scale {
        obj.insert("$Scale".to_string(), scale_facet_value(*v));
    }
    if let Some(v) = &td.srid {
        obj.insert("$SRID".to_string(), srid_facet_value(*v));
    }
    if let Some(b) = td.unicode {
        obj.insert("$Unicode".to_string(), Value::Bool(b));
    }
    for ann in &td.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn term_to_json(term: &Term, namespace: &str) -> Value {
    let mut obj = Map::new();
    obj.insert("$Kind".to_string(), Value::String("Term".to_string()));
    write_type_facets(
        &mut obj,
        term.type_name.as_deref(),
        term.is_collection,
        term.nullable,
        term.max_length,
        namespace,
    );
    if let Some(v) = &term.precision {
        obj.insert("$Precision".to_string(), facet_value(v));
    }
    if let Some(v) = &term.scale {
        obj.insert("$Scale".to_string(), scale_facet_value(*v));
    }
    if let Some(v) = &term.srid {
        obj.insert("$SRID".to_string(), srid_facet_value(*v));
    }
    if let Some(b) = term.unicode {
        obj.insert("$Unicode".to_string(), Value::Bool(b));
    }
    if let Some(b) = &term.base_term {
        obj.insert(
            "$BaseTerm".to_string(),
            Value::String(rewrite_type(b, namespace)),
        );
    }
    if let Some(d) = &term.default_value {
        obj.insert("$DefaultValue".to_string(), Value::String(d.clone()));
    }
    if !term.applies_to.is_empty() {
        obj.insert(
            "$AppliesTo".to_string(),
            Value::Array(
                term.applies_to
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
    }
    for ann in &term.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn facet_value(raw: &str) -> Value {
    raw.parse::<u64>()
        .map(|n| Value::Number(Number::from(n)))
        .unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn max_length_facet_value(value: MaxLengthFacet) -> Value {
    match value {
        MaxLengthFacet::Number(n) => Value::Number(Number::from(n)),
        MaxLengthFacet::Max => Value::String("max".to_string()),
    }
}

fn scale_facet_value(value: ScaleFacet) -> Value {
    match value {
        ScaleFacet::Number(n) => Value::Number(Number::from(n)),
        ScaleFacet::Floating => Value::String("floating".into()),
        ScaleFacet::Variable => Value::String("variable".to_string()),
    }
}

fn srid_facet_value(value: SridFacet) -> Value {
    match value {
        SridFacet::Number(n) => Value::Number(Number::from(n)),
        SridFacet::Variable => Value::String("variable".to_string()),
    }
}

fn parameter_to_json(parameter: &Parameter, namespace: &str) -> Value {
    let mut obj = Map::new();
    obj.insert("$Name".to_string(), Value::String(parameter.name.clone()));
    write_type_facets(
        &mut obj,
        parameter.type_name.as_deref(),
        parameter.is_collection,
        parameter
            .nullable
            .or_else(|| (!parameter.is_collection).then_some(true)),
        parameter.max_length,
        namespace,
    );
    if let Some(v) = &parameter.precision {
        obj.insert("$Precision".to_string(), facet_value(v));
    }
    if let Some(v) = &parameter.scale {
        obj.insert("$Scale".to_string(), scale_facet_value(*v));
    }
    if let Some(v) = &parameter.srid {
        obj.insert("$SRID".to_string(), srid_facet_value(*v));
    }
    if let Some(b) = parameter.unicode {
        obj.insert("$Unicode".to_string(), Value::Bool(b));
    }
    if let Some(dv) = &parameter.default_value {
        obj.insert("$DefaultValue".to_string(), Value::String(dv.clone()));
    }
    for ann in &parameter.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn entity_container_to_json(container: &EntityContainer, namespace: &str) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "$Kind".to_string(),
        Value::String("EntityContainer".to_string()),
    );
    if let Some(ext) = &container.extends {
        obj.insert("$Extends".to_string(), Value::String(ext.clone()));
    }

    for set in &container.entity_sets {
        obj.insert(set.name.clone(), entity_set_to_json(set, namespace));
    }

    for singleton in &container.singletons {
        obj.insert(
            singleton.name.clone(),
            singleton_to_json(singleton, namespace),
        );
    }

    for import in &container.function_imports {
        obj.insert(
            import.name.clone(),
            function_import_to_json(import, namespace),
        );
    }

    for import in &container.action_imports {
        obj.insert(
            import.name.clone(),
            action_import_to_json(import, namespace),
        );
    }

    for ann in &container.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }

    Value::Object(obj)
}

fn action_import_to_json(import: &ActionImport, namespace: &str) -> Value {
    let mut obj = Map::new();
    if let Some(action) = &import.action {
        obj.insert(
            "$Action".to_string(),
            Value::String(rewrite_type(action, namespace)),
        );
    }
    if let Some(es) = &import.entity_set {
        obj.insert("$EntitySet".to_string(), Value::String(es.clone()));
    }
    if let Some(b) = import.include_in_service_document {
        obj.insert("$IncludeInServiceDocument".to_string(), Value::Bool(b));
    }
    for ann in &import.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn entity_set_to_json(set: &EntitySet, namespace: &str) -> Value {
    let mut obj = Map::new();
    obj.insert("$Collection".to_string(), Value::Bool(true));
    if let Some(entity_type) = &set.entity_type {
        obj.insert(
            "$Type".to_string(),
            Value::String(rewrite_type(entity_type, namespace)),
        );
    }
    if let Some(b) = set.include_in_service_document {
        obj.insert("$IncludeInServiceDocument".to_string(), Value::Bool(b));
    }
    write_nav_bindings(&mut obj, &set.navigation_property_bindings);
    for ann in &set.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn singleton_to_json(singleton: &Singleton, namespace: &str) -> Value {
    let mut obj = Map::new();
    insert_optional_bool(&mut obj, "$Nullable", singleton.nullable);
    if let Some(type_name) = &singleton.type_name {
        obj.insert(
            "$Type".to_string(),
            Value::String(rewrite_type(type_name, namespace)),
        );
    }
    if let Some(b) = singleton.include_in_service_document {
        obj.insert("$IncludeInServiceDocument".to_string(), Value::Bool(b));
    }
    write_nav_bindings(&mut obj, &singleton.navigation_property_bindings);
    for ann in &singleton.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn function_import_to_json(import: &FunctionImport, namespace: &str) -> Value {
    let mut obj = Map::new();
    if let Some(entity_set) = &import.entity_set {
        obj.insert("$EntitySet".to_string(), Value::String(entity_set.clone()));
    }
    if let Some(function) = &import.function {
        obj.insert(
            "$Function".to_string(),
            Value::String(rewrite_type(function, namespace)),
        );
    }
    if let Some(b) = import.include_in_service_document {
        obj.insert("$IncludeInServiceDocument".to_string(), Value::Bool(b));
    }
    for ann in &import.annotations {
        if let Some((k, v)) = annotation_member(ann) {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn write_nav_bindings(obj: &mut Map<String, Value>, bindings: &[NavigationPropertyBinding]) {
    if bindings.is_empty() {
        return;
    }

    let mut map = Map::new();
    for b in bindings {
        map.insert(b.path.clone(), Value::String(b.target.clone()));
    }
    obj.insert("$NavigationPropertyBinding".to_string(), Value::Object(map));
}

/// Write the type-and-facet keys (`$Type`, `$Collection`, `$Nullable`,
/// `$MaxLength`) into a JSON object. `type_name` is the *unwrapped* element
/// type; `is_collection` is carried separately. Nullable is emitted only when
/// `Some` — the JSON-side default lives in the semantic layer, not the model.
fn write_type_facets(
    obj: &mut Map<String, Value>,
    type_name: Option<&str>,
    is_collection: bool,
    nullable: Option<bool>,
    max_length: Option<MaxLengthFacet>,
    namespace: &str,
) {
    if is_collection {
        obj.insert("$Collection".to_string(), Value::Bool(true));
    }
    if let Some(t) = type_name {
        let rewritten = rewrite_type(t, namespace);
        // `Edm.String` is the JSON default; omit when it matches so we don't
        // emit redundant `$Type: "Edm.String"`. Same convention applied
        // elsewhere for other defaults.
        if rewritten != "Edm.String" {
            obj.insert("$Type".to_string(), Value::String(rewritten));
        }
    }

    if let Some(n) = nullable {
        obj.insert("$Nullable".to_string(), Value::Bool(n));
    }

    if let Some(max_len) = max_length {
        obj.insert("$MaxLength".to_string(), max_length_facet_value(max_len));
    }
}

fn annotation_member(annotation: &Annotation) -> Option<(String, Value)> {
    if annotation.term.is_empty() {
        return None;
    }

    // Sibling-target form: `Target@Term[#Qualifier]`. Own-annotation form:
    // `@Term[#Qualifier]` (target prefix empty).
    let mut key = match &annotation.target {
        Some(t) => format!("{t}@{}", annotation.term),
        None => format!("@{}", annotation.term),
    };
    if let Some(q) = &annotation.qualifier {
        key.push('#');
        key.push_str(q);
    }

    let value = match &annotation.expression {
        Some(expr) => expr_to_annotation_json(expr),
        None => Value::Null,
    };

    Some((key, value))
}

fn rewrite_type(value: &str, namespace: &str) -> String {
    let _ = namespace;
    value.to_string()
}

fn expr_to_json(expr: &CsdlAnnotationExpression) -> Value {
    match expr {
        CsdlAnnotationExpression::Bool(v) => Value::Bool(*v),
        CsdlAnnotationExpression::Int(v) => Value::Number(Number::from(*v)),
        CsdlAnnotationExpression::Float(v) => Number::from_f64(*v)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(v.to_string())),
        CsdlAnnotationExpression::String(v) => Value::String(v.clone()),
        CsdlAnnotationExpression::Path(v) => {
            let mut m = Map::new();
            m.insert("$Path".to_string(), Value::String(v.clone()));
            Value::Object(m)
        }
        CsdlAnnotationExpression::PropertyPath(v) => {
            let mut m = Map::new();
            m.insert("$PropertyPath".to_string(), Value::String(v.clone()));
            Value::Object(m)
        }
        CsdlAnnotationExpression::NavigationPropertyPath(v) => {
            let mut m = Map::new();
            m.insert(
                "$NavigationPropertyPath".to_string(),
                Value::String(v.clone()),
            );
            Value::Object(m)
        }
        CsdlAnnotationExpression::AnnotationPath(v) => {
            let mut m = Map::new();
            m.insert("$AnnotationPath".to_string(), Value::String(v.clone()));
            Value::Object(m)
        }
        CsdlAnnotationExpression::Collection(items) => {
            Value::Array(items.iter().map(expr_to_json).collect::<Vec<_>>())
        }
        CsdlAnnotationExpression::Null => Value::Null,
        CsdlAnnotationExpression::Record {
            type_,
            properties,
            annotations,
        } => {
            let mut m = Map::new();
            if let Some(t) = type_ {
                m.insert("$Type".to_string(), Value::String(t.clone()));
            }
            for pv in properties {
                if let Some(v) = &pv.value {
                    m.insert(pv.property.clone(), expr_to_json(v));
                }
                // PropertyValue's own annotations emit as Property@Term keys
                // sibling to the value entry. annotation_member returns the
                // `@Term[#Q]` form; splice the property name to make it a
                // sibling-target on `pv.property`.
                for ann in &pv.annotations {
                    if let Some((k, v)) = annotation_member(ann) {
                        m.insert(format!("{}{k}", pv.property), v);
                    }
                }
            }
            // Record-level annotations land at top level of the Record object.
            for ann in annotations {
                if let Some((k, v)) = annotation_member(ann) {
                    m.insert(k, v);
                }
            }
            Value::Object(m)
        }
        CsdlAnnotationExpression::Binary(v) => {
            Value::String(String::from_utf8_lossy(v.as_slice()).into_owned())
        }
        CsdlAnnotationExpression::Date(v)
        | CsdlAnnotationExpression::DateTimeOffset(v)
        | CsdlAnnotationExpression::Decimal(v)
        | CsdlAnnotationExpression::Duration(v)
        | CsdlAnnotationExpression::EnumMember(v)
        | CsdlAnnotationExpression::Guid(v)
        | CsdlAnnotationExpression::TimeOfDay(v)
        | CsdlAnnotationExpression::LabeledElementReference(v) => Value::String(v.clone()),
        CsdlAnnotationExpression::Not(inner) => {
            let mut m = Map::new();
            m.insert("$Not".to_string(), expr_to_json(inner));
            Value::Object(m)
        }
        CsdlAnnotationExpression::BinaryExpression { op, lhs, rhs } => {
            let key = match op {
                BinaryOperator::And => "$And",
                BinaryOperator::Or => "$Or",
                BinaryOperator::Eq => "$Eq",
                BinaryOperator::Ne => "$Ne",
                BinaryOperator::Gt => "$Gt",
                BinaryOperator::Ge => "$Ge",
                BinaryOperator::Lt => "$Lt",
                BinaryOperator::Le => "$Le",
            };
            let mut m = Map::new();
            m.insert(
                key.to_string(),
                Value::Array(vec![expr_to_json(lhs), expr_to_json(rhs)]),
            );
            Value::Object(m)
        }
        CsdlAnnotationExpression::If { test, then_, else_ } => {
            let mut arr = vec![expr_to_json(test), expr_to_json(then_)];
            if let Some(e) = else_ {
                arr.push(expr_to_json(e));
            }
            let mut m = Map::new();
            m.insert("$If".to_string(), Value::Array(arr));
            Value::Object(m)
        }
        CsdlAnnotationExpression::Apply { function, args } => {
            let mut m = Map::new();
            m.insert("$Function".to_string(), Value::String(function.clone()));
            m.insert(
                "$Apply".to_string(),
                Value::Array(args.iter().map(expr_to_json).collect::<Vec<_>>()),
            );
            Value::Object(m)
        }
        CsdlAnnotationExpression::Cast { type_, expr } => {
            let mut m = Map::new();
            m.insert("$Cast".to_string(), expr_to_json(expr));
            m.insert("$Type".to_string(), Value::String(type_.clone()));
            Value::Object(m)
        }
        CsdlAnnotationExpression::IsOf { type_, expr } => {
            let mut m = Map::new();
            m.insert("$IsOf".to_string(), expr_to_json(expr));
            m.insert("$Type".to_string(), Value::String(type_.clone()));
            Value::Object(m)
        }
        CsdlAnnotationExpression::LabeledElement { name, expr } => {
            let mut m = Map::new();
            m.insert("$LabeledElement".to_string(), expr_to_json(expr));
            m.insert("$Name".to_string(), Value::String(name.clone()));
            Value::Object(m)
        }
        CsdlAnnotationExpression::UrlRef(inner) => {
            let mut m = Map::new();
            m.insert("$UrlRef".to_string(), expr_to_json(inner));
            Value::Object(m)
        }
    }
}

fn expr_to_annotation_json(expr: &CsdlAnnotationExpression) -> Value {
    // Previously this collapsed `Collection<PropertyPath>` to a bare string
    // array (the CSDL JSON shorthand for `Collection(Edm.PropertyPath)`).
    // Without vocabulary metadata the reader can't tell whether a string
    // array means `Collection<String>` or `Collection<PropertyPath>`, so the
    // shorthand broke XML→JSON→XML round-trips. We now emit the explicit
    // `[{"$PropertyPath": "..."}]` form instead, which round-trips losslessly.
    expr_to_json(expr)
}
