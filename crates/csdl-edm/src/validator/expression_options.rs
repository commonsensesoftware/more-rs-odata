use crate::edm::{TermType, TypeFacets};
use std::collections::HashMap;

/// Caller-provided static type information; this never executes client code.
#[derive(Debug, Clone)]
pub struct ExpressionType {
    pub ty: TermType,
    pub is_collection: bool,
    pub is_ordered: bool,
    pub nullable: bool,
    pub facets: TypeFacets,
}

#[derive(Debug, Clone)]
pub struct ClientFunctionSignature {
    pub parameters: Vec<ExpressionType>,
    pub return_type: ExpressionType,
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationValidationOptions {
    /// Keys are authored qualified client-function names.
    pub client_functions: HashMap<String, Vec<ClientFunctionSignature>>,
    /// Types of resources addressed by constant URL-reference operands.
    pub url_result_types: HashMap<String, ExpressionType>,
}
