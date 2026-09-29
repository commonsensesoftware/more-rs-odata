use std::io;

use crate::csdl::*;
use crate::csdl_xml_reader::{CsdlReader, SyntaxUnit};
use crate::expr::CsdlAnnotationExpression;

#[derive(Debug, Clone)]
struct RawElement {
    name: String,
    attributes: Vec<(String, String)>,
    annotation_expressions: Vec<CsdlAnnotationExpression>,
    children: Vec<RawElement>,
}

pub fn from_xml_reader<R: io::BufRead>(mut reader: R) -> io::Result<CsdlDocument> {
    let mut csdl_reader = CsdlReader::from_reader(&mut reader);
    from_units(|| {
        csdl_reader
            .next_unit()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
    })
}

pub fn from_json_reader<R: io::Read>(mut reader: R) -> io::Result<CsdlDocument> {
    let value: serde_json::Value = serde_json::from_reader(&mut reader)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    let mut units = crate::csdl_json_reader::JsonCsdlReader::new(value)
        .into_units()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?
        .into_iter();
    from_units(|| Ok(units.next()))
}

/// Drives the shared SyntaxUnit→RawElement→CsdlDocument loop. `next_unit`
/// returns `Ok(None)` at EOF.
fn from_units<F>(mut next_unit: F) -> io::Result<CsdlDocument>
where
    F: FnMut() -> io::Result<Option<SyntaxUnit>>,
{
    let mut stack: Vec<RawElement> = Vec::new();
    let mut roots: Vec<RawElement> = Vec::new();

    loop {
        let Some(unit) = next_unit()? else {
            break;
        };

        match unit {
            SyntaxUnit::StartElement { name, attributes } => {
                stack.push(RawElement {
                    name,
                    attributes,
                    annotation_expressions: Vec::new(),
                    children: Vec::new(),
                });
            }
            SyntaxUnit::AnnotationExpression(expr) => {
                let current = stack.last_mut().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "annotation expression outside any element",
                    )
                })?;
                current.annotation_expressions.push(expr);
            }
            SyntaxUnit::EndElement { name } => {
                let node = stack.pop().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("end element </{name}> without matching start"),
                    )
                })?;

                if node.name != name {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "mismatched closing tag: expected </{}> but got </{}>",
                            node.name, name
                        ),
                    ));
                }

                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    roots.push(node);
                }
            }
        }
    }

    if !stack.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "document ended before all start elements were closed",
        ));
    }

    let edmx = roots
        .iter()
        .find(|n| n.name == "Edmx")
        .map(parse_edmx)
        .transpose()?;

    Ok(CsdlDocument { edmx })
}

fn parse_edmx(raw: &RawElement) -> io::Result<Edmx> {
    let version = raw_attr(raw, "Version").map(str::to_string);
    let mut references = Vec::new();
    let mut schemas = Vec::new();

    for child in &raw.children {
        match child.name.as_str() {
            "Reference" => references.push(parse_reference(child)?),
            "DataServices" => {
                for schema in child.children.iter().filter(|n| n.name == "Schema") {
                    schemas.push(parse_schema(schema)?);
                }
            }
            "Schema" => schemas.push(parse_schema(child)?),
            _ => {}
        }
    }

    Ok(Edmx {
        version,
        references,
        schemas,
    })
}

fn parse_reference(raw: &RawElement) -> io::Result<Reference> {
    let uri = raw_attr(raw, "Uri").unwrap_or("").to_string();
    let mut includes = Vec::new();

    for include in raw.children.iter().filter(|n| n.name == "Include") {
        includes.push(parse_include(include)?);
    }

    let include_annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "IncludeAnnotations")
        .map(parse_include_annotations)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(Reference {
        uri,
        includes,
        include_annotations,
    })
}

