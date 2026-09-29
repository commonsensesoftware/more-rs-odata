use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use csdl_edm::CsdlDocument;
use rstest::rstest;
use serde_json::Value;
use similar::TextDiff;

#[test]
fn format_equivalence_case_list_matches_fixture_pairs() {
    let expected = expected_equivalence();
    let samples = sample_names();

    let expected_names = expected.keys().cloned().collect::<Vec<_>>();
    assert_eq!(
        samples, expected_names,
        "sample set mismatch between fixtures and expected equivalence baseline"
    );
}

#[rstest]
#[case::enum_sample("enum_sample", ExpectedEquivalence::Equivalent)]
#[case::extras_sample("extras_sample", ExpectedEquivalence::Equivalent)]
#[case::record_sample("record_sample", ExpectedEquivalence::KnownMismatch("JSON defaults to Edm.String; XML omits required Type"))]
#[case::reporting_line("reporting_line", ExpectedEquivalence::KnownMismatch("JSON fixture omits required key aliases"))]
#[case::example89(
    "example89",
    ExpectedEquivalence::KnownMismatch(
        "alias/default facets/reference URI and annotation shape differ"
    )
)]
#[case::fields_sample(
    "fields_sample",
    ExpectedEquivalence::KnownMismatch("schema alias and qualified type usage differ")
)]
#[case::import_sample(
    "import_sample",
    ExpectedEquivalence::KnownMismatch("schema alias differs")
)]
#[case::types_sample(
    "types_sample",
    ExpectedEquivalence::KnownMismatch("schema alias and operation parameter type refs differ")
)]
fn paired_xml_and_json_inputs_are_equivalent(
    #[case] sample: &str,
    #[case] expected: ExpectedEquivalence,
) {
    let xml_path = input_path(sample, SourceFormat::Xml);
    let json_path = input_path(sample, SourceFormat::Json);

    let from_xml = CsdlDocument::from_path(&xml_path)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", xml_path.display()));
    let from_json = CsdlDocument::from_path(&json_path)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", json_path.display()));

    let equivalent = from_xml == from_json;
    match expected {
        ExpectedEquivalence::Equivalent => {
            if !equivalent {
                panic!(
                    "unexpected equivalence mismatch for sample {sample}\n{}",
                    mismatch_details(sample, &from_xml, &from_json)
                );
            }
        }
        ExpectedEquivalence::KnownMismatch(reason) => {
            assert!(
                !equivalent,
                "sample {sample} is now equivalent; remove KnownMismatch baseline ({reason})"
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ExpectedEquivalence {
    Equivalent,
    KnownMismatch(&'static str),
}

fn expected_equivalence() -> BTreeMap<String, ExpectedEquivalence> {
    [
        ("enum_sample", ExpectedEquivalence::Equivalent),
        (
            "example89",
            ExpectedEquivalence::KnownMismatch(
                "alias/default facets/reference URI and annotation shape differ",
            ),
        ),
        ("extras_sample", ExpectedEquivalence::Equivalent),
        (
            "fields_sample",
            ExpectedEquivalence::KnownMismatch("schema alias and qualified type usage differ"),
        ),
        (
            "import_sample",
            ExpectedEquivalence::KnownMismatch("schema alias differs"),
        ),
        ("record_sample", ExpectedEquivalence::KnownMismatch("JSON defaults to Edm.String; XML omits required Type")),
        ("reporting_line", ExpectedEquivalence::KnownMismatch("JSON fixture omits required key aliases")),
        (
            "types_sample",
            ExpectedEquivalence::KnownMismatch(
                "schema alias and operation parameter type refs differ",
            ),
        ),
    ]
    .into_iter()
    .map(|(name, expectation)| (name.to_string(), expectation))
    .collect()
}

fn mismatch_details(sample: &str, from_xml: &CsdlDocument, from_json: &CsdlDocument) -> String {
    let mut details = Vec::new();

    let left_json = serialize_json(from_xml, "from XML input");
    let right_json = serialize_json(from_json, "from JSON input");
    if left_json != right_json {
        let diff = TextDiff::from_lines(&left_json, &right_json)
            .unified_diff()
            .header("from_xml as JSON", "from_json as JSON")
            .to_string();
        details.push(format!("JSON canonical diff:\n{diff}"));
    }

    let left_xml = serialize_xml(from_xml, "from XML input");
    let right_xml = serialize_xml(from_json, "from JSON input");
    let left_xml_doc = csdl_edm::parser::from_xml_reader(Cursor::new(left_xml.as_bytes()))
        .unwrap_or_else(|err| {
            panic!("invalid XML serialization for sample {sample} (from XML input): {err}")
        });
    let right_xml_doc = csdl_edm::parser::from_xml_reader(Cursor::new(right_xml.as_bytes()))
        .unwrap_or_else(|err| {
            panic!("invalid XML serialization for sample {sample} (from JSON input): {err}")
        });
    if left_xml_doc != right_xml_doc {
        let diff = TextDiff::from_lines(&left_xml, &right_xml)
            .unified_diff()
            .header("from_xml as XML", "from_json as XML")
            .to_string();
        details.push(format!("XML canonical diff:\n{diff}"));
    }

    details.join("\n")
}

fn serialize_json(doc: &CsdlDocument, context: &str) -> String {
    let mut out = Vec::new();
    doc.write_as(&mut out, csdl_edm::CsdlFormat::Json)
        .unwrap_or_else(|err| panic!("failed to serialize {context} as JSON: {err}"));

    let text = String::from_utf8(out)
        .unwrap_or_else(|err| panic!("JSON output is not utf8 ({context}): {err}"));
    let normalized: Value = serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("invalid serialized JSON ({context}): {err}"));
    serde_json::to_string_pretty(&normalized)
        .unwrap_or_else(|err| panic!("failed to normalize serialized JSON ({context}): {err}"))
}

fn serialize_xml(doc: &CsdlDocument, context: &str) -> String {
    let mut out = Vec::new();
    doc.write_as(&mut out, csdl_edm::CsdlFormat::Xml)
        .unwrap_or_else(|err| panic!("failed to serialize {context} as XML: {err}"));
    String::from_utf8(out).unwrap_or_else(|err| panic!("XML output is not utf8 ({context}): {err}"))
}

#[derive(Debug, Clone, Copy)]
enum SourceFormat {
    Json,
    Xml,
}

fn sample_names() -> Vec<String> {
    let mut xml_samples = BTreeSet::new();
    let mut json_samples = BTreeSet::new();

    for path in fs::read_dir(inputs_dir())
        .expect("list data/inputs")
        .map(|entry| entry.expect("valid dir entry").path())
    {
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
    names
}

fn input_path(sample: &str, source: SourceFormat) -> PathBuf {
    let ext = match source {
        SourceFormat::Json => "json",
        SourceFormat::Xml => "xml",
    };

    let path = inputs_dir().join(format!("{sample}.csdl.{ext}"));
    assert!(path.is_file(), "missing fixture input: {}", path.display());
    path
}

fn inputs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("inputs")
}
