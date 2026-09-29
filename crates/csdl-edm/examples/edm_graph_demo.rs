//! Load CSDL, resolve to the EDM semantic model, and export the semantic graph
//! as `graph.json` for the `edm-graph` visualizer.
//!
//! Usage:
//!   cargo run --example edm_graph_demo -- <input.csdl.xml|.json> [out.json]
//!
//! With no arguments it reads the bundled `example89.csdl.xml` and writes
//! `apps/edm-graph/public/graph.json`.

use std::path::{Path, PathBuf};

use csdl_edm::CsdlDocument;
use csdl_edm::graph::build_graph;
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::validate_document;

fn main() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut args = std::env::args().skip(1);
    let input: PathBuf = args.next().map_or_else(
        || {
            project_root
                .join("data")
                .join("inputs")
                .join("example89.csdl.xml")
        },
        PathBuf::from,
    );
    // Default output is the in-repo visualizer's runtime file, so a plain
    // `cargo run --example edm_graph_demo` refreshes what the app serves.
    let output: PathBuf = args.next().map_or_else(
        || project_root.join("../../apps/edm-graph/public/graph.json"),
        PathBuf::from,
    );

    export(&input, &output);
}

fn export(input: &Path, output: &Path) {
    let parsed = CsdlDocument::from_path(input).expect("parse CSDL input");
    let edmx = parsed.edmx.expect("missing Edmx document");
    let edm = Resolver::resolve_edmx_document(edmx).expect("resolve to EDM");
    validate_document(&edm).expect("validate EDM");

    let graph = build_graph(&edm);
    let json = serde_json::to_string_pretty(&graph).expect("serialize graph");
    std::fs::write(output, json).expect("write graph.json");

    println!(
        "Wrote {} nodes, {} edges, {} paths to {}",
        graph.nodes.len(),
        graph.edges.len(),
        graph.paths.len(),
        output.display()
    );
}

#[test]
fn exported_graph_matches_the_public_graph_api() {
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/inputs/example89.csdl.xml");
    let output = std::env::temp_dir().join(format!("csdl-edm-graph-{}.json", std::process::id()));
    export(&input, &output);
    let actual: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    std::fs::remove_file(output).unwrap();
    let parsed = CsdlDocument::from_path(input).unwrap();
    let model = Resolver::resolve_document(parsed.edmx.unwrap()).unwrap();
    let expected = serde_json::to_value(build_graph(&model)).unwrap();
    assert_eq!(actual, expected);
    assert!(!actual["nodes"].as_array().unwrap().is_empty());
}