fn parse_include_annotations(raw: &RawElement) -> io::Result<IncludeAnnotations> {
    Ok(IncludeAnnotations {
        term_namespace: raw_attr(raw, "TermNamespace").unwrap_or("").to_string(),
        qualifier: raw_attr(raw, "Qualifier").map(str::to_string),
        target_namespace: raw_attr(raw, "TargetNamespace").map(str::to_string),
    })
}

fn parse_include(raw: &RawElement) -> io::Result<Include> {
    let namespace = raw_attr(raw, "Namespace").unwrap_or("").to_string();
    let alias = raw_attr(raw, "Alias").map(str::to_string);
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(Include {
        namespace,
        alias,
        annotations,
    })
}

fn parse_schema(raw: &RawElement) -> io::Result<Schema> {
    let namespace = raw_attr(raw, "Namespace").unwrap_or("").to_string();
    let alias = raw_attr(raw, "Alias").map(str::to_string);
    let mut elements = Vec::new();

    for child in &raw.children {
        match child.name.as_str() {
            "EntityType" => elements.push(SchemaElement::EntityType(parse_entity_type(child)?)),
            "ComplexType" => elements.push(SchemaElement::ComplexType(parse_complex_type(child)?)),
            "EnumType" => elements.push(SchemaElement::EnumType(parse_enum_type(child)?)),
            "TypeDefinition" => {
                elements.push(SchemaElement::TypeDefinition(parse_type_definition(child)?));
            }
            "Term" => elements.push(SchemaElement::Term(parse_term(child)?)),
            "Function" => elements.push(SchemaElement::Function(parse_function(child)?)),
            "Action" => elements.push(SchemaElement::Action(parse_action(child)?)),
            "EntityContainer" => {
                elements.push(SchemaElement::EntityContainer(parse_entity_container(
                    child,
                )?));
            }
            _ => {}
        }
    }

    Ok(Schema {
        namespace,
        alias,
        elements,
        annotations: parse_annotations(raw)?,
    })
}

fn parse_entity_type(raw: &RawElement) -> io::Result<EntityType> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let base_type = raw_attr(raw, "BaseType").map(str::to_string);
    let abstract_ = raw_attr_bool(raw, "Abstract");
    let open_type = raw_attr_bool(raw, "OpenType");
    let has_stream = raw_attr_bool(raw, "HasStream");

    let key = raw
        .children
        .iter()
        .find(|n| n.name == "Key")
        .map(parse_key)
        .transpose()?;

    let properties = raw
        .children
        .iter()
        .filter(|n| n.name == "Property")
        .map(parse_property)
        .collect::<io::Result<Vec<_>>>()?;

    let navigation_properties = raw
        .children
        .iter()
        .filter(|n| n.name == "NavigationProperty")
        .map(parse_navigation_property)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(EntityType {
        name,
        base_type,
        abstract_,
        open_type,
        has_stream,
        key,
        properties,
        navigation_properties,
        annotations: parse_annotations(raw)?,
    })
}

fn parse_complex_type(raw: &RawElement) -> io::Result<ComplexType> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let base_type = raw_attr(raw, "BaseType").map(str::to_string);
    let abstract_ = raw_attr_bool(raw, "Abstract");
    let open_type = raw_attr_bool(raw, "OpenType");

    let properties = raw
        .children
        .iter()
        .filter(|n| n.name == "Property")
        .map(parse_property)
        .collect::<io::Result<Vec<_>>>()?;

    let navigation_properties = raw
        .children
        .iter()
        .filter(|n| n.name == "NavigationProperty")
        .map(parse_navigation_property)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(ComplexType {
        name,
        base_type,
        abstract_,
        open_type,
        properties,
        navigation_properties,
        annotations: parse_annotations(raw)?,
    })
}

fn parse_enum_type(raw: &RawElement) -> io::Result<EnumType> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let underlying_type = raw_attr(raw, "UnderlyingType").map(str::to_string);
    let is_flags = raw_attr_bool(raw, "IsFlags");

    let members = raw
        .children
        .iter()
        .filter(|n| n.name == "Member")
        .map(parse_enum_member)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(EnumType {
        name,
        underlying_type,
        is_flags,
        members,
        annotations: parse_annotations(raw)?,
    })
}

