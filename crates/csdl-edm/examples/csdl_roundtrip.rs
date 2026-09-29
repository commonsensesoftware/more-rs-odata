use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;
use std::path::Path;

use csdl_edm::parser;
use csdl_edm::{CsdlDocument, CsdlFormat, Error, Result};
use serde_json::Value;
use similar::TextDiff;

fn main() -> Result<()> {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let inputs = project_root.join("data").join("inputs");
    let mut mismatches = Vec::new();

    for sample in sample_names(&inputs)? {
        if let Err(err) = compare_sample(&sample, &inputs) {
            mismatches.push(err.to_string());
        }
    }

    if mismatches.is_empty() {
        return Ok(());
    }

    Err(Error::Csdl(format!(
        "{} sample(s) are not equivalent:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    )))
}

#[test]
fn comparisons_report_equivalence_and_known_mismatches() {
    let inputs = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/inputs");
    assert_eq!(sample_names(&inputs).unwrap().len(), 8);
    compare_sample("enum_sample", &inputs).unwrap();
    compare_sample("extras_sample", &inputs).unwrap();
    let mismatch = compare_sample("reporting_line", &inputs).unwrap_err();
    assert!(mismatch.to_string().contains("not equivalent"));
}

fn compare_sample(sample: &str, inputs: &Path) -> Result<()> {
    let xml_in = inputs.join(format!("{sample}.csdl.xml"));
    let json_in = inputs.join(format!("{sample}.csdl.json"));

    let from_xml = CsdlDocument::from_path(&xml_in)
        .map_err(|err| Error::Csdl(format!("read {}: {err}", xml_in.display())))?;
    let from_json = CsdlDocument::from_path(&json_in)
        .map_err(|err| Error::Csdl(format!("read {}: {err}", json_in.display())))?;

    if from_xml == from_json {
        println!("[{sample}] OK - XML and JSON inputs are equivalent");
        return Ok(());
    }

    println!("[{sample}] mismatch between XML and JSON inputs");
    report_model_diff(sample, &from_xml, &from_json)?;
    Err(Error::Csdl(format!(
        "{sample}: XML and JSON inputs are not equivalent"
    )))
}

fn report_model_diff(
    sample: &str,
    from_xml: &CsdlDocument,
    from_json: &CsdlDocument,
) -> Result<()> {
    let xml_json = serialize_json(from_xml)?;
    let json_json = serialize_json(from_json)?;
    if xml_json != json_json {
        let diff = TextDiff::from_lines(&xml_json, &json_json)
            .unified_diff()
            .header("from_xml as JSON", "from_json as JSON")
            .to_string();
        println!("  [{sample}] canonical JSON diff:\n{diff}");
    }

    let xml_xml = serialize_xml(from_xml)?;
    let json_xml = serialize_xml(from_json)?;
    let left_doc = parser::from_xml_reader(Cursor::new(xml_xml.as_bytes()))
        .map_err(|err| Error::Csdl(format!("parse XML serialization (from XML input): {err}")))?;
    let right_doc = parser::from_xml_reader(Cursor::new(json_xml.as_bytes()))
        .map_err(|err| Error::Csdl(format!("parse XML serialization (from JSON input): {err}")))?;
    if left_doc != right_doc {
        let diff = TextDiff::from_lines(&xml_xml, &json_xml)
            .unified_diff()
            .header("from_xml as XML", "from_json as XML")
            .to_string();
        println!("  [{sample}] canonical XML diff:\n{diff}");
    }

    Ok(())
}

fn serialize_json(doc: &CsdlDocument) -> Result<String> {
    let mut out = Vec::new();
    doc.write_as(&mut out, CsdlFormat::Json)
        .map_err(|err| Error::Csdl(format!("serialize as JSON: {err}")))?;
    let text = String::from_utf8(out)
        .map_err(|err| Error::Csdl(format!("JSON output is not UTF-8: {err}")))?;
    let normalized: Value = serde_json::from_str(&text)
        .map_err(|err| Error::Csdl(format!("parse serialized JSON: {err}")))?;
    serde_json::to_string_pretty(&normalized)
        .map_err(|err| Error::Csdl(format!("normalize serialized JSON: {err}")))
}

fn serialize_xml(doc: &CsdlDocument) -> Result<String> {
    let mut out = Vec::new();
    doc.write_as(&mut out, CsdlFormat::Xml)
        .map_err(|err| Error::Csdl(format!("serialize as XML: {err}")))?;
    String::from_utf8(out).map_err(|err| Error::Csdl(format!("XML output is not UTF-8: {err}")))
}

fn sample_names(inputs: &Path) -> Result<Vec<String>> {
    let mut xml_samples = BTreeSet::new();
    let mut json_samples = BTreeSet::new();

    for path in fs::read_dir(inputs)
        .map_err(|err| Error::Csdl(format!("list {}: {err}", short_path(inputs))))?
        .map(|entry| entry.map(|e| e.path()))
    {
        let path = path.map_err(|err| Error::Csdl(format!("read dir entry: {err}")))?;
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if let Some(base) = name.strip_suffix(".csdl.xml") {
            xml_samples.insert(base.to_owned());
            continue;
        }

        if let Some(base) = name.strip_suffix(".csdl.json") {
            json_samples.insert(base.to_owned());
        }
    }

    let mut names = xml_samples
        .intersection(&json_samples)
        .cloned()
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

fn short_path(p: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    match p.strip_prefix(root) {
        Ok(rest) => rest.display().to_string(),
        Err(_) => p.display().to_string(),
    }
}
