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

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub term: String,
    pub qualifier: Option<String>,
    pub target: Option<String>,
    pub expression: Option<CsdlAnnotationExpression>,
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