fn parse_enum_member(raw: &RawElement) -> io::Result<EnumMember> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let value = raw_attr(raw, "Value").and_then(|v| v.parse::<i64>().ok());
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(EnumMember {
        name,
        value,
        annotations,
    })
}

fn parse_key(raw: &RawElement) -> io::Result<Key> {
    let property_refs = raw
        .children
        .iter()
        .filter(|n| n.name == "PropertyRef")
        .map(parse_property_ref)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(Key { property_refs })
}

fn parse_property_ref(raw: &RawElement) -> io::Result<PropertyRef> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    Ok(PropertyRef { name })
}

fn parse_property(raw: &RawElement) -> io::Result<Property> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let (type_name, is_collection) = raw_type_attr(raw, "Type");
    let nullable = raw_attr_bool(raw, "Nullable");
    let max_length = raw_attr(raw, "MaxLength")
        .map(parse_max_length_facet)
        .transpose()?;
    let precision = raw_attr(raw, "Precision").map(str::to_string);
    let scale = raw_attr(raw, "Scale").map(parse_scale_facet).transpose()?;
    let srid = raw_attr(raw, "SRID").map(parse_srid_facet).transpose()?;
    let unicode = raw_attr_bool(raw, "Unicode");
    let default_value = raw_attr(raw, "DefaultValue").map(str::to_string);
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(Property {
        name,
        type_name,
        is_collection,
        nullable,
        max_length,
        precision,
        scale,
        srid,
        unicode,
        default_value,
        annotations,
    })
}

fn parse_navigation_property(raw: &RawElement) -> io::Result<NavigationProperty> {
    for attribute in ["Nullable", "ContainsTarget"] {
        if raw_attr(raw, attribute).is_some_and(|value| !matches!(value, "true" | "false")) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("NavigationProperty {attribute} must be true or false"),
            ));
        }
    }
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let (type_name, is_collection) = raw_type_attr(raw, "Type");
    let nullable = raw_attr_bool(raw, "Nullable");
    let partner = raw_attr(raw, "Partner").map(str::to_string);
    let contains_target = raw_attr_bool(raw, "ContainsTarget");

    let mut delete_elements = raw.children.iter().filter(|n| n.name == "OnDelete");
    let on_delete = delete_elements
        .next()
        .map(|element| {
            let action = raw_attr(element, "Action").ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "OnDelete requires Action")
            })?;
            parse_on_delete_action(action)
        })
        .transpose()?;
    if delete_elements.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "NavigationProperty permits at most one OnDelete",
        ));
    }

    let referential_constraints = raw
        .children
        .iter()
        .filter(|n| n.name == "ReferentialConstraint")
        .map(parse_referential_constraint)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(NavigationProperty {
        name,
        type_name,
        is_collection,
        nullable,
        partner,
        contains_target,
        on_delete,
        referential_constraints,
        annotations: parse_annotations(raw)?,
    })
}

fn parse_on_delete_action(raw: &str) -> io::Result<OnDeleteAction> {
    OnDeleteAction::parse(raw).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported OnDelete Action value: {raw}"),
        )
    })
}

fn parse_max_length_facet(raw: &str) -> io::Result<MaxLengthFacet> {
    MaxLengthFacet::parse(raw).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported MaxLength value: {raw}"),
        )
    })
}

fn parse_scale_facet(raw: &str) -> io::Result<ScaleFacet> {
    ScaleFacet::parse(raw).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported Scale value: {raw}"),
        )
    })
}

fn parse_srid_facet(raw: &str) -> io::Result<SridFacet> {
    SridFacet::parse(raw).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported SRID value: {raw}"),
        )
    })
}

