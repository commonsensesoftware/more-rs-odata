#[derive(Debug, Clone, PartialEq)]
pub enum CsdlAnnotationExpression {
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

    // Dynamic expressions
    Not(Box<CsdlAnnotationExpression>),
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
    And,
    Or,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
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
