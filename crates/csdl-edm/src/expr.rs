#[derive(Debug, Clone, PartialEq)]
pub enum CsdlAnnotationExpression {
    /// Metadata attached to a dynamic expression, without changing its value.
    Annotated {
        expression: Box<CsdlAnnotationExpression>,
        annotations: Vec<Annotation>,
    },
    /// Facets authored on Cast or IsOf.
    Faceted {
        expression: Box<CsdlAnnotationExpression>,
        facets: ExpressionFacets,
    },
    // Constant expressions
    Binary(Vec<u8>),
    Bool(bool),
    Date(String),
    DateTimeOffset(String),
    Decimal(String),
    Duration(String),
    EnumMember(String),
    Float(f64),
    Guid(String),
    Int(i64),
    String(String),
    TimeOfDay(String),
    Null,

    // Path expressions
    Path(String),
    PropertyPath(String),
    NavigationPropertyPath(String),
    AnnotationPath(String),
    ModelElementPath(String),

    // Dynamic expressions
    Not(Box<CsdlAnnotationExpression>),
    Neg(Box<CsdlAnnotationExpression>),
    BinaryExpression {
        op: BinaryOperator,
        lhs: Box<CsdlAnnotationExpression>,
        rhs: Box<CsdlAnnotationExpression>,
    },

    If {
        test: Box<CsdlAnnotationExpression>,
        then_: Box<CsdlAnnotationExpression>,
        else_: Option<Box<CsdlAnnotationExpression>>,
    },
    Apply {
        function: String,
        args: Vec<CsdlAnnotationExpression>,
    },
    Cast {
        type_: String,
        expr: Box<CsdlAnnotationExpression>,
    },
    IsOf {
        type_: String,
        expr: Box<CsdlAnnotationExpression>,
    },

    Record {
        type_: Option<String>,
        properties: Vec<PropertyValue>,
        /// Record-level own- and sibling-target annotations (CSDL 14.5.14).
        /// `<Annotation>` children of `<Record>` in XML; `@Term` keys on the
        /// Record object in JSON. Sibling-target keys (`Foo@Term`) attach to
        /// the named `PropertyValue` via `PropertyValue::annotations`, not
        /// here.
        annotations: Vec<Annotation>,
    },
    Collection(Vec<CsdlAnnotationExpression>),

    LabeledElement {
        name: String,
        expr: Box<CsdlAnnotationExpression>,
    },
    LabeledElementReference(String),

    UrlRef(Box<CsdlAnnotationExpression>),
}

/// Operator for a binary [`CsdlAnnotationExpression::Binary`] expression.
/// Mirrors the CSDL 4.01 binary dynamic-expression element names (14.5.1, 14.5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOperator {
    Has,
    In,
    Add,
    Sub,
    Mul,
    Div,
    DivBy,
    Mod,
    And,
    Or,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpressionFacets {
    pub nullable: Option<bool>,
    pub max_length: Option<crate::csdl::MaxLengthFacet>,
    pub precision: Option<String>,
    pub scale: Option<crate::csdl::ScaleFacet>,
    pub srid: Option<crate::csdl::SridFacet>,
    pub unicode: Option<bool>,
}

impl ExpressionFacets {
    pub(crate) fn from_attributes(attrs: &[(String, String)]) -> crate::Result<Self> {
        use crate::{Error, csdl::*};
        let get = |key| {
            attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let boolean = |key| {
            get(key)
                .map(|s| match s {
                    "true" => Ok(true),
                    "false" => Ok(false),
                    _ => Err(Error::Csdl(format!("{key} must be boolean"))),
                })
                .transpose()
        };
        Ok(Self {
            nullable: boolean("Nullable")?,
            unicode: boolean("Unicode")?,
            max_length: get("MaxLength")
                .map(|s| {
                    MaxLengthFacet::parse(s).ok_or_else(|| Error::Csdl("Invalid MaxLength".into()))
                })
                .transpose()?,
            precision: get("Precision").map(str::to_owned),
            scale: get("Scale")
                .map(|s| ScaleFacet::parse(s).ok_or_else(|| Error::Csdl("Invalid Scale".into())))
                .transpose()?,
            srid: get("SRID")
                .map(|s| SridFacet::parse(s).ok_or_else(|| Error::Csdl("Invalid SRID".into())))
                .transpose()?,
        })
    }
    pub(crate) fn attributes(&self) -> Vec<(String, String)> {
        [
            ("Nullable", self.nullable.map(|v| v.to_string())),
            ("Unicode", self.unicode.map(|v| v.to_string())),
            ("MaxLength", self.max_length.map(|v| v.as_str())),
            ("Precision", self.precision.clone()),
            ("Scale", self.scale.map(|v| v.as_str())),
            ("SRID", self.srid.map(|v| v.as_str())),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k.into(), v)))
        .collect()
    }
}