fn parse_referential_constraint(raw: &RawElement) -> io::Result<ReferentialConstraint> {
    for attribute in ["Property", "ReferencedProperty"] {
        if raw_attr(raw, attribute).is_none_or(str::is_empty) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("ReferentialConstraint requires {attribute}"),
            ));
        }
    }
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;
    Ok(ReferentialConstraint {
        property: raw_attr(raw, "Property").unwrap_or("").to_string(),
        referenced_property: raw_attr(raw, "ReferencedProperty")
            .unwrap_or("")
            .to_string(),
        annotations,
    })
}

fn parse_function(raw: &RawElement) -> io::Result<Function> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let is_bound = raw_attr_bool(raw, "IsBound");
    let is_composable = raw_attr_bool(raw, "IsComposable");
    let entity_set_path = raw_attr(raw, "EntitySetPath").map(str::to_string);

    let parameters = raw
        .children
        .iter()
        .filter(|n| n.name == "Parameter")
        .map(parse_parameter)
        .collect::<io::Result<Vec<_>>>()?;

    let return_type = raw
        .children
        .iter()
        .find(|n| n.name == "ReturnType")
        .map(parse_return_type)
        .transpose()?;

    Ok(Function {
        name,
        is_bound,
        is_composable,
        entity_set_path,
        parameters,
        return_type,
        annotations: parse_annotations(raw)?,
    })
}

fn parse_action(raw: &RawElement) -> io::Result<Action> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let is_bound = raw_attr_bool(raw, "IsBound");
    let entity_set_path = raw_attr(raw, "EntitySetPath").map(str::to_string);
    let parameters = raw
        .children
        .iter()
        .filter(|n| n.name == "Parameter")
        .map(parse_parameter)
        .collect::<io::Result<Vec<_>>>()?;
    let return_type = raw
        .children
        .iter()
        .find(|n| n.name == "ReturnType")
        .map(parse_return_type)
        .transpose()?;
    Ok(Action {
        name,
        is_bound,
        entity_set_path,
        parameters,
        return_type,
        annotations: parse_annotations(raw)?,
    })
}

fn parse_type_definition(raw: &RawElement) -> io::Result<TypeDefinition> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let underlying_type = raw_attr(raw, "UnderlyingType").unwrap_or("").to_string();
    let max_length = raw_attr(raw, "MaxLength")
        .map(parse_max_length_facet)
        .transpose()?;
    let precision = raw_attr(raw, "Precision").map(str::to_string);
    let scale = raw_attr(raw, "Scale").map(parse_scale_facet).transpose()?;
    let srid = raw_attr(raw, "SRID").map(parse_srid_facet).transpose()?;
    let unicode = raw_attr_bool(raw, "Unicode");
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(TypeDefinition {
        name,
        underlying_type,
        max_length,
        precision,
        scale,
        srid,
        unicode,
        annotations,
    })
}

fn parse_term(raw: &RawElement) -> io::Result<Term> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let (type_name, is_collection) = raw_type_attr(raw, "Type");
    let base_term = raw_attr(raw, "BaseTerm").map(str::to_string);
    let default_value = raw_attr(raw, "DefaultValue").map(str::to_string);
    let applies_to = raw_attr(raw, "AppliesTo")
        .map(|s| s.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    let nullable = raw_attr_bool(raw, "Nullable");
    let max_length = raw_attr(raw, "MaxLength")
        .map(parse_max_length_facet)
        .transpose()?;
    let precision = raw_attr(raw, "Precision").map(str::to_string);
    let scale = raw_attr(raw, "Scale").map(parse_scale_facet).transpose()?;
    let srid = raw_attr(raw, "SRID").map(parse_srid_facet).transpose()?;
    let unicode = raw_attr_bool(raw, "Unicode");
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(Term {
        name,
        type_name,
        is_collection,
        base_term,
        default_value,
        applies_to,
        nullable,
        max_length,
        precision,
        scale,
        srid,
        unicode,
        annotations,
    })
}

