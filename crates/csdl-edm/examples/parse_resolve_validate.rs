use std::path::Path;

use csdl_edm::CsdlDocument;
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::validate_document;

fn main() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = std::env::args().nth(1).map_or_else(
        || {
            project_root
                .join("data")
                .join("inputs")
                .join("example89.csdl.xml")
        },
        |value| project_root.join(value),
    );

    load(&input);
    println!("Parsed, resolved, and validated {}", input.display());
}

fn load(input: &Path) -> csdl_edm::edm::DocumentModel {
    let parsed = CsdlDocument::from_path(input).expect("parse CSDL input");
    let edmx = parsed.edmx.expect("missing Edmx document");
    let edm = Resolver::resolve_edmx_document(edmx).expect("resolve to EDM");
    validate_document(&edm).expect("validate EDM");

    edm
}

#[test]
fn pipeline_accepts_both_formats() {
    let inputs = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/inputs");
    let xml = load(&inputs.join("example89.csdl.xml"));
    let json = load(&inputs.join("example89.csdl.json"));
    assert_eq!(xml.schemas.len(), json.schemas.len());
    assert_eq!(
        xml.schemas[0].elements.len(),
        json.schemas[0].elements.len()
    );
}
