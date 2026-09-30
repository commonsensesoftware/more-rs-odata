pub mod csdl;
pub mod conformance;
pub mod csdl_json_reader;
pub mod csdl_xml_reader;
#[cfg(feature = "edm")]
pub mod edm;
pub mod error;
#[cfg(feature = "edm")]
pub mod graph;
pub mod expr;
pub mod parser;
#[cfg(feature = "edm")]
pub mod path_expansion;
#[cfg(feature = "resolver")]
pub mod resolver;
pub mod serialization;
mod spatial;
#[cfg(feature = "validator")]
pub mod validator;

pub use csdl::CsdlDocument;
pub use error::{Error, Result};
pub use serialization::CsdlFormat;