fn parse_parameter(raw: &RawElement) -> io::Result<Parameter> {
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;
    let (type_name, is_collection) = raw_type_attr(raw, "Type");
    Ok(Parameter {
        name: raw_attr(raw, "Name").unwrap_or("").to_string(),
        type_name,
        is_collection,
        nullable: raw_attr_bool(raw, "Nullable"),
        max_length: raw_attr(raw, "MaxLength")
            .map(parse_max_length_facet)
            .transpose()?,
        precision: raw_attr(raw, "Precision").map(str::to_string),
        scale: raw_attr(raw, "Scale").map(parse_scale_facet).transpose()?,
        srid: raw_attr(raw, "SRID").map(parse_srid_facet).transpose()?,
        unicode: raw_attr_bool(raw, "Unicode"),
        default_value: raw_attr(raw, "DefaultValue").map(str::to_string),
        annotations,
    })
}

fn parse_return_type(raw: &RawElement) -> io::Result<ReturnType> {
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;
    let (type_name, is_collection) = raw_type_attr(raw, "Type");
    Ok(ReturnType {
        type_name,
        is_collection,
        nullable: raw_attr_bool(raw, "Nullable"),
        max_length: raw_attr(raw, "MaxLength")
            .map(parse_max_length_facet)
            .transpose()?,
        precision: raw_attr(raw, "Precision").map(str::to_string),
        scale: raw_attr(raw, "Scale").map(parse_scale_facet).transpose()?,
        srid: raw_attr(raw, "SRID").map(parse_srid_facet).transpose()?,
        unicode: raw_attr_bool(raw, "Unicode"),
        annotations,
    })
}

fn parse_entity_container(raw: &RawElement) -> io::Result<EntityContainer> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let extends = raw_attr(raw, "Extends").map(str::to_string);

    let entity_sets = raw
        .children
        .iter()
        .filter(|n| n.name == "EntitySet")
        .map(parse_entity_set)
        .collect::<io::Result<Vec<_>>>()?;

    let singletons = raw
        .children
        .iter()
        .filter(|n| n.name == "Singleton")
        .map(parse_singleton)
        .collect::<io::Result<Vec<_>>>()?;

    let function_imports = raw
        .children
        .iter()
        .filter(|n| n.name == "FunctionImport")
        .map(parse_function_import)
        .collect::<io::Result<Vec<_>>>()?;

    let action_imports = raw
        .children
        .iter()
        .filter(|n| n.name == "ActionImport")
        .map(parse_action_import)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(EntityContainer {
        name,
        extends,
        entity_sets,
        singletons,
        function_imports,
        action_imports,
        annotations: parse_annotations(raw)?,
    })
}

fn parse_action_import(raw: &RawElement) -> io::Result<ActionImport> {
    Ok(ActionImport {
        name: raw_attr(raw, "Name").unwrap_or("").to_string(),
        action: raw_attr(raw, "Action").map(str::to_string),
        entity_set: raw_attr(raw, "EntitySet").map(str::to_string),
        include_in_service_document: raw_attr_bool(raw, "IncludeInServiceDocument"),
        annotations: parse_annotations(raw)?,
    })
}

fn parse_entity_set(raw: &RawElement) -> io::Result<EntitySet> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let entity_type = raw_attr(raw, "EntityType").map(str::to_string);
    let navigation_property_bindings = raw
        .children
        .iter()
        .filter(|n| n.name == "NavigationPropertyBinding")
        .map(parse_navigation_property_binding)
        .collect::<io::Result<Vec<_>>>()?;
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(EntitySet {
        name,
        entity_type,
        include_in_service_document: raw_attr_bool(raw, "IncludeInServiceDocument"),
        navigation_property_bindings,
        annotations,
    })
}

