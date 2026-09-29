//! ---------------------------------------------------------------------------
//! 2. Semantic model
//! ---------------------------------------------------------------------------

use std::sync::{Arc, OnceLock, Weak};

/// Authored facets. Type-definition facets are retained separately to detect
/// forbidden re-specification before calculating effective constraints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeFacets {
    pub max_length: Option<crate::csdl::MaxLengthFacet>,
    pub precision: Option<String>,
    pub scale: Option<crate::csdl::ScaleFacet>,
    pub srid: Option<crate::csdl::SridFacet>,
    pub unicode: Option<bool>,
}

/// Open set: future applicability values survive resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnnotationTargetKind {
    Action,
    ActionImport,
    Annotation,
    Apply,
    Cast,
    Collection,
    ComplexType,
    EntityContainer,
    EntitySet,
    EntityType,
    EnumType,
    Function,
    FunctionImport,
    If,
    Include,
    IsOf,
    LabeledElement,
    Member,
    NavigationProperty,
    Null,
    OnDelete,
    Parameter,
    Property,
    PropertyValue,
    Record,
    Reference,
    ReferentialConstraint,
    ReturnType,
    Schema,
    Singleton,
    Term,
    TypeDefinition,
    UrlRef,
    Other(String),
}

impl AnnotationTargetKind {
    pub fn parse(value: &str) -> Self {
        match value {
            "Action" => Self::Action,
            "ActionImport" => Self::ActionImport,
            "Annotation" => Self::Annotation,
            "Apply" => Self::Apply,
            "Cast" => Self::Cast,
            "Collection" => Self::Collection,
            "ComplexType" => Self::ComplexType,
            "EntityContainer" => Self::EntityContainer,
            "EntitySet" => Self::EntitySet,
            "EntityType" => Self::EntityType,
            "EnumType" => Self::EnumType,
            "Function" => Self::Function,
            "FunctionImport" => Self::FunctionImport,
            "If" => Self::If,
            "Include" => Self::Include,
            "IsOf" => Self::IsOf,
            "LabeledElement" => Self::LabeledElement,
            "Member" => Self::Member,
            "NavigationProperty" => Self::NavigationProperty,
            "Null" => Self::Null,
            "OnDelete" => Self::OnDelete,
            "Parameter" => Self::Parameter,
            "Property" => Self::Property,
            "PropertyValue" => Self::PropertyValue,
            "Record" => Self::Record,
            "Reference" => Self::Reference,
            "ReferentialConstraint" => Self::ReferentialConstraint,
            "ReturnType" => Self::ReturnType,
            "Schema" => Self::Schema,
            "Singleton" => Self::Singleton,
            "Term" => Self::Term,
            "TypeDefinition" => Self::TypeDefinition,
            "UrlRef" => Self::UrlRef,
            other => Self::Other(other.to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    /// Abstract root of all primitive, enumeration, and defined scalar types.
    PrimitiveType,
    Binary,
    Stream,
    Geography,
    GeographyPoint,
    GeographyLineString,
    GeographyPolygon,
    GeographyMultiPoint,
    GeographyMultiLineString,
    GeographyMultiPolygon,
    GeographyCollection,
    Geometry,
    GeometryPoint,
    GeometryLineString,
    GeometryPolygon,
    GeometryMultiPoint,
    GeometryMultiLineString,
    GeometryMultiPolygon,
    GeometryCollection,
    Byte,
    Date,
    DateTimeOffset,
    Decimal,
    Double,
    Duration,
    Guid,
    Int16,
    Int64,
    SByte,
    Single,
    String,
    TimeOfDay,
    Int32,
    Boolean,
}

#[derive(Debug, Clone)]
pub enum ResolvedType {
    Builtin(BuiltinType),
    Primitive(PrimitiveType),
    Enum(Arc<EnumType>),
    Complex(Arc<ComplexType>),
    TypeDefinition(Arc<TypeDefinition>),
}

#[derive(Debug, Clone)]
pub enum TermType {
    Builtin(BuiltinType),
    Primitive(PrimitiveType),
    TypeDefinition(Arc<TypeDefinition>),
    Enum(Arc<EnumType>),
    Complex(Arc<ComplexType>),
    Entity(Arc<EntityType>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinType {
    Untyped,
    AnnotationPath,
    PropertyPath,
    NavigationPropertyPath,
    AnyPropertyPath,
    ModelElementPath,
}

impl BuiltinType {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "Edm.Untyped" => Self::Untyped,
            "Edm.AnnotationPath" => Self::AnnotationPath,
            "Edm.PropertyPath" => Self::PropertyPath,
            "Edm.NavigationPropertyPath" => Self::NavigationPropertyPath,
            "Edm.AnyPropertyPath" => Self::AnyPropertyPath,
            "Edm.ModelElementPath" => Self::ModelElementPath,
            _ => return None,
        })
    }
}

/// Canonical built-in abstract entity node. It is never a declared schema child.
pub fn abstract_entity_type() -> &'static Arc<EntityType> {
    static TYPE: OnceLock<Arc<EntityType>> = OnceLock::new();
    TYPE.get_or_init(|| {
        Arc::new(EntityType {
            name: "Edm.EntityType".into(),
            is_abstract: true,
            is_open: false,
            has_stream: false,
            base_type: OnceLock::from(None),
            keys: OnceLock::from(Vec::new()),
            key_aliases: OnceLock::from(Vec::new()),
            properties: OnceLock::from(Vec::new()),
            navigation_properties: OnceLock::from(Vec::new()),
        })
    })
}

/// Canonical built-in abstract complex node.
pub fn abstract_complex_type() -> &'static Arc<ComplexType> {
    static TYPE: OnceLock<Arc<ComplexType>> = OnceLock::new();
    TYPE.get_or_init(|| {
        Arc::new(ComplexType {
            name: "Edm.ComplexType".into(),
            is_abstract: true,
            is_open: false,
            base_type: OnceLock::from(None),
            properties: OnceLock::from(Vec::new()),
            navigation_properties: OnceLock::from(Vec::new()),
        })
    })
}

