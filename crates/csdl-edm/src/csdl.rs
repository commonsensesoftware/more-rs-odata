use std::io;
use std::path::Path;

use crate::expr::CsdlAnnotationExpression;

#[derive(Debug, Clone, PartialEq)]
pub struct CsdlDocument {
    pub edmx: Option<Edmx>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edmx {
    /// XML Base on the document wrapper.
    pub base_uri: Option<String>,
    /// Namespace-qualified service container, explicit in JSON and inferred
    /// only when XML declares exactly one container.
    pub entity_container: Option<String>,
    pub version: Option<String>,
    pub references: Vec<Reference>,
    pub schemas: Vec<Schema>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub annotations: Vec<Annotation>,
    /// XML Base on this reference, resolved after the document's base.
    pub base_uri: Option<String>,
    pub uri: String,
    pub includes: Vec<Include>,
    pub include_annotations: Vec<IncludeAnnotations>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncludeAnnotations {
    pub term_namespace: String,
    pub qualifier: Option<String>,
    pub target_namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Include {
    pub namespace: String,
    pub alias: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub external_annotations: Vec<Annotations>,
    pub namespace: String,
    pub alias: Option<String>,
    pub elements: Vec<SchemaElement>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotations {
    pub target: String,
    /// Group qualifiers are normalized onto each annotation.
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaElement {
    EntityType(EntityType),
    ComplexType(ComplexType),
    EnumType(EnumType),
    TypeDefinition(TypeDefinition),
    Term(Term),
    Function(Function),
    Action(Action),
    EntityContainer(EntityContainer),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDefinition {
    pub name: String,
    pub underlying_type: String,
    pub max_length: Option<MaxLengthFacet>,
    pub precision: Option<String>,
    pub scale: Option<ScaleFacet>,
    pub srid: Option<SridFacet>,
    pub unicode: Option<bool>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub name: String,
    pub type_name: Option<String>,
    pub is_collection: bool,
    pub base_term: Option<String>,
    pub default_value: Option<String>,
    pub applies_to: Vec<String>,
    pub nullable: Option<bool>,
    pub max_length: Option<MaxLengthFacet>,
    pub precision: Option<String>,
    pub scale: Option<ScaleFacet>,
    pub srid: Option<SridFacet>,
    pub unicode: Option<bool>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub name: String,
    pub is_bound: Option<bool>,
    pub entity_set_path: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<ReturnType>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumType {
    pub name: String,
    pub underlying_type: Option<String>,
    pub is_flags: Option<bool>,
    pub members: Vec<EnumMember>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumMember {
    pub name: String,
    pub value: Option<i64>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntityType {
    pub name: String,
    pub base_type: Option<String>,
    pub abstract_: Option<bool>,
    pub open_type: Option<bool>,
    pub has_stream: Option<bool>,
    pub key: Option<Key>,
    pub properties: Vec<Property>,
    pub navigation_properties: Vec<NavigationProperty>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComplexType {
    pub name: String,
    pub base_type: Option<String>,
    pub abstract_: Option<bool>,
    pub open_type: Option<bool>,
    pub properties: Vec<Property>,
    pub navigation_properties: Vec<NavigationProperty>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub property_refs: Vec<PropertyRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyRef {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub type_name: Option<String>,
    pub is_collection: bool,
    pub nullable: Option<bool>,
    pub max_length: Option<MaxLengthFacet>,
    pub precision: Option<String>,
    pub scale: Option<ScaleFacet>,
    pub srid: Option<SridFacet>,
    pub unicode: Option<bool>,
    pub default_value: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NavigationProperty {
    pub name: String,
    pub type_name: Option<String>,
    pub is_collection: bool,
    pub nullable: Option<bool>,
    pub partner: Option<String>,
    pub contains_target: Option<bool>,
    pub on_delete: Option<OnDeleteAction>,
    pub referential_constraints: Vec<ReferentialConstraint>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDeleteAction {
    Cascade,
    None,
    SetNull,
    SetDefault,
}

impl OnDeleteAction {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "Cascade" => Some(Self::Cascade),
            "None" => Some(Self::None),
            "SetNull" => Some(Self::SetNull),
            "SetDefault" => Some(Self::SetDefault),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cascade => "Cascade",
            Self::None => "None",
            Self::SetNull => "SetNull",
            Self::SetDefault => "SetDefault",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReferentialConstraint {
    pub property: String,
    pub referenced_property: String,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub is_bound: Option<bool>,
    pub is_composable: Option<bool>,
    pub entity_set_path: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<ReturnType>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// Representation default when Nullable was omitted (JSON: false).
    pub nullable_default: Option<bool>,
    pub name: String,
    pub type_name: Option<String>,
    pub is_collection: bool,
    pub nullable: Option<bool>,
    pub max_length: Option<MaxLengthFacet>,
    pub precision: Option<String>,
    pub scale: Option<ScaleFacet>,
    pub srid: Option<SridFacet>,
    pub unicode: Option<bool>,
    pub default_value: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnType {
    /// Representation default when Nullable was omitted (JSON: false).
    pub nullable_default: Option<bool>,
    pub type_name: Option<String>,
    pub is_collection: bool,
    pub nullable: Option<bool>,
    pub max_length: Option<MaxLengthFacet>,
    pub precision: Option<String>,
    pub scale: Option<ScaleFacet>,
    pub srid: Option<SridFacet>,
    pub unicode: Option<bool>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxLengthFacet {
    Number(u64),
    Max,
}

impl MaxLengthFacet {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw == "max" {
            return Some(Self::Max);
        }
        raw.parse::<u64>().ok().map(Self::Number)
    }

    pub fn as_str(self) -> String {
        match self {
            Self::Number(value) => value.to_string(),
            Self::Max => "max".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleFacet {
    Floating,
    Number(u64),
    Variable,
}

impl ScaleFacet {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.eq_ignore_ascii_case("floating") {
            return Some(Self::Floating);
        }
        if raw.eq_ignore_ascii_case("variable") {
            return Some(Self::Variable);
        }
        raw.parse::<u64>().ok().map(Self::Number)
    }

    pub fn as_str(self) -> String {
        match self {
            Self::Number(value) => value.to_string(),
            Self::Variable => "variable".to_string(),
            Self::Floating => "floating".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SridFacet {
    Number(u64),
    Variable,
}

impl SridFacet {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw == "variable" {
            return Some(Self::Variable);
        }
        raw.parse::<u64>().ok().map(Self::Number)
    }

    pub fn as_str(self) -> String {
        match self {
            Self::Number(value) => value.to_string(),
            Self::Variable => "variable".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntityContainer {
    pub name: String,
    pub extends: Option<String>,
    pub entity_sets: Vec<EntitySet>,
    pub singletons: Vec<Singleton>,
    pub function_imports: Vec<FunctionImport>,
    pub action_imports: Vec<ActionImport>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionImport {
    pub name: String,
    pub action: Option<String>,
    pub entity_set: Option<String>,
    pub include_in_service_document: Option<bool>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntitySet {
    pub name: String,
    pub entity_type: Option<String>,
    pub include_in_service_document: Option<bool>,
    pub navigation_property_bindings: Vec<NavigationPropertyBinding>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Singleton {
    pub name: String,
    pub type_name: Option<String>,
    pub nullable: Option<bool>,
    pub include_in_service_document: Option<bool>,
    pub navigation_property_bindings: Vec<NavigationPropertyBinding>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionImport {
    pub name: String,
    pub entity_set: Option<String>,
    pub function: Option<String>,
    pub include_in_service_document: Option<bool>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NavigationPropertyBinding {
    pub path: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct Annotation {
    /// Literal spelling rules of the source; None for programmatic expressions.
    /// Equality compares expression data, excluding representation provenance.
    pub source_format: Option<crate::CsdlFormat>,
    pub annotations: Vec<Annotation>,
    pub term: String,
    pub qualifier: Option<String>,
    pub target: Option<String>,
    pub expression: Option<CsdlAnnotationExpression>,
}

impl PartialEq for Annotation {
    fn eq(&self, other: &Self) -> bool {
        self.annotations == other.annotations
            && self.term == other.term
            && self.qualifier == other.qualifier
            && self.target == other.target
            && self.expression == other.expression
    }
}

impl CsdlDocument {
    pub fn from_path<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path_ref = path.as_ref();
        let ext = path_ref
            .extension()
            .and_then(|v| v.to_str())
            .map(|v| v.to_ascii_lowercase());

        match ext.as_deref() {
            Some("xml") => {
                let content = std::fs::read_to_string(path_ref)?;
                crate::parser::from_xml_reader(content.as_bytes())
            }
            Some("json") => {
                let content = std::fs::read_to_string(path_ref)?;
                crate::parser::from_json_reader(content.as_bytes())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported CSDL file extension for {}", path_ref.display()),
            )),
        }
    }
}

/// Normalize JSON annotations-on-annotations into the same tree as XML.
pub(crate) fn nest_annotations(values: &mut Vec<Annotation>) {
    loop {
        let pair = values
            .iter()
            .enumerate()
            .filter_map(|(child, a)| {
                let target = a.target.as_ref()?;
                values.iter().enumerate().find_map(|(parent, p)| {
                    if child == parent {
                        return None;
                    }
                    let key = format!(
                        "{}@{}{}",
                        p.target.as_deref().unwrap_or(""),
                        p.term,
                        p.qualifier
                            .as_ref()
                            .map(|q| format!("#{q}"))
                            .unwrap_or_default()
                    );
                    (key == *target).then_some((child, parent))
                })
            })
            .max_by_key(|(child, _)| {
                values[*child]
                    .target
                    .as_ref()
                    .map_or(0, |s| s.matches('@').count())
            });
        let Some((child, parent)) = pair else {
            break;
        };
        let mut annotation = values.remove(child);
        annotation.target = None;
        values[if parent > child { parent - 1 } else { parent }]
            .annotations
            .push(annotation);
    }
    for a in values {
        nest_annotations(&mut a.annotations);
    }
}

pub(crate) fn annotation_groups(schema: &Schema) -> Vec<Annotations> {
    fn add(out: &mut Vec<Annotations>, path: &str, annotations: &[Annotation]) {
        for annotation in annotations {
            let mut annotation = annotation.clone();
            let target = annotation
                .target
                .take()
                .map(|p| format!("{path}/{p}"))
                .unwrap_or_else(|| path.into());
            out.push(Annotations {
                target,
                annotations: vec![annotation],
            });
        }
    }
    fn members(
        out: &mut Vec<Annotations>,
        path: &str,
        properties: &[Property],
        navs: &[NavigationProperty],
    ) {
        for p in properties {
            add(out, &format!("{path}/{}", p.name), &p.annotations);
        }
        for n in navs {
            let path = format!("{path}/{}", n.name);
            add(out, &path, &n.annotations);
            for c in &n.referential_constraints {
                add(out, &format!("{path}/{}", c.property), &c.annotations);
            }
        }
    }
    fn callable(
        out: &mut Vec<Annotations>,
        path: &str,
        params: &[Parameter],
        ret: &Option<ReturnType>,
        annotations: &[Annotation],
    ) {
        let signature = params
            .iter()
            .map(|p| {
                let ty = p.type_name.as_deref().unwrap_or("Edm.String");
                if p.is_collection {
                    format!("Collection({ty})")
                } else {
                    ty.into()
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let path = format!("{path}({signature})");
        add(out, &path, annotations);
        for p in params {
            add(out, &format!("{path}/{}", p.name), &p.annotations);
        }
        if let Some(r) = ret {
            add(out, &format!("{path}/$ReturnType"), &r.annotations);
        }
    }
    let mut out = schema.external_annotations.clone();
    add(&mut out, &schema.namespace, &schema.annotations);
    for e in &schema.elements {
        match e {
            SchemaElement::EntityType(t) => {
                let path = format!("{}.{}", schema.namespace, t.name);
                add(&mut out, &path, &t.annotations);
                members(&mut out, &path, &t.properties, &t.navigation_properties);
            }
            SchemaElement::ComplexType(t) => {
                let path = format!("{}.{}", schema.namespace, t.name);
                add(&mut out, &path, &t.annotations);
                members(&mut out, &path, &t.properties, &t.navigation_properties);
            }
            SchemaElement::EnumType(t) => {
                let path = format!("{}.{}", schema.namespace, t.name);
                add(&mut out, &path, &t.annotations);
                for m in &t.members {
                    add(&mut out, &format!("{path}/{}", m.name), &m.annotations);
                }
            }
            SchemaElement::TypeDefinition(t) => add(
                &mut out,
                &format!("{}.{}", schema.namespace, t.name),
                &t.annotations,
            ),
            SchemaElement::Term(t) => add(
                &mut out,
                &format!("{}.{}", schema.namespace, t.name),
                &t.annotations,
            ),
            SchemaElement::Function(f) => callable(
                &mut out,
                &format!("{}.{}", schema.namespace, f.name),
                &f.parameters,
                &f.return_type,
                &f.annotations,
            ),
            SchemaElement::Action(f) => callable(
                &mut out,
                &format!("{}.{}", schema.namespace, f.name),
                &f.parameters,
                &f.return_type,
                &f.annotations,
            ),
            SchemaElement::EntityContainer(c) => {
                let path = format!("{}.{}", schema.namespace, c.name);
                add(&mut out, &path, &c.annotations);
                for s in &c.entity_sets {
                    add(&mut out, &format!("{path}/{}", s.name), &s.annotations);
                }
                for s in &c.singletons {
                    add(&mut out, &format!("{path}/{}", s.name), &s.annotations);
                }
                for s in &c.function_imports {
                    add(&mut out, &format!("{path}/{}", s.name), &s.annotations);
                }
                for s in &c.action_imports {
                    add(&mut out, &format!("{path}/{}", s.name), &s.annotations);
                }
            }
        }
    }
    out
}
