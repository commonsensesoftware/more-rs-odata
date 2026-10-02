#![cfg(all(feature = "resolver", feature = "validator"))]
use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

#[test]
fn label_references_preserve_the_defining_context_type_and_multiplicity() {
    for source_collection in [false, true] {
        for target_collection in [false, true] {
            for target_type in ["Edm.Int32", "Edm.String"] {
                let source_type = if source_collection { "Collection(Edm.Int32)" } else { "Edm.Int32" };
                let target_xml_type = if target_collection { format!("Collection({target_type})") } else { target_type.into() };
                let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Definition" Type="{source_type}"/><Term Name="Value" Type="{target_xml_type}"/><ComplexType Name="Source"><Property Name="Data" Type="{source_type}" Nullable="false"/><Annotation Term="N.Definition"><LabeledElement Name="Shared"><Path>Data</Path></LabeledElement></Annotation></ComplexType><ComplexType Name="Destination"><Property Name="Data" Type="Edm.String"/><Annotation Term="N.Value"><LabeledElementReference>N.Shared</LabeledElementReference></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#);
                let json = format!(r#"{{"$Version":"4.01","N":{{"Definition":{{"$Kind":"Term","$Type":"Edm.Int32","$Collection":{source_collection}}},"Value":{{"$Kind":"Term","$Type":"{target_type}","$Collection":{target_collection}}},"Source":{{"$Kind":"ComplexType","Data":{{"$Type":"Edm.Int32","$Collection":{source_collection}}},"@N.Definition":{{"$Name":"Shared","$LabeledElement":{{"$Path":"Data"}}}}}},"Destination":{{"$Kind":"ComplexType","Data":{{"$Nullable":true}},"@N.Value":{{"$LabeledElementReference":"N.Shared"}}}}}}}}"#);
                for parsed in [parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict), parser::from_json_reader_with_profile(json.as_bytes(), P::Strict)] {
                    let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                    validator::validate_document(&model).unwrap();
                    assert_eq!(model.schemas[0].annotations.get().unwrap().len(), 2);
                    let diagnostics = validator::validate_annotations(&model);
                    let valid = source_collection == target_collection && target_type == "Edm.Int32";
                    assert_eq!(diagnostics.is_empty(), valid, "source_collection={source_collection}, target_collection={target_collection}, {target_type}: {diagnostics:?}");
                    assert!(diagnostics.iter().all(|d| d.term == "N.Value"), "{diagnostics:?}");
                }
            }
        }
    }
}