#[derive(Debug)]
pub struct DocumentModel {
    pub version: String,
    pub references: Vec<Reference>,
    pub schemas: Vec<Arc<Model>>,
}

#[derive(Debug)]
pub struct Reference {
    pub uri: String,
    pub includes: Vec<Include>,
    pub include_annotations: Vec<IncludeAnnotations>,
}

#[derive(Debug)]
pub struct Include {
    pub namespace: String,
    pub alias: Option<String>,
}

#[derive(Debug)]
pub struct IncludeAnnotations {
    pub term_namespace: String,
    pub target_namespace: Option<String>,
    pub qualifier: Option<String>,
}

#[derive(Debug)]
pub struct Model {
    /// Inline, sibling-target, and externally targeted annotations.
    pub annotations: OnceLock<Vec<ResolvedAnnotation>>,
    pub namespace: String,
    pub alias: Option<String>,
    pub elements: Vec<Arc<SchemaElement>>,
    pub entity_container: Option<Arc<EntityContainer>>,
    /// All declared containers; `entity_container` remains the first for compatibility.
    pub entity_containers: Vec<Arc<EntityContainer>>,
}

#[derive(Debug, Clone)]
pub enum AnnotationTarget {
    Expression {
        kind: AnnotationTargetKind,
        ordinal: usize,
    },
    Schema(String),
    Element(Arc<SchemaElement>),
    Property(Arc<Property>),
    NavigationProperty(Arc<NavigationProperty>),
    EnumMember(Arc<EnumMember>),
    Container(Arc<EntityContainer>),
    Resource(Arc<EntityContainerElement>),
    Parameter {
        operation: Arc<SchemaElement>,
        index: usize,
    },
    ReturnType(Arc<SchemaElement>),
    ReferentialConstraint {
        navigation: Arc<NavigationProperty>,
        index: usize,
    },
    OnDelete(Arc<NavigationProperty>),
    AppliedTerm {
        term: Arc<Term>,
        qualifier: Option<String>,
    },
    DynamicProperty(String),
    Unresolved(String),
}