impl BinaryOperator {
    pub fn name(self) -> &'static str {
        match self {
            Self::And => "And",
            Self::Or => "Or",
            Self::Eq => "Eq",
            Self::Ne => "Ne",
            Self::Gt => "Gt",
            Self::Ge => "Ge",
            Self::Lt => "Lt",
            Self::Le => "Le",
            Self::Has => "Has",
            Self::In => "In",
            Self::Add => "Add",
            Self::Sub => "Sub",
            Self::Mul => "Mul",
            Self::Div => "Div",
            Self::DivBy => "DivBy",
            Self::Mod => "Mod",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "And" => Self::And,
            "Or" => Self::Or,
            "Eq" => Self::Eq,
            "Ne" => Self::Ne,
            "Gt" => Self::Gt,
            "Ge" => Self::Ge,
            "Lt" => Self::Lt,
            "Le" => Self::Le,
            "Has" => Self::Has,
            "In" => Self::In,
            "Add" => Self::Add,
            "Sub" => Self::Sub,
            "Mul" => Self::Mul,
            "Div" => Self::Div,
            "DivBy" => Self::DivBy,
            "Mod" => Self::Mod,
            _ => return None,
        })
    }
}

impl CsdlAnnotationExpression {
    pub fn kind_name(&self) -> &'static str {
        use CsdlAnnotationExpression::*;
        match self {
            Annotated { expression, .. } | Faceted { expression, .. } => expression.kind_name(),
            BinaryExpression { op, .. } => op.name(),
            Neg(_) => "Neg",
            Not(_) => "Not",
            If { .. } => "If",
            Apply { .. } => "Apply",
            Cast { .. } => "Cast",
            IsOf { .. } => "IsOf",
            Record { .. } => "Record",
            Collection(_) => "Collection",
            LabeledElement { .. } => "LabeledElement",
            LabeledElementReference(_) => "LabeledElementReference",
            UrlRef(_) => "UrlRef",
            ModelElementPath(_) => "ModelElementPath",
            Path(_) => "Path",
            PropertyPath(_) => "PropertyPath",
            NavigationPropertyPath(_) => "NavigationPropertyPath",
            AnnotationPath(_) => "AnnotationPath",
            Null => "Null",
            Binary(_) => "Binary",
            Bool(_) => "Bool",
            Date(_) => "Date",
            DateTimeOffset(_) => "DateTimeOffset",
            Decimal(_) => "Decimal",
            Duration(_) => "Duration",
            EnumMember(_) => "EnumMember",
            Float(_) => "Float",
            Guid(_) => "Guid",
            Int(_) => "Int",
            String(_) => "String",
            TimeOfDay(_) => "TimeOfDay",
        }
    }
}

/// A single PropertyValue inside a Record (CSDL 14.5.14). Semantically carries
/// one annotation expression. The reader's recursive parser takes the first
/// expression it sees (inline attribute form takes precedence over nested
/// element form); any extras are dropped. `None` means no expression was
/// present at all.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyValue {
    pub property: String,
    pub value: Option<CsdlAnnotationExpression>,
    pub annotations: Vec<Annotation>,
}

// `Annotation` lives in `model.rs`. `PropertyValue` carries them too, so we
// import the model definition here. The two modules are mutually-referential
// (model uses CsdlAnnotationExpression from here, we use Annotation from
// model) — Rust handles that fine.
use crate::csdl::Annotation;

/// Convert a scalar default's lexical representation to an annotation value.
/// Invalid lexical values remain strings for authoring diagnostics to report.
pub(crate) fn default_literal(type_name: &str, value: Option<&str>) -> CsdlAnnotationExpression {
    use CsdlAnnotationExpression as E;
    let Some(value) = value else {
        return E::Null;
    };
    match type_name {
        "Edm.Boolean" => value
            .parse()
            .map(E::Bool)
            .unwrap_or_else(|_| E::String(value.into())),
        "Edm.Byte" | "Edm.SByte" | "Edm.Int16" | "Edm.Int32" | "Edm.Int64" => value
            .parse()
            .map(E::Int)
            .unwrap_or_else(|_| E::String(value.into())),
        "Edm.Single" | "Edm.Double" => match value {
            "INF" => E::Float(f64::INFINITY),
            "-INF" => E::Float(f64::NEG_INFINITY),
            "NaN" => E::Float(f64::NAN),
            _ => value
                .parse()
                .map(E::Float)
                .unwrap_or_else(|_| E::String(value.into())),
        },
        "Edm.Decimal" => E::Decimal(value.into()),
        "Edm.Date" => E::Date(value.into()),
        "Edm.DateTimeOffset" => E::DateTimeOffset(value.into()),
        "Edm.Duration" => E::Duration(value.into()),
        "Edm.Guid" => E::Guid(value.into()),
        "Edm.TimeOfDay" => E::TimeOfDay(value.into()),
        _ => E::String(value.into()),
    }
}