fn parse_singleton(raw: &RawElement) -> io::Result<Singleton> {
    let name = raw_attr(raw, "Name").unwrap_or("").to_string();
    let type_name = raw_attr(raw, "Type").map(str::to_string);
    let navigation_property_bindings = raw
        .children
        .iter()
        .filter(|n| n.name == "NavigationPropertyBinding")
        .map(parse_navigation_property_binding)
        .collect::<io::Result<Vec<_>>>()?;
    let annotations = raw
        .children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(Singleton {
        name,
        type_name,
        include_in_service_document: raw_attr_bool(raw, "IncludeInServiceDocument"),
        navigation_property_bindings,
        annotations,
    })
}

fn parse_function_import(raw: &RawElement) -> io::Result<FunctionImport> {
    Ok(FunctionImport {
        name: raw_attr(raw, "Name").unwrap_or("").to_string(),
        entity_set: raw_attr(raw, "EntitySet").map(str::to_string),
        function: raw_attr(raw, "Function").map(str::to_string),
        include_in_service_document: raw_attr_bool(raw, "IncludeInServiceDocument"),
        annotations: parse_annotations(raw)?,
    })
}

fn parse_navigation_property_binding(raw: &RawElement) -> io::Result<NavigationPropertyBinding> {
    Ok(NavigationPropertyBinding {
        path: raw_attr(raw, "Path").unwrap_or("").to_string(),
        target: raw_attr(raw, "Target").unwrap_or("").to_string(),
    })
}

fn parse_annotation(raw: &RawElement) -> io::Result<Annotation> {
    let term = raw_attr(raw, "Term").unwrap_or("").to_string();
    let qualifier = raw_attr(raw, "Qualifier").map(str::to_string);
    // Sibling-target form: XML `<Annotation Target="Foo" .../>`, JSON's
    // `Foo@Term[#Qualifier]` is rewritten by csdl_json_reader to attach the
    // same Target attribute on the SyntaxUnit.
    let target = raw_attr(raw, "Target").map(str::to_string);

    let expression = if let Some(expr) = raw.annotation_expressions.first() {
        Some(expr.clone())
    } else {
        None
    };

    Ok(Annotation {
        term,
        qualifier,
        target,
        expression,
    })
}

/// Gather every direct `<Annotation>` child of `raw` into a `Vec<Annotation>`.
/// Used by every parse_* function that exposes a model field for own- and
/// sibling-target annotations on the host element.
fn parse_annotations(raw: &RawElement) -> io::Result<Vec<Annotation>> {
    raw.children
        .iter()
        .filter(|n| n.name == "Annotation")
        .map(parse_annotation)
        .collect()
}

fn raw_attr<'a>(raw: &'a RawElement, key: &str) -> Option<&'a str> {
    raw.attributes
        .iter()
        .find_map(|(k, v)| (k == key).then_some(v.as_str()))
}

fn raw_attr_bool(raw: &RawElement, key: &str) -> Option<bool> {
    raw_attr(raw, key).map(|v| v.eq_ignore_ascii_case("true"))
}

/// Read a Type-like attribute and split `Collection(X)` wrapping. Returns
/// `(unwrapped_type, is_collection)`. If the attribute is missing the type is
/// `None`; if a separate `Collection="true"` attribute is present that also
/// flips the collection flag (the JSON reader emits the two pieces
/// separately; the XML reader passes them combined as `Collection(X)`).
fn raw_type_attr(raw: &RawElement, key: &str) -> (Option<String>, bool) {
    let raw_value = raw_attr(raw, key).map(str::to_string);
    let mut is_collection = raw_attr_bool(raw, "Collection").unwrap_or(false);
    let type_name = match raw_value {
        None => None,
        Some(v) => match v
            .strip_prefix("Collection(")
            .and_then(|s| s.strip_suffix(')'))
        {
            Some(inner) => {
                is_collection = true;
                Some(inner.to_string())
            }
            None => Some(v),
        },
    };
    (type_name, is_collection)
}