#[derive(Debug)]
pub struct ResolvedAnnotation {
    pub term_name: String,
    /// Unavailable external terms remain unresolved; resolution does not fetch documents.
    pub term: Option<Arc<Term>>,
    pub qualifier: Option<String>,
    pub target_path: String,
    /// An operation-name target can select multiple overloads.
    pub targets: Vec<Arc<[AnnotationTarget]>>,
    /// Authored expression retained for consumers; paths are resolved separately.
    pub expression: Option<crate::expr::CsdlAnnotationExpression>,
    pub expression_paths: Vec<AnnotationExpressionPath>,
}

#[derive(Debug)]
pub struct AnnotationExpressionPath {
    pub kind: AnnotationPathKind,
    pub segments: Arc<[AnnotationTarget]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationPathKind {
    Value,
    Property,
    NavigationProperty,
    Annotation,
}

impl Model {
    pub fn containers(&self) -> &[Arc<EntityContainer>] {
        if self.entity_containers.is_empty() {
            self.entity_container.as_slice()
        } else {
            &self.entity_containers
        }
    }
}

#[derive(Debug)]
pub enum SchemaElement {
    EntityType(Arc<EntityType>),
    ComplexType(Arc<ComplexType>),
    EnumType(Arc<EnumType>),
    TypeDefinition(Arc<TypeDefinition>),
    Term(Arc<Term>),
    Function(Arc<Function>),
    Action(Arc<Action>),
}

/// Entity type. `properties` and `navigation_properties` are wrapped in
/// `OnceLock` so the resolver can fill them in after every entity/complex
/// `Arc` exists. After resolution they are effectively immutable.
#[derive(Debug)]
pub struct EntityType {
    pub name: String,
    pub is_abstract: bool,
    pub is_open: bool,
    pub has_stream: bool,
    pub base_type: OnceLock<Option<Weak<EntityType>>>,
    /// Effective key paths (own + inherited). Each key is a resolved path of
    /// [`KeyPathSegment`]s rather than a raw string, filled by the resolver in a
    /// late pass once every entity/complex type has its properties.
    pub keys: OnceLock<Vec<Arc<[KeyPathSegment]>>>,
    /// Effective aliases, in the same order as `keys` (including inherited keys).
    pub key_aliases: OnceLock<Vec<Option<String>>>,
    pub properties: OnceLock<Vec<Arc<Property>>>,
    pub navigation_properties: OnceLock<Vec<Arc<NavigationProperty>>>,
}

impl EntityType {
    pub fn keys(&self) -> &[Arc<[KeyPathSegment]>] {
        self.keys.get().map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn properties(&self) -> &[Arc<Property>] {
        self.properties.get().map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn navigation_properties(&self) -> &[Arc<NavigationProperty>] {
        self.navigation_properties
            .get()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[derive(Debug)]
pub struct ComplexType {
    pub name: String,
    pub is_abstract: bool,
    pub is_open: bool,
    pub base_type: OnceLock<Option<Weak<ComplexType>>>,
    pub properties: OnceLock<Vec<Arc<Property>>>,
    pub navigation_properties: OnceLock<Vec<Arc<NavigationProperty>>>,
}

impl ComplexType {
    pub fn properties(&self) -> &[Arc<Property>] {
        self.properties.get().map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn navigation_properties(&self) -> &[Arc<NavigationProperty>] {
        self.navigation_properties
            .get()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[derive(Debug)]
pub struct EnumType {
    pub name: String,
    pub underlying_type: PrimitiveType,
    pub is_flags: bool,
    pub members: Vec<Arc<EnumMember>>,
}

#[derive(Debug)]
pub struct EnumMember {
    pub name: String,
    pub value: Option<i64>,
    /// Numeric value after applying XML's ordered implicit-value rule.
    pub resolved_value: i64,
}

#[derive(Debug)]
pub struct TypeDefinition {
    pub facets: TypeFacets,
    pub name: String,
    pub underlying_type: PrimitiveType,
}

#[derive(Debug)]
pub struct Term {
    pub nullable: Option<bool>,
    pub default_value: Option<String>,
    pub applies_to: Vec<AnnotationTargetKind>,
    pub facets: TypeFacets,
    pub name: String,
    pub is_collection: bool,
    pub ty: OnceLock<TermType>,
    pub base_term: OnceLock<Option<Weak<Term>>>,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub is_bound: bool,
    pub is_composable: bool,
    /// Resolved against the binding parameter's type; the head names the binding
    /// parameter (see [`EntitySetPathSegment`]).
    pub entity_set_path: Option<Arc<[EntitySetPathSegment]>>,
    pub parameters: Vec<OperationParameter>,
    pub return_type: Option<OperationReturnType>,
}

#[derive(Debug)]
pub struct Action {
    pub name: String,
    pub is_bound: bool,
    /// See [`Function::entity_set_path`].
    pub entity_set_path: Option<Arc<[EntitySetPathSegment]>>,
    pub parameters: Vec<OperationParameter>,
    pub return_type: Option<OperationReturnType>,
}

#[derive(Debug)]
pub struct OperationParameter {
    pub default_value: Option<String>,
    pub facets: TypeFacets,
    pub name: String,
    pub ty: TermType,
    pub is_collection: bool,
    pub nullable: Option<bool>,
}

#[derive(Debug)]
pub struct OperationReturnType {
    pub facets: TypeFacets,
    pub ty: TermType,
    pub is_collection: bool,
    pub nullable: Option<bool>,
}

impl Term {
    pub fn ty(&self) -> Option<&TermType> {
        self.ty.get()
    }
}

#[derive(Debug)]
pub struct Property {
    pub facets: TypeFacets,
    pub name: String,
    pub ty: ResolvedType,
    pub is_collection: bool,
    pub nullable: Option<bool>,
    pub default_value: Option<String>,
}

#[derive(Debug)]
pub struct NavigationProperty {
    pub name: String,
    /// Weak to break entity <-> entity cycles.
    pub target: Weak<EntityType>,
    pub is_collection: bool,
    pub nullable: Option<bool>,
    /// Resolved `Partner` path against the target entity type; ends at a
    /// navigation property. Filled by the resolver in a late pass once every
    /// entity's navigation properties exist. Unset when no partner is declared.
    pub partner: OnceLock<Arc<[BindingPathSegment]>>,
    pub contains_target: Option<bool>,
    pub on_delete: Option<OnDeleteAction>,
    pub referential_constraints: Vec<ReferentialConstraint>,
}

impl NavigationProperty {
    pub fn partner(&self) -> Option<&[BindingPathSegment]> {
        self.partner.get().map(Arc::as_ref)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDeleteAction {
    Cascade,
    None,
    SetNull,
    SetDefault,
}

impl OnDeleteAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cascade => "Cascade",
            Self::None => "None",
            Self::SetNull => "SetNull",
            Self::SetDefault => "SetDefault",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReferentialConstraint {
    pub property: String,
    pub referenced_property: String,
    /// Resolver-owned paths; authored strings above remain for diagnostics.
    pub property_path: OnceLock<Arc<[ConstraintPathSegment]>>,
    pub referenced_property_path: OnceLock<Arc<[ConstraintPathSegment]>>,
}

#[derive(Debug, Clone)]
pub enum ConstraintPathSegment {
    Property(Weak<Property>),
    NavigationProperty(Weak<NavigationProperty>),
    Unresolved(String),
}

#[derive(Debug)]
pub struct EntityContainer {
    pub name: String,
    pub extends: Option<Arc<EntityContainer>>,
    pub elements: Vec<Arc<EntityContainerElement>>,
}

#[derive(Debug)]
pub enum EntityContainerElement {
    EntitySet(Arc<EntitySet>),
    Singleton(Arc<Singleton>),
    FunctionImport(Arc<FunctionImport>),
    ActionImport(Arc<ActionImport>),
}

#[derive(Debug)]
pub struct EntitySet {
    pub name: String,
    pub target: Arc<EntityType>,
    pub include_in_service_document: bool,
    /// Filled by the resolver after every container element exists, so that
    /// binding targets can reference sibling sets/singletons.
    pub navigation_property_bindings: OnceLock<Vec<NavigationPropertyBinding>>,
}

impl EntitySet {
    pub fn navigation_property_bindings(&self) -> &[NavigationPropertyBinding] {
        self.navigation_property_bindings
            .get()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[derive(Debug)]
pub struct Singleton {
    pub name: String,
    pub target: Arc<EntityType>,
    pub nullable: Option<bool>,
    /// See [`EntitySet::navigation_property_bindings`].
    pub navigation_property_bindings: OnceLock<Vec<NavigationPropertyBinding>>,
}

impl Singleton {
    pub fn navigation_property_bindings(&self) -> &[NavigationPropertyBinding] {
        self.navigation_property_bindings
            .get()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

impl NavigationPropertyBinding {
    pub fn path_string(&self) -> String {
        binding_path_segments_to_string(&self.path)
    }

    pub fn target_string(&self) -> String {
        binding_path_segments_to_string(&self.target)
    }
}

#[derive(Debug)]
pub struct FunctionImport {
    pub name: String,
    pub function: String,
    pub overloads: Vec<Arc<Function>>,
    pub include_in_service_document: bool,
    /// Resolved against the container; ends at an `EntitySet`/`Singleton`. Shares
    /// the binding-target representation (see [`BindingPathSegment`]).
    pub entity_set: Option<Arc<[BindingPathSegment]>>,
}

impl FunctionImport {
    pub fn entity_set_string(&self) -> Option<String> {
        self.entity_set
            .as_deref()
            .map(binding_path_segments_to_string)
    }
}

#[derive(Debug)]
pub struct ActionImport {
    pub name: String,
    pub action: String,
    pub operation: Arc<Action>,
    /// See [`FunctionImport::entity_set`].
    pub entity_set: Option<Arc<[BindingPathSegment]>>,
}

impl ActionImport {
    pub fn entity_set_string(&self) -> Option<String> {
        self.entity_set
            .as_deref()
            .map(binding_path_segments_to_string)
    }
}

/// A resolved navigation property binding: both the source `path` (to the bound
/// navigation property) and the `target` (the entity set / singleton it is
/// bound to) are modeled as resolved reference paths over the EDM graph rather
/// than the raw strings of the syntactic CSDL node.
#[derive(Debug, Clone)]
pub struct NavigationPropertyBinding {
    /// Resolves against the bound entity type; ends at a `NavigationProperty`.
    pub path: Arc<[BindingPathSegment]>,
    /// Resolves against the container(s); ends at an `EntitySet`/`Singleton`.
    pub target: Arc<[BindingPathSegment]>,
}

/// One segment of a resolved binding `path` or `target`.
///
/// Each variant references a named element of the resolved EDM graph. References
/// are `Weak` to avoid reference cycles among container elements (two entity
/// sets can legally bind to each other). [`BindingPathSegment::Unresolved`] carries the
/// authored segment name when it could not be resolved against the model — the
/// resolver is best-effort and leaves such failures for the validator to report.
#[derive(Debug, Clone)]
pub enum BindingPathSegment {
    /// A structural property traversed on the way to
    /// the bound navigation property.
    Property(Weak<Property>),

    /// A navigation property — the terminal of a `path`, or a containment hop in
    /// either a `path` or a `target`.
    NavigationProperty(Weak<NavigationProperty>),

    /// A type-cast to a derived entity type (reserved; not resolved in v1).
    EntityTypeCast(Weak<EntityType>),

    /// A type-cast to a derived complex type (reserved; not resolved in v1).
    ComplexTypeCast(Weak<ComplexType>),

    /// An entity set — the head of a `target`.
    EntitySet(Weak<EntitySet>),

    /// A singleton — the head of a `target`.
    Singleton(Weak<Singleton>),

    /// A qualifying entity container in a `target` path (reserved).
    EntityContainer(Weak<EntityContainer>),

    /// An authored segment name that did not resolve against the model.
    Unresolved(String),
}

impl BindingPathSegment {
    pub fn display_name(&self) -> String {
        match self {
            Self::Property(property) => property
                .upgrade()
                .map(|property| property.name.clone())
                .unwrap_or_else(|| "<dangling-property>".to_owned()),
            Self::NavigationProperty(navigation) => navigation
                .upgrade()
                .map(|navigation| navigation.name.clone())
                .unwrap_or_else(|| "<dangling-navigation>".to_owned()),
            Self::EntityTypeCast(entity_type) => entity_type
                .upgrade()
                .map(|entity_type| entity_type.name.clone())
                .unwrap_or_else(|| "<dangling-entity-type>".to_owned()),
            Self::ComplexTypeCast(complex_type) => complex_type
                .upgrade()
                .map(|complex_type| complex_type.name.clone())
                .unwrap_or_else(|| "<dangling-complex-type>".to_owned()),
            Self::EntitySet(entity_set) => entity_set
                .upgrade()
                .map(|entity_set| entity_set.name.clone())
                .unwrap_or_else(|| "<dangling-entity-set>".to_owned()),
            Self::Singleton(singleton) => singleton
                .upgrade()
                .map(|singleton| singleton.name.clone())
                .unwrap_or_else(|| "<dangling-singleton>".to_owned()),
            Self::EntityContainer(container) => container
                .upgrade()
                .map(|container| container.name.clone())
                .unwrap_or_else(|| "<dangling-container>".to_owned()),
            Self::Unresolved(name) => name.clone(),
        }
    }
}

fn binding_path_segments_to_string(path: &[BindingPathSegment]) -> String {
    path.iter()
        .map(BindingPathSegment::display_name)
        .collect::<Vec<_>>()
        .join("/")
}

/// Render a resolved binding path (or partner path) back to its `Segment/Segment`
/// string form.
pub fn binding_path_to_string(path: &[BindingPathSegment]) -> String {
    binding_path_segments_to_string(path)
}

/// One segment of a resolved entity key path. A key path walks structural
/// properties (descending through complex types) and terminates at a primitive
/// property. [`KeyPathSegment::Unresolved`] carries the authored name when a
/// segment could not be resolved against the model.
#[derive(Debug, Clone)]
pub enum KeyPathSegment {
    Property(Weak<Property>),
    NavigationProperty(Weak<NavigationProperty>),
    Unresolved(String),
}

impl KeyPathSegment {
    pub fn display_name(&self) -> String {
        match self {
            Self::NavigationProperty(nav) => nav
                .upgrade()
                .map(|n| n.name.clone())
                .unwrap_or_else(|| "<dangling-navigation>".into()),
            Self::Property(property) => property
                .upgrade()
                .map(|property| property.name.clone())
                .unwrap_or_else(|| "<dangling-property>".to_owned()),
            Self::Unresolved(name) => name.clone(),
        }
    }
}

/// Render a resolved key path back to its `Segment/Segment` string form.
pub fn key_path_to_string(path: &[KeyPathSegment]) -> String {
    path.iter()
        .map(KeyPathSegment::display_name)
        .collect::<Vec<_>>()
        .join("/")
}

/// One segment of a resolved operation `EntitySetPath`. The head names the
/// binding parameter; the remaining segments walk that parameter's type via
/// navigation/structural properties. Type-cast (qualified) segments and any
/// segment that fails to resolve become [`EntitySetPathSegment::Unresolved`].
#[derive(Debug, Clone)]
pub enum EntitySetPathSegment {
    /// The binding-parameter head of the path.
    BindingParameter(String),
    EntityTypeCast(Weak<EntityType>),
    NavigationProperty(Weak<NavigationProperty>),
    Property(Weak<Property>),
    Unresolved(String),
}

impl EntitySetPathSegment {
    pub fn display_name(&self) -> String {
        match self {
            Self::BindingParameter(name) => name.clone(),
            Self::EntityTypeCast(ty) => ty
                .upgrade()
                .map(|ty| ty.name.clone())
                .unwrap_or_else(|| "<dangling-type>".into()),
            Self::NavigationProperty(navigation) => navigation
                .upgrade()
                .map(|navigation| navigation.name.clone())
                .unwrap_or_else(|| "<dangling-navigation>".to_owned()),
            Self::Property(property) => property
                .upgrade()
                .map(|property| property.name.clone())
                .unwrap_or_else(|| "<dangling-property>".to_owned()),
            Self::Unresolved(name) => name.clone(),
        }
    }
}

/// Render a resolved entity-set path back to its `Segment/Segment` string form.
pub fn entity_set_path_to_string(path: &[EntitySetPathSegment]) -> String {
    path.iter()
        .map(EntitySetPathSegment::display_name)
        .collect::<Vec<_>>()
        .join("/")
}