#[test]
fn cyclic_label_references_are_diagnosed_without_recursing_forever() {
    check_type(r#"<LabeledElement Name="Self"><LabeledElementReference>N.Self</LabeledElementReference></LabeledElement>"#, r#"{"$Name":"Self","$LabeledElement":{"$LabeledElementReference":"N.Self"}}"#, "Edm.Int64", false, false);
    check_type(r#"<Collection><LabeledElement Name="First"><LabeledElementReference>N.Second</LabeledElementReference></LabeledElement><LabeledElement Name="Second"><LabeledElementReference>N.First</LabeledElementReference></LabeledElement></Collection>"#, r#"[{"$Name":"First","$LabeledElement":{"$LabeledElementReference":"N.Second"}},{"$Name":"Second","$LabeledElement":{"$LabeledElementReference":"N.First"}}]"#, "Edm.Int64", true, false);
}

#[test]
fn dynamic_collection_items_must_match_the_element_type() {
    for (xml, json, valid) in [
        ("<Add><Int>1</Int><Int>2</Int></Add>", r#"{"$Add":[1,2]}"#, true),
        ("<Eq><Int>1</Int><Int>2</Int></Eq>", r#"{"$Eq":[1,2]}"#, false),
        (r#"<Cast Type="Edm.Int64"><Int>1</Int></Cast>"#, r#"{"$Cast":1,"$Type":"Edm.Int64"}"#, true),
        (r#"<IsOf Type="Edm.Int64"><Int>1</Int></IsOf>"#, r#"{"$IsOf":1,"$Type":"Edm.Int64"}"#, false),
        (r#"<Apply Function="odata.length"><String>abc</String></Apply>"#, r#"{"$Apply":["abc"],"$Function":"odata.length"}"#, true),
        ("<If><Bool>true</Bool><Int>1</Int></If>", r#"{"$If":[true,1]}"#, true),
        ("<If><Bool>false</Bool><Bool>true</Bool></If>", r#"{"$If":[false,true]}"#, false),
        (r#"<LabeledElement Name="Count"><Int>2</Int></LabeledElement><LabeledElementReference>N.Count</LabeledElementReference>"#, r#"{"$Name":"Count","$LabeledElement":2},{"$LabeledElementReference":"N.Count"}"#, true),
        (r#"<LabeledElement Name="Count"><Bool>true</Bool></LabeledElement><LabeledElementReference>N.Count</LabeledElementReference>"#, r#"{"$Name":"Count","$LabeledElement":true},{"$LabeledElementReference":"N.Count"}"#, false),
        (r#"<LabeledElement Name="Count"><If><Bool>true</Bool><Int>2</Int></If></LabeledElement>"#, r#"{"$Name":"Count","$LabeledElement":{"$If":[true,2]}}"#, false),
    ] {
        check_type(&format!("<Collection><Int>1</Int>{xml}</Collection>"), &format!("[1,{json}]"), "Edm.Int64", true, valid);
    }
}

#[test]
fn enum_and_type_definition_collection_items_use_resolved_types() {
    for (ty, xml_value, json_value, valid) in [
        ("N.Choice", "<EnumMember>N.Choice/One</EnumMember>", r#""One""#, true),
        ("N.Choice", "<EnumMember>N.Choice/Missing</EnumMember>", r#""Missing""#, false),
        ("N.Choice", "<Bool>true</Bool>", "true", false),
        ("N.Code", "<String>ok</String>", r#""ok""#, true),
        ("N.Code", "<String>long</String>", r#""long""#, false),
        ("N.Code", "<Bool>true</Bool>", "true", false),
    ] {
        let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><EnumType Name="Choice"><Member Name="One"/></EnumType><TypeDefinition Name="Code" UnderlyingType="Edm.String" MaxLength="2"/><Term Name="Value" Type="Collection({ty})"/><Annotation Term="N.Value"><Collection>{xml_value}</Collection></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#);
        let json = format!(r#"{{"$Version":"4.01","N":{{"Choice":{{"$Kind":"EnumType","One":0}},"Code":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.String","$MaxLength":2}},"Value":{{"$Kind":"Term","$Type":"{ty}","$Collection":true}},"@N.Value":[{json_value}]}}}}"#);
        for parsed in [parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict), parser::from_json_reader_with_profile(json.as_bytes(), P::Strict)] {
            let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
            validator::validate_document(&model).unwrap();
            assert_eq!(model.schemas[0].annotations.get().unwrap().len(), 1);
            let diagnostics = validator::validate_annotations(&model);
            assert_eq!(diagnostics.is_empty(), valid, "{ty}, {xml_value}: {diagnostics:?}");
        }
    }
}

#[test]
fn path_collection_items_require_scalar_compatible_values() {
    for (path, valid) in [("Count", true), ("Text", false), ("Counts", false), ("Missing", false)] {
        let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Collection(Edm.Int32)" Nullable="false"/><ComplexType Name="Owner"><Property Name="Count" Type="Edm.Int32" Nullable="false"/><Property Name="Text" Type="Edm.String"/><Property Name="Counts" Type="Collection(Edm.Int32)" Nullable="false"/><Annotation Term="N.Value"><Collection><Path>{path}</Path></Collection></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#);
        let json = format!(r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"Edm.Int32","$Collection":true}},"Owner":{{"$Kind":"ComplexType","Count":{{"$Type":"Edm.Int32"}},"Text":{{"$Nullable":true}},"Counts":{{"$Type":"Edm.Int32","$Collection":true}},"@N.Value":[{{"$Path":"{path}"}}]}}}}}}"#);
        for parsed in [parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict), parser::from_json_reader_with_profile(json.as_bytes(), P::Strict)] {
            let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
            validator::validate_document(&model).unwrap();
            assert_eq!(model.schemas[0].annotations.get().unwrap().len(), 1);
            let diagnostics = validator::validate_annotations(&model);
            assert_eq!(diagnostics.is_empty(), valid, "{path}: {diagnostics:?}");
        }
    }
}

#[test]
fn null_values_require_nullable_scalar_contexts() {
    for nullable in [false, true] {
        for collection in [false, true] {
            check_type_nullable(
                "<Null/>",
                "null",
                "Edm.String",
                collection,
                nullable,
                nullable && !collection,
            );
            if collection {
                check_type_nullable(
                    "<Collection><Null/></Collection>",
                    "[null]",
                    "Edm.String",
                    true,
                    nullable,
                    nullable,
                );
            }
        }
        check_type_nullable(
            "<String>null</String>",
            r#""null""#,
            "Edm.String",
            false,
            nullable,
            true,
        );
    }
}

#[test]
fn xml_null_requires_element_notation_without_expression_children() {
    for (annotation, valid) in [
        (r#"<Annotation Term="N.Value"><Null/></Annotation>"#, true),
        (
            r#"<Annotation Term="N.Value"><Null></Null></Annotation>"#,
            true,
        ),
        (r#"<Annotation Term="N.Value" Null="true"/>"#, false),
        (r#"<Annotation Term="N.Value" Null=""/>"#, false),
        (
            r#"<Annotation Term="N.Value"><Null><String>x</String></Null></Annotation>"#,
            false,
        ),
    ] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N">{annotation}</Schema></edmx:DataServices></edmx:Edmx>"#
        );
        let parsed = parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict);
        assert_eq!(parsed.is_ok(), valid, "{annotation}: {parsed:?}");
    }
}

#[test]
fn concat_accepts_scalar_enums_and_type_definitions_but_not_structured_values() {
    for item_type in ["N.Color", "N.Code", "N.Detail"] {
        for collection in [false, true] {
            for result_type in ["Edm.String", "Edm.Boolean"] {
                let xml_type = if collection {
                    format!("Collection({item_type})")
                } else {
                    item_type.into()
                };
                let xml = format!(
                    r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><EnumType Name="Color"><Member Name="Red"/></EnumType><TypeDefinition Name="Code" UnderlyingType="Edm.String"/><ComplexType Name="Detail"/><Term Name="Value" Type="{result_type}"/><ComplexType Name="Owner"><Property Name="Input" Type="{xml_type}" Nullable="false"/><Annotation Term="N.Value"><Apply Function="odata.concat"><String>prefix</String><Path>Input</Path></Apply></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
                );
                let json = format!(
                    r#"{{"$Version":"4.01","N":{{"Color":{{"$Kind":"EnumType","Red":0}},"Code":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.String"}},"Detail":{{"$Kind":"ComplexType"}},"Value":{{"$Kind":"Term","$Type":"{result_type}"}},"Owner":{{"$Kind":"ComplexType","Input":{{"$Type":"{item_type}","$Collection":{collection}}},"@N.Value":{{"$Apply":["prefix",{{"$Path":"Input"}}],"$Function":"odata.concat"}}}}}}}}"#
                );
                for parsed in [
                    parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                    parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                ] {
                    let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                    validator::validate_document(&model).unwrap();
                    let diagnostics = validator::validate_annotations(&model);
                    assert_eq!(
                        diagnostics.is_empty(),
                        item_type != "N.Detail" && !collection && result_type == "Edm.String",
                        "{item_type}, collection={collection}, result={result_type}: {diagnostics:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn uri_template_functions_require_string_templates_and_labeled_parameters() {
    for (xml, json, valid) in [
        (
            "<String>https://example.test/{id}</String><LabeledElement Name=\"id\"><Int>1</Int></LabeledElement>",
            r#"["https://example.test/{id}",{"$Name":"id","$LabeledElement":1}]"#,
            true,
        ),
        (
            "<String>https://example.test/</String>",
            r#"["https://example.test/"]"#,
            false,
        ),
        ("", "[]", false),
        (
            "<Int>1</Int><LabeledElement Name=\"id\"><Int>1</Int></LabeledElement>",
            r#"[1,{"$Name":"id","$LabeledElement":1}]"#,
            false,
        ),
        (
            "<String>https://example.test/{id}</String><Int>1</Int>",
            r#"["https://example.test/{id}",1]"#,
            false,
        ),
        (
            "<String>https://example.test/{id}</String><LabeledElement Name=\"id\"><Int>1</Int></LabeledElement><Int>2</Int>",
            r#"["https://example.test/{id}",{"$Name":"id","$LabeledElement":1},2]"#,
            false,
        ),
    ] {
        check_type(
            &format!(r#"<Apply Function="odata.fillUriTemplate">{xml}</Apply>"#),
            &format!(r#"{{"$Apply":{json},"$Function":"odata.fillUriTemplate"}}"#),
            "Edm.String",
            false,
            valid,
        );
    }
}

#[test]
fn apply_concat_requires_at_least_two_scalar_arguments() {
    for count in 0..=3 {
        let xml = format!(
            r#"<Apply Function="odata.concat">{}</Apply>"#,
            "<String>a</String>".repeat(count)
        );
        let json = format!(
            r#"{{"$Apply":[{}],"$Function":"odata.concat"}}"#,
            vec![r#""a""#; count].join(",")
        );
        check_type(&xml, &json, "Edm.String", false, count >= 2);
    }
    for (xml, json, valid) in [
        ("<String>a</String><Int>1</Int>", r#"["a",1]"#, true),
        ("<String>a</String><Bool>true</Bool>", r#"["a",true]"#, true),
        (
            "<String>a</String><Collection><String>b</String></Collection>",
            r#"["a",["b"]]"#,
            false,
        ),
        ("<String>a</String><Record/>", r#"["a",{}]"#, false),
    ] {
        check_type(
            &format!(r#"<Apply Function="odata.concat">{xml}</Apply>"#),
            &format!(r#"{{"$Apply":{json},"$Function":"odata.concat"}}"#),
            "Edm.String",
            false,
            valid,
        );
    }
}

#[test]
fn apply_requires_function_reference_and_correct_builtin_signature() {
    check_type(
        r#"<Apply Function="odata.length"><String>abc</String></Apply>"#,
        r#"{"$Apply":["abc"],"$Function":"odata.length"}"#,
        "Edm.Int32",
        false,
        true,
    );
    for (xml, json) in [
        (
            r#"<Apply><String>abc</String></Apply>"#,
            r#"{"$Apply":["abc"]}"#,
        ),
        (
            r#"<Apply Function="odata.length"><Int>1</Int></Apply>"#,
            r#"{"$Apply":[1],"$Function":"odata.length"}"#,
        ),
        (
            r#"<Apply Function="odata.length"><String>a</String><String>b</String></Apply>"#,
            r#"{"$Apply":["a","b"],"$Function":"odata.length"}"#,
        ),
        (
            r#"<Apply Function="odata.unknown"><String>a</String></Apply>"#,
            r#"{"$Apply":["a"],"$Function":"odata.unknown"}"#,
        ),
    ] {
        check_type(xml, json, "Edm.Int32", false, false);
    }
}

#[test]
fn label_uniqueness_spans_annotations_but_not_schema_boundaries() {
    for separate_schema in [false, true] {
        for second_name in ["Flag", "Other"] {
            let xml_second = format!(
                r#"<ComplexType Name="Second"><Annotation Term="N.Value"><LabeledElement Name="{second_name}" Bool="false"/></Annotation></ComplexType>"#
            );
            let json_second = format!(
                r#""Second":{{"$Kind":"ComplexType","@N.Value":{{"$Name":"{second_name}","$LabeledElement":false}}}}"#
            );
            let (xml_tail, json_tail) = if separate_schema {
                (
                    format!(
                        r#"</Schema><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="M">{xml_second}</Schema>"#
                    ),
                    format!(r#"}},"M":{{{json_second}}}"#),
                )
            } else {
                (
                    format!("{xml_second}</Schema>"),
                    format!(",{json_second}}}"),
                )
            };
            let xml = format!(
                r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.Boolean"/><ComplexType Name="First"><Annotation Term="N.Value"><LabeledElement Name="Flag" Bool="true"/></Annotation></ComplexType>{xml_tail}</edmx:DataServices></edmx:Edmx>"#
            );
            let json = format!(
                r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"Edm.Boolean"}},"First":{{"$Kind":"ComplexType","@N.Value":{{"$Name":"Flag","$LabeledElement":true}}}}{json_tail}}}"#
            );
            for parsed in [
                parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
            ] {
                let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                validator::validate_document(&model).unwrap();
                let diagnostics = validator::validate_annotations(&model);
                assert_eq!(
                    diagnostics.is_empty(),
                    separate_schema || second_name != "Flag",
                    "{diagnostics:?}"
                );
            }
        }
    }
}

#[test]
fn labels_require_identifiers_and_references_must_resolve_in_scope() {
    for (name, reference, valid) in [
        ("Flag", "N.Flag", true),
        ("Flag", "N.Missing", false),
        ("Flag", "Missing.Flag", false),
        ("Flag", "Flag", false),
        ("1Invalid", "N.1Invalid", false),
        ("", "N.Empty", false),
    ] {
        check(
            &format!(
                r#"<Collection><LabeledElement Name="{name}"><Bool>true</Bool></LabeledElement><LabeledElementReference>{reference}</LabeledElementReference></Collection>"#
            ),
            &format!(
                r#"[{{"$Name":"{name}","$LabeledElement":true}},{{"$LabeledElementReference":"{reference}"}}]"#
            ),
            true,
            valid,
        );
    }
}

#[test]
fn label_names_are_unique_within_the_schema() {
    for (second, valid) in [("Other", true), ("Flag", false)] {
        check(
            &format!(
                r#"<Collection><LabeledElement Name="Flag"><Bool>true</Bool></LabeledElement><LabeledElement Name="{second}"><Bool>false</Bool></LabeledElement></Collection>"#
            ),
            &format!(
                r#"[{{"$Name":"Flag","$LabeledElement":true}},{{"$Name":"{second}","$LabeledElement":false}}]"#
            ),
            true,
            valid,
        );
    }
}

#[test]
fn labels_require_names_and_one_expression() {
    check(
        r#"<LabeledElement Name="Flag" Bool="true"/>"#,
        r#"{"$Name":"Flag","$LabeledElement":true}"#,
        false,
        true,
    );
    check(
        r#"<LabeledElement><Bool>true</Bool></LabeledElement>"#,
        r#"{"$LabeledElement":true}"#,
        false,
        false,
    );
    for body in ["", "<Bool>true</Bool><Bool>false</Bool>"] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.Boolean"/><ComplexType Name="Owner"><Annotation Term="N.Value"><LabeledElement Name="Flag">{body}</LabeledElement></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        assert!(parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict).is_err());
    }
}

#[test]
fn structured_collection_items_must_derive_from_the_expected_type() {
  for entity in [false, true] {
    for (item_type, valid) in [
        ("N.Base", true),
        ("N.Derived", true),
        ("N.Other", false),
        ("Missing.Type", false),
    ] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Base"/><ComplexType Name="Derived" BaseType="N.Base"/><ComplexType Name="Other"/><Term Name="Value" Type="Collection(N.Base)" Nullable="false"/><ComplexType Name="Owner"><Annotation Term="N.Value"><Collection><Record Type="N.Base"/><Record Type="{item_type}"/></Collection></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        let json = format!(
            r##"{{"$Version":"4.01","N":{{"Base":{{"$Kind":"ComplexType"}},"Derived":{{"$Kind":"ComplexType","$BaseType":"N.Base"}},"Other":{{"$Kind":"ComplexType"}},"Value":{{"$Kind":"Term","$Type":"N.Base","$Collection":true}},"Owner":{{"$Kind":"ComplexType","@N.Value":[{{"@type":"#N.Base"}},{{"@type":"#{item_type}"}}]}}}}}}"##
        );
        let mut xml = xml;
        let mut json = json;
        if entity {
            for name in ["Base", "Derived", "Other"] {
                xml = xml.replace(&format!("<ComplexType Name=\"{name}\""), &format!("<EntityType Name=\"{name}\""));
                json = json.replace(&format!(r#""{name}":{{"$Kind":"ComplexType""#), &format!(r#""{name}":{{"$Kind":"EntityType""#));
            }
            for name in ["Base", "Other"] {
                xml = xml.replace(&format!(r#"<EntityType Name="{name}"/>"#), &format!(r#"<EntityType Name="{name}"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType>"#));
                json = json.replace(&format!(r#""{name}":{{"$Kind":"EntityType"}}"#), &format!(r#""{name}":{{"$Kind":"EntityType","$Key":["ID"],"ID":{{"$Type":"Edm.Int32"}}}}"#));
            }
            for name in ["N.Base", item_type] {
                xml = xml.replace(&format!(r#"<Record Type="{name}"/>"#), &format!(r#"<Record Type="{name}"><PropertyValue Property="ID" Int="1"/></Record>"#));
                json = json.replace(&format!(r##"{{"@type":"#{name}"}}"##), &format!(r##"{{"@type":"#{name}","ID":1}}"##));
            }
        }
        for parsed in [
            parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
            parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
        ] {
            let result = parsed
                .map_err(|e| e.to_string())
                .and_then(|d| {
                    Resolver::resolve_document(d.edmx.unwrap()).map_err(|e| format!("{e:?}"))
                })
                .and_then(|m| {
                    validator::validate_document(&m).map_err(|e| format!("{e:?}"))?;
                    let diagnostics = validator::validate_annotations(&m);
                    if diagnostics.is_empty() {
                        Ok(())
                    } else {
                        Err(format!("{diagnostics:?}"))
                    }
                });
            assert_eq!(result.is_ok(), valid, "{result:?}: {item_type}");
        }
    }
  }
}

#[test]
fn collection_items_must_be_compatible_with_the_expected_element_type() {
    for (xml, json, ty, valid) in [
        ("<Collection/>", "[]", "Edm.Boolean", true),
        (
            "<Collection><Bool>true</Bool></Collection>",
            "[true]",
            "Edm.Boolean",
            true,
        ),
        (
            "<Collection><Bool>true</Bool><Bool>false</Bool></Collection>",
            "[true,false]",
            "Edm.Boolean",
            true,
        ),
        (
            "<Collection><Int>1</Int><Float>2.5</Float></Collection>",
            "[1,2.5]",
            "Edm.Double",
            true,
        ),
        (
            "<Collection><Bool>true</Bool><Int>1</Int></Collection>",
            "[true,1]",
            "Edm.Boolean",
            false,
        ),
        (
            "<Collection><Int>1</Int><String>a</String></Collection>",
            r#"[1,"a"]"#,
            "Edm.Double",
            false,
        ),
        (
            "<Collection><Collection><Bool>true</Bool></Collection></Collection>",
            "[[true]]",
            "Edm.Boolean",
            false,
        ),
        (
            "<Collection><Null/></Collection>",
            "[null]",
            "Edm.Boolean",
            false,
        ),
        ("<Bool>true</Bool>", "true", "Edm.Boolean", false),
    ] {
        check_type(xml, json, ty, true, valid);
    }
    check(
        "<Collection><Bool>true</Bool></Collection>",
        "[true]",
        false,
        false,
    );
}

#[test]
fn cast_and_isof_require_one_operand_and_resolved_target_type() {
    for operator in ["Cast", "IsOf"] {
        let result_type = if operator == "Cast" {
            "Edm.Int64"
        } else {
            "Edm.Boolean"
        };
        for count in 0..=3 {
            let xml = format!(
                r#"<{operator} Type="Edm.Int64">{}</{operator}>"#,
                "<Int>1</Int>".repeat(count)
            );
            let xml = if count == 3 {
                format!("<{operator}><Int>1</Int></{operator}>")
            } else {
                xml
            };
            if count == 1 {
                check_type(
                    &xml,
                    &format!(r#"{{"${operator}":1,"$Type":"Edm.Int64"}}"#),
                    result_type,
                    false,
                    true,
                );
            } else {
                let document = format!(
                    r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.Boolean"/><ComplexType Name="Owner"><Annotation Term="N.Value">{xml}</Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
                );
                assert!(
                    parser::from_xml_reader_with_profile(document.as_bytes(), P::Strict).is_err()
                );
            }
        }
        check_type(
            &format!(r#"<{operator} Type="Missing.Type"><Int>1</Int></{operator}>"#),
            &format!(r#"{{"${operator}":1,"$Type":"Missing.Type"}}"#),
            result_type,
            false,
            false,
        );
    }
}

#[test]
fn has_requires_two_operands_of_the_same_flags_enum() {
    for flags in [false, true] {
        for (right_type, collection, compatible) in [
            ("N.Choice", false, true),
            ("N.Other", false, false),
            ("Edm.Int32", false, false),
            ("N.Choice", true, false),
        ] {
            for count in 0..=3 {
                let operands = ["Left", "Right", "Left"];
                let xml_operands = operands[..count]
                    .iter()
                    .map(|p| format!("<Path>{p}</Path>"))
                    .collect::<String>();
                let json_operands = operands[..count]
                    .iter()
                    .map(|p| format!(r#"{{"$Path":"{p}"}}"#))
                    .collect::<Vec<_>>()
                    .join(",");
                let xml_type = if collection {
                    format!("Collection({right_type})")
                } else {
                    right_type.to_owned()
                };
                let xml = format!(
                    r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><EnumType Name="Choice" IsFlags="{flags}"><Member Name="One" Value="1"/></EnumType><EnumType Name="Other" IsFlags="true"><Member Name="One" Value="1"/></EnumType><Term Name="Value" Type="Edm.Boolean"/><ComplexType Name="Owner"><Property Name="Left" Type="N.Choice" Nullable="false"/><Property Name="Right" Type="{xml_type}" Nullable="false"/><Annotation Term="N.Value"><Has>{xml_operands}</Has></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
                );
                let json = format!(
                    r#"{{"$Version":"4.01","N":{{"Choice":{{"$Kind":"EnumType","$IsFlags":{flags},"One":1}},"Other":{{"$Kind":"EnumType","$IsFlags":true,"One":1}},"Value":{{"$Kind":"Term","$Type":"Edm.Boolean"}},"Owner":{{"$Kind":"ComplexType","Left":{{"$Type":"N.Choice"}},"Right":{{"$Type":"{right_type}","$Collection":{collection}}},"@N.Value":{{"$Has":[{json_operands}]}}}}}}}}"#
                );
                for parsed in [
                    parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                    parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                ] {
                    let result = parsed
                        .map_err(|e| e.to_string())
                        .and_then(|d| {
                            Resolver::resolve_document(d.edmx.unwrap())
                                .map_err(|e| format!("{e:?}"))
                        })
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
                        flags && compatible && count == 2,
                        "{result:?}: flags={flags}, right={xml_type}, operands={count}"
                    );
                }
            }
        }
    }
}

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
    check_type_nullable(
        xml_expression,
        json_expression,
        item_type,
        collection,
        false,
        valid,
    );
}

fn check_type_nullable(
    xml_expression: &str,
    json_expression: &str,
    item_type: &str,
    collection: bool,
    nullable: bool,
    valid: bool,
) {
    let ty = if collection {
        format!("Collection({item_type})")
    } else {
        item_type.to_owned()
    };
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{ty}" Nullable="{nullable}"/><ComplexType Name="Owner"><Annotation Term="N.Value">{xml_expression}</Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let json = format!(
        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"{item_type}","$Collection":{collection},"$Nullable":{nullable}}},"Owner":{{"$Kind":"ComplexType","@N.Value":{json_expression}}}}}}}"#
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
        for count in 0..=4 {
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
    let source = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Annotation Term="N.Value"><If Bool="true"><Bool>true</Bool><Bool>false</Bool></If></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#;
    assert!(parser::from_xml_reader_with_profile(source.as_bytes(), P::Strict).is_err());
}

#[test]
fn conditional_record_branches_require_compatible_structured_types() {
    for condition in ["true", "false"] {
        for (ty, valid) in [("N.Base", true), ("N.Derived", true), ("N.Other", false), ("N.Missing", false)] {
            for then_branch in [false, true] {
                let (left, right) = if then_branch { (ty, "N.Base") } else { ("N.Base", ty) };
                let xml = format!(r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="Base"/><ComplexType Name="Derived" BaseType="N.Base"/><ComplexType Name="Other"/><Term Name="Value" Type="N.Base"/><Annotation Term="N.Value"><If><Bool>{condition}</Bool><Record Type="{left}"/><Record Type="{right}"/></If></Annotation></Schema></edmx:DataServices></edmx:Edmx>"#);
                let json = format!(r#"{{"$Version":"4.01","N":{{"Base":{{"$Kind":"ComplexType"}},"Derived":{{"$Kind":"ComplexType","$BaseType":"N.Base"}},"Other":{{"$Kind":"ComplexType"}},"Value":{{"$Kind":"Term","$Type":"N.Base"}},"@N.Value":{{"$If":[{condition},{{"@type":"{left}"}},{{"@type":"{right}"}}]}}}}}}"#);
                for parsed in [
                    parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                    parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                ] {
                    let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                    validator::validate_document(&model).unwrap();
                    let diagnostics = validator::validate_annotations(&model);
                    assert_eq!(diagnostics.is_empty(), valid, "{condition}, {left}, {right}: {diagnostics:?}");
                }
            }
        }
    }
}

#[test]
fn conditional_branches_both_match_the_surrounding_type() {
    for condition in ["true", "false"] {
        for (left_xml, left_json, right_xml, right_json, ty, collection, valid) in [
            ("<Int>1</Int>", "1", "<Float>2.5</Float>", "2.5", "Edm.Double", false, true),
            ("<String>a</String>", r#""a""#, "<String>b</String>", r#""b""#, "Edm.String", false, true),
            ("<String>a</String>", r#""a""#, "<Int>1</Int>", "1", "Edm.String", false, false),
            ("<Int>1</Int>", "1", "<String>a</String>", r#""a""#, "Edm.String", false, false),
            ("<Collection><Int>1</Int></Collection>", "[1]", "<Collection/>", "[]", "Edm.Int64", true, true),
            ("<Collection><Int>1</Int></Collection>", "[1]", "<Int>2</Int>", "2", "Edm.Int64", true, false),
            ("<Int>1</Int>", "1", "<Collection><Int>2</Int></Collection>", "[2]", "Edm.Int64", false, false),
            ("<Collection><Int>1</Int></Collection>", "[1]", "<Collection><String>a</String></Collection>", r#"["a"]"#, "Edm.Int64", true, false),
        ] {
            check_type(
                &format!("<If><Bool>{condition}</Bool>{left_xml}{right_xml}</If>"),
                &format!(r#"{{"$If":[{condition},{left_json},{right_json}]}}"#),
                ty, collection, valid,
            );
        }
        for nullable in [false, true] {
            for (left_xml, left_json, right_xml, right_json) in [
                ("<Null/>", "null", "<String>a</String>", r#""a""#),
                ("<String>a</String>", r#""a""#, "<Null/>", "null"),
            ] {
                check_type_nullable(
                    &format!("<If><Bool>{condition}</Bool>{left_xml}{right_xml}</If>"),
                    &format!(r#"{{"$If":[{condition},{left_json},{right_json}]}}"#),
                    "Edm.String", false, nullable, nullable,
                );
            }
        }
    }
}
