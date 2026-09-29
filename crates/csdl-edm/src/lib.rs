pub mod csdl;
pub mod csdl_json_reader;
pub mod csdl_xml_reader;
pub mod edm;
pub mod error;
pub mod graph;
pub mod expr;
pub mod parser;
pub mod path_expansion;
pub mod resolver;
pub mod serialization;
mod spatial;
pub mod validator;

pub use csdl::CsdlDocument;
pub use error::{Error, Result};
pub use serialization::CsdlFormat;
