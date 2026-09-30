use csdl_edm::{CsdlFormat, conformance::ValidationProfile, parser};
use std::collections::HashSet;

fn xml(body: &str) -> String {
    format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    )
}

#[test]
fn strict_xml_cardinalities_and_attribute_escaping() {
    for body in [
        r#"<EnumType Name="Empty"/>"#,
        r#"<EntityContainer Name="Empty"/>"#,
    ] {
        assert!(
            parser::from_xml_reader_with_profile(xml(body).as_bytes(), ValidationProfile::Strict)
                .is_err()
        );
    }
    let doc = parser::from_xml_reader(xml(r#"<Term Name="Text" Type="Edm.String" DefaultValue="A &amp; B &quot;C&quot;"/><Annotation Term="N.Text" String="A &amp; B &quot;C&quot;"/>"#).as_bytes()).unwrap();
    assert_eq!(doc.to_json_value()["N"]["@N.Text"], "A & B \"C\"");
    let mut bytes = Vec::new();
    doc.write_as_with_profile(&mut bytes, CsdlFormat::Xml, ValidationProfile::Strict)
        .unwrap();
    assert_eq!(doc, parser::from_xml_reader(bytes.as_slice()).unwrap());
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn two_operand_if_is_only_valid_as_a_collection_item() {
    use csdl_edm::{resolver::Resolver, validator};
    for (ty, expr, valid) in [
        ("Edm.Int64", "<If><Bool>true</Bool><Int>1</Int></If>", false),
        (
            "Collection(Edm.Int64)",
            "<Collection><If><Bool>true</Bool><Int>1</Int></If></Collection>",
            true,
        ),
    ] {
        let doc = parser::from_xml_reader(
            xml(&format!(
                r#"<Term Name="Value" Type="{ty}"/><Annotation Term="N.Value">{expr}</Annotation>"#
            ))
            .as_bytes(),
        )
        .unwrap();
        for doc in [
            doc.clone(),
            parser::from_json_reader(doc.to_json_value().to_string().as_bytes()).unwrap(),
        ] {
            let model = Resolver::resolve_document(doc.edmx.unwrap()).unwrap();
            assert_eq!(validator::validate_annotations(&model).is_empty(), valid);
        }
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
#[test]
fn count_path_resolves_and_validates() {
    use csdl_edm::{resolver::Resolver, validator};
    let doc = parser::from_xml_reader(xml(r#"<Term Name="Value" Type="Edm.Int64"/><ComplexType Name="C"><Property Name="Names" Type="Collection(Edm.String)" Nullable="false"/><Annotation Term="N.Value"><Path>Names/$count</Path></Annotation></ComplexType>"#).as_bytes()).unwrap();
    let model = Resolver::resolve_document(doc.edmx.unwrap()).unwrap();
    assert!(validator::validate_annotations(&model).is_empty());
}

#[test]
fn every_inventoried_obligation_has_a_matrix_disposition_and_real_evidence_links() {
    let inventory: serde_json::Value =
        serde_json::from_str(include_str!("../docs/conformance-requirements.json")).unwrap();
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("../docs/conformance-matrix.json")).unwrap();
    let rows = inventory["requirements"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        485,
        "Regenerate and review the audit when changing source editions"
    );
    let mut ids = HashSet::new();
    for row in rows {
        assert!(ids.insert(row["id"].as_str().unwrap()));
        assert!(
            row["source"]
                .as_str()
                .unwrap()
                .starts_with("https://docs.oasis-open.org/")
        );
        let section = row["section"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert!(
            matrix["groups"].as_array().unwrap().iter().any(|group| {
                let prefix = group["section"].as_str().unwrap();
                section == prefix
                    || section
                        .strip_prefix(prefix)
                        .is_some_and(|s| s.starts_with('.'))
            }),
            "Unclassified requirement {}",
            row["id"]
        );
    }
    for group in matrix["groups"].as_array().unwrap() {
        assert!(
            matrix["status_meaning"]
                .get(group["status"].as_str().unwrap())
                .is_some()
        );
        for key in ["code", "tests"] {
            for link in group[key].as_array().unwrap() {
                let link = link.as_str().unwrap();
                let (file, function) = link
                    .split_once("::")
                    .map_or((link, None), |(f, n)| (f, Some(n)));
                let text = std::fs::read_to_string(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file),
                )
                .unwrap_or_else(|e| panic!("{link}: {e}"));
                if let Some(function) = function {
                    assert!(
                        text.contains(&format!("fn {function}(")),
                        "Missing test {link}"
                    );
                }
            }
        }
    }
    let mut overrides = HashSet::new();
    for entry in matrix["overrides"].as_array().unwrap() {
        for id in entry["ids"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            assert!(ids.contains(id), "Unknown override {id}");
            assert!(overrides.insert(id), "Duplicate override {id}");
        }
    }
}

#[test]
fn every_advisory_has_an_explicit_implementation_or_caller_policy() {
    let inventory: serde_json::Value =
        serde_json::from_str(include_str!("../docs/conformance-requirements.json")).unwrap();
    let policies: serde_json::Value =
        serde_json::from_str(include_str!("../docs/advisory-policies.json")).unwrap();
    let required = inventory["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["keywords"].as_array().unwrap().iter().any(|keyword| {
                matches!(
                    keyword.as_str(),
                    Some("SHOULD" | "SHOULD NOT" | "RECOMMENDED")
                )
            })
        })
        .map(|row| row["id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    let mut accounted = HashSet::new();
    for policy in policies["policies"].as_array().unwrap() {
        assert!(!policy["policy"].as_str().unwrap().is_empty());
        for id in policy["ids"].as_array().unwrap() {
            assert!(
                accounted.insert(id.as_str().unwrap()),
                "Duplicate policy for {id}"
            );
        }
    }
    assert_eq!(required, accounted);
}

#[test]
fn decomposed_obligations_link_real_paragraphs_and_focused_evidence() {
    let inventory: serde_json::Value =
        serde_json::from_str(include_str!("../docs/conformance-requirements.json")).unwrap();
    let obligations: serde_json::Value =
        serde_json::from_str(include_str!("../docs/conformance-obligations.json")).unwrap();
    let paragraphs = inventory["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    let mut ids = HashSet::new();
    for obligation in obligations["obligations"].as_array().unwrap() {
        assert!(ids.insert(obligation["id"].as_str().unwrap()));
        for paragraph in obligation["paragraphs"].as_array().unwrap() {
            assert!(paragraphs.contains(paragraph.as_str().unwrap()));
        }
        if obligation["status"] == "covered" {
            assert!(!obligation["tests"].as_array().unwrap().is_empty());
        }
        for test in obligation["tests"].as_array().into_iter().flatten() {
            let link = test.as_str().unwrap();
            let (file, function) = link
                .split_once("::")
                .map_or((link, None), |(f, n)| (f, Some(n)));
            let source = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file),
            )
            .unwrap();
            if let Some(function) = function {
                assert!(source.contains(&format!("fn {function}(")), "{link}");
            }
        }
    }
}
