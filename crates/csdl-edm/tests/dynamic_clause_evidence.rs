#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

#[test]
fn comparison_expressions_require_two_compatible_operands() {
    for operator in ["Eq", "Ne", "Gt", "Ge", "Lt", "Le"] {
        for count in 0..=3 {
            let xml = format!("<{operator}>{}</{operator}>", "<Int>1</Int>".repeat(count));
            let json = format!(r#"{{"${operator}":[{}]}}"#, vec!["1"; count].join(","));
            check(&xml, &json, false, count == 2);
        }
        for (xml, json, valid) in [
            ("<Int>1</Int><Float>2.5</Float>", "[1,2.5]", true),
            ("<String>a</String><String>b</String>", r#"["a","b"]"#, true),
            ("<Int>1</Int><String>a</String>", r#"[1,"a"]"#, false),
            (
                "<Collection><Int>1</Int></Collection><Int>1</Int>",
                "[[1],1]",
                false,
            ),
        ] {
            check(
                &format!("<{operator}>{xml}</{operator}>"),
                &format!(r#"{{"${operator}":{json}}}"#),
                false,
                valid,
            );
        }
    }
}

fn check(xml_expression: &str, json_expression: &str, collection: bool, valid: bool) {
    check_type(
        xml_expression,
        json_expression,
        "Edm.Boolean",
        collection,
        valid,
    );
}

fn check_type(
    xml_expression: &str,
    json_expression: &str,
    item_type: &str,
    collection: bool,
    valid: bool,
) {
    let ty = if collection {
        format!("Collection({item_type})")
    } else {
        item_type.to_owned()
    };
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{ty}" Nullable="false"/><ComplexType Name="Owner"><Annotation Term="N.Value">{xml_expression}</Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(
        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"{item_type}","$Collection":{collection}}},"Owner":{{"$Kind":"ComplexType","@N.Value":{json_expression}}}}}}}"#
    );
    for parsed in [
        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
    ] {
        let result = parsed
            .map_err(|e| e.to_string())
            .and_then(|d| Resolver::resolve_document(d.edmx.unwrap()).map_err(|e| format!("{e:?}")))
            .and_then(|m| {
                validator::validate_document(&m).map_err(|e| format!("{e:?}"))?;
                let diagnostics = validator::validate_annotations(&m);
                if diagnostics.is_empty() {
                    Ok(())
                } else {
                    Err(format!("{diagnostics:?}"))
                }
            });
        assert_eq!(
            result.is_ok(),
            valid,
            "{result:?}\n{xml_expression}\n{json_expression}"
        );
    }
}

#[test]
fn membership_requires_a_scalar_and_compatible_collection() {
    for (xml, json, valid) in [
        (
            "<Int>1</Int><Collection><Int>1</Int><Int>2</Int></Collection>",
            "[1,[1,2]]",
            true,
        ),
        ("<Int>1</Int><Int>2</Int>", "[1,2]", false),
        (
            "<Collection><Int>1</Int></Collection><Collection><Int>2</Int></Collection>",
            "[[1],[2]]",
            false,
        ),
        (
            "<Int>1</Int><Collection><String>a</String></Collection>",
            r#"[1,["a"]]"#,
            false,
        ),
        ("<Int>1</Int>", "[1]", false),
        ("<Int>1</Int><Collection/><Int>2</Int>", "[1,[],2]", false),
    ] {
        check(
            &format!("<In>{xml}</In>"),
            &format!(r#"{{"$In":{json}}}"#),
            false,
            valid,
        );
    }
}

#[test]
fn arithmetic_expressions_require_numeric_operands_and_exact_arity() {
    for operator in ["Add", "Sub", "Mul", "Div", "DivBy", "Mod", "Neg"] {
        let arity = if operator == "Neg" { 1 } else { 2 };
        for count in 0..=3 {
            let xml = format!("<{operator}>{}</{operator}>", "<Int>2</Int>".repeat(count));
            let json = if operator == "Neg" && count == 1 {
                r#"{"$Neg":2}"#.to_owned()
            } else {
                format!(r#"{{"${operator}":[{}]}}"#, vec!["2"; count].join(","))
            };
            check_type(&xml, &json, "Edm.Double", false, count == arity);
        }
        let (xml, json) = if operator == "Neg" {
            ("<Bool>true</Bool>", "true")
        } else {
            ("<Int>2</Int><Bool>true</Bool>", "[2,true]")
        };
        check_type(
            &format!("<{operator}>{xml}</{operator}>"),
            &format!(r#"{{"${operator}":{json}}}"#),
            "Edm.Double",
            false,
            false,
        );
    }
}

#[test]
fn logical_operators_require_boolean_operands_and_exact_arity() {
    for operator in ["And", "Or", "Not"] {
        let arity = if operator == "Not" { 1 } else { 2 };
        for count in 0..=3 {
            let xml = "<Bool>true</Bool>".repeat(count);
            let json = vec!["true"; count].join(",");
            let json_expression = if operator == "Not" && count == 1 {
                r#"{"$Not":true}"#.to_owned()
            } else {
                format!(r#"{{"${operator}":[{json}]}}"#)
            };
            check(
                &format!("<{operator}>{xml}</{operator}>"),
                &json_expression,
                false,
                count == arity,
            );
        }
        let (xml, json) = if operator == "Not" {
            ("<Int>1</Int>", "1")
        } else {
            ("<Bool>true</Bool><Int>1</Int>", "[true,1]")
        };
        check(
            &format!("<{operator}>{xml}</{operator}>"),
            &format!(r#"{{"${operator}":{json}}}"#),
            false,
            false,
        );
    }
}

#[test]
fn conditional_requires_boolean_condition_and_collection_context_for_two_operands() {
    for collection in [false, true] {
        for count in 1..=4 {
            let xml = format!("<If>{}</If>", "<Bool>true</Bool>".repeat(count));
            let json = format!(r#"{{"$If":[{}]}}"#, vec!["true"; count].join(","));
            let xml = if collection {
                format!("<Collection>{xml}</Collection>")
            } else {
                xml
            };
            let json = if collection {
                format!("[{json}]")
            } else {
                json
            };
            check(
                &xml,
                &json,
                collection,
                count == 3 || collection && count == 2,
            );
        }
    }
    check(
        "<If><Int>1</Int><Bool>true</Bool><Bool>false</Bool></If>",
        r#"{"$If":[1,true,false]}"#,
        false,
        false,
    );
    check(
        "<If><Bool>true</Bool><Bool>true</Bool><Int>1</Int></If>",
        r#"{"$If":[true,true,1]}"#,
        false,
        false,
    );
}
