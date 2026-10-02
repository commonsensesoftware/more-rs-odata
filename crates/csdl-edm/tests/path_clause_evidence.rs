#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::{conformance::ValidationProfile as P, parser, resolver::Resolver, validator};

#[test]
fn model_path_endpoints_match_the_declared_path_kind() {
    for collection in [false, true] {
        let xml_type = |ty: &str| {
            if collection {
                format!("Collection({ty})")
            } else {
                ty.to_owned()
            }
        };
        for expression in ["PropertyPath", "NavigationPropertyPath", "AnnotationPath"] {
            for (path, property, navigation, annotation) in [
                ("Text", true, false, false),
                ("Detail", true, false, false),
                ("Choice", true, false, false),
                ("Code", true, false, false),
                ("Nav", false, true, false),
                ("@N.Label", true, false, true),
                ("@N.Related", false, true, true),
                ("Missing", false, false, false),
            ] {
                let text_type = xml_type("Edm.String");
                let detail_type = xml_type("N.DetailType");
                let choice_type = xml_type("N.ChoiceType");
                let code_type = xml_type("N.CodeType");
                let entity_type = xml_type("N.Entity");
                let xml = format!(
                    r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><ComplexType Name="DetailType"/><EnumType Name="ChoiceType"><Member Name="One"/></EnumType><TypeDefinition Name="CodeType" UnderlyingType="Edm.String"/><EntityType Name="Entity" Abstract="true"/><Term Name="Value" Type="Edm.{expression}"/><Term Name="Label" Type="{text_type}" Nullable="false"/><Term Name="Related" Type="{entity_type}"/><ComplexType Name="Owner"><Property Name="Text" Type="{text_type}" Nullable="false"/><Property Name="Detail" Type="{detail_type}" Nullable="false"/><Property Name="Choice" Type="{choice_type}" Nullable="false"/><Property Name="Code" Type="{code_type}" Nullable="false"/><NavigationProperty Name="Nav" Type="{entity_type}"/><Annotation Term="N.Value"><{expression}>{path}</{expression}></Annotation></ComplexType></Schema></edmx:DataServices></edmx:Edmx>"#
                );
                let json = format!(
                    r#"{{"$Version":"4.01","N":{{"DetailType":{{"$Kind":"ComplexType"}},"ChoiceType":{{"$Kind":"EnumType","One":0}},"CodeType":{{"$Kind":"TypeDefinition","$UnderlyingType":"Edm.String"}},"Entity":{{"$Kind":"EntityType","$Abstract":true}},"Value":{{"$Kind":"Term","$Type":"Edm.{expression}"}},"Label":{{"$Kind":"Term","$Collection":{collection}}},"Related":{{"$Kind":"Term","$Type":"N.Entity","$Collection":{collection}}},"Owner":{{"$Kind":"ComplexType","Text":{{"$Collection":{collection}}},"Detail":{{"$Type":"N.DetailType","$Collection":{collection}}},"Choice":{{"$Type":"N.ChoiceType","$Collection":{collection}}},"Code":{{"$Type":"N.CodeType","$Collection":{collection}}},"Nav":{{"$Kind":"NavigationProperty","$Type":"N.Entity","$Collection":{collection}}},"@N.Value":{{"${expression}":"{path}"}}}}}}}}"#
                );
                for parsed in [
                    parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                    parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                ] {
                    let model = Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                    validator::validate_document(&model).unwrap();
                    assert_eq!(model.schemas[0].annotations.get().unwrap().len(), 1);
                    let diagnostics = validator::validate_annotations(&model);
                    let valid = match expression {
                        "PropertyPath" => property,
                        "NavigationPropertyPath" => navigation,
                        _ => annotation,
                    };
                    assert_eq!(
                        diagnostics.is_empty(),
                        valid,
                        "{expression} {path} collection={collection}: {diagnostics:?}"
                    );
                    assert_eq!(
                        diagnostics
                            .iter()
                            .any(|d| d.issue == validator::AnnotationIssue::InvalidExpressionPath),
                        !valid,
                        "{diagnostics:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn structured_type_paths_start_at_the_annotated_type() {
    use csdl_edm::edm::{AnnotationTarget as T, SchemaElement as S};
    for entity in [false, true] {
        let kind = if entity { "EntityType" } else { "ComplexType" };
        let abstract_xml = if entity { r#" Abstract="true""# } else { "" };
        let abstract_json = if entity { r#", "$Abstract":true"# } else { "" };
        for external in [false, true] {
            for expression in [
                "Path",
                "PropertyPath",
                "NavigationPropertyPath",
                "AnnotationPath",
                "ModelElementPath",
            ] {
                for (path, resolves, property_endpoint, navigation_endpoint, annotation_endpoint) in [
                    ("", true, !entity, entity, false),
                    ("Name", true, true, false, false),
                    ("/N.Owner/Name", true, true, false, false),
                    ("/N.Owner", true, !entity, entity, false),
                    ("/Owner/Name", false, false, false, false),
                    ("/N", false, false, false, false),
                    ("/", false, false, false, false),
                    ("/N.Owner//Name", false, false, false, false),
                    ("Name/", false, false, false, false),
                    ("Nav//Name", false, false, false, false),
                    ("Nav", true, false, true, false),
                    ("Nav/Name", true, true, false, false),
                    ("N.Derived/Extra", true, true, false, false),
                    ("@N.Text", true, true, false, true),
                    ("Missing", false, false, false, false),
                    ("N.Missing/Extra", false, false, false, false),
                    ("@N.Missing", false, false, false, false),
                ] {
                    let ax = format!(
                        r#"<Annotation Term="N.Value"><{expression}>{path}</{expression}></Annotation>"#
                    );
                    let aj = format!(r#", "@N.Value":{{"${expression}":"{path}"}}"#);
                    let (ix, ij, gx, gj) = if external {
                        (
                            String::new(),
                            String::new(),
                            format!(r#"<Annotations Target="N.Owner">{ax}</Annotations>"#),
                            format!(
                                r#", "$Annotations":{{"N.Owner":{{"@N.Value":{{"${expression}":"{path}"}}}}}}"#
                            ),
                        )
                    } else {
                        (ax, aj, String::new(), String::new())
                    };
                    let term_type = if expression == "Path" {
                        "Edm.Untyped".into()
                    } else {
                        format!("Edm.{expression}")
                    };
                    let xml = format!(
                        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{term_type}"/><Term Name="Text" Type="Edm.String"/><EntityType Name="Target" Abstract="true"><Property Name="Name" Type="Edm.String"/></EntityType><{kind} Name="Owner"{abstract_xml}><Property Name="Name" Type="Edm.String"/><NavigationProperty Name="Nav" Type="N.Target"/>{ix}</{kind}><{kind} Name="Derived" BaseType="N.Owner"{abstract_xml}><Property Name="Extra" Type="Edm.String"/></{kind}>{gx}</Schema></edmx:DataServices></edmx:Edmx>"#
                    );
                    let json = format!(
                        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"{term_type}"}},"Text":{{"$Kind":"Term"}},"Target":{{"$Kind":"EntityType","$Abstract":true,"Name":{{"$Nullable":true}}}},"Owner":{{"$Kind":"{kind}"{abstract_json},"Name":{{"$Nullable":true}},"Nav":{{"$Kind":"NavigationProperty","$Type":"N.Target","$Nullable":true}}{ij}}},"Derived":{{"$Kind":"{kind}"{abstract_json},"$BaseType":"N.Owner","Extra":{{"$Nullable":true}}}}{gj}}}}}"#
                    );
                    for parsed in [
                        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                    ] {
                        let model =
                            Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                        validator::validate_document(&model).unwrap();
                        let annotations = model.schemas[0].annotations.get().unwrap();
                        assert_eq!(annotations.len(), 1);
                        assert_eq!(annotations[0].expression_paths.len(), 1);
                        let resolved = &annotations[0].expression_paths[0];
                        assert_eq!(
                            !resolved
                                .segments
                                .iter()
                                .any(|s| matches!(s, T::Unresolved(_))),
                            resolves,
                            "{expression} {path}: {resolved:?}"
                        );
                        if path.is_empty() {
                            assert!(
                                matches!(resolved.segments.last(), Some(T::Element(e)) if match e.as_ref() { S::EntityType(t) => entity && t.name == "Owner", S::ComplexType(t) => !entity && t.name == "Owner", _ => false })
                            );
                        }
                        let valid = resolves
                            && match expression {
                                "PropertyPath" => property_endpoint,
                                "NavigationPropertyPath" => navigation_endpoint,
                                "AnnotationPath" => annotation_endpoint,
                                _ => true,
                            };
                        let diagnostics = validator::validate_annotations(&model);
                        assert_eq!(
                            diagnostics
                                .iter()
                                .any(|d| d.issue
                                    == validator::AnnotationIssue::InvalidExpressionPath),
                            !valid,
                            "{kind} {expression} {path}: {diagnostics:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn container_and_resource_paths_use_their_declared_starting_context() {
    use csdl_edm::edm::{AnnotationTarget as T, EntityContainerElement as C};
    for expression in [
        "Path",
        "PropertyPath",
        "NavigationPropertyPath",
        "AnnotationPath",
        "ModelElementPath",
    ] {
        for location in ["container", "set", "singleton"] {
            for external in [false, true] {
                let paths = if location == "container" {
                    vec![
                        ("", true),
                        ("Items/Name", true),
                        ("Items/Nav", true),
                        ("Items/@N.Text", true),
                        ("Items(1)/Name", true),
                        ("One/Name", true),
                        ("Call/p/Name", true),
                        ("Do/$ReturnType/Name", true),
                        ("Name", false),
                        ("Missing/Name", false),
                        ("@N.Text", false),
                        ("N.E", false),
                    ]
                } else {
                    vec![
                        ("", true),
                        ("Name", true),
                        ("Nav/Name", true),
                        ("Nav", true),
                        ("N.Derived/Extra", true),
                        ("@N.Text", true),
                        ("Items/Name", false),
                        ("Missing", false),
                    ]
                };
                for (path, valid) in paths {
                    let valid = valid && (expression == "Path" || !path.contains('('));
                    let endpoint_valid = match expression {
                        "PropertyPath" => {
                            path.ends_with("Name")
                                || path.ends_with("Extra")
                                || path.ends_with("@N.Text")
                        }
                        "NavigationPropertyPath" => {
                            path.ends_with("Nav") || path.is_empty() && location != "container"
                        }
                        "AnnotationPath" => path.ends_with("@N.Text"),
                        _ => true,
                    };
                    let ax = format!(
                        r#"<Annotation Term="N.Value"><{expression}>{path}</{expression}></Annotation>"#
                    );
                    let aj = format!(r#", "@N.Value":{{"${expression}":"{path}"}}"#);
                    let inline = |place: &str| {
                        if !external && location == place {
                            (ax.as_str(), aj.as_str())
                        } else {
                            ("", "")
                        }
                    };
                    let (cx, cj) = inline("container");
                    let (sx, sj) = inline("set");
                    let (ox, oj) = inline("singleton");
                    let target = match location {
                        "container" => "N.C",
                        "set" => "N.C/Items",
                        _ => "N.C/One",
                    };
                    let gx = if external {
                        format!(r#"<Annotations Target="{target}">{ax}</Annotations>"#)
                    } else {
                        String::new()
                    };
                    let gj = if external {
                        format!(
                            r#", "$Annotations":{{"{target}":{{"@N.Value":{{"${expression}":"{path}"}}}}}}"#
                        )
                    } else {
                        String::new()
                    };
                    let term_type = if expression == "Path" {
                        "Edm.Untyped".to_owned()
                    } else {
                        format!("Edm.{expression}")
                    };
                    let xml = format!(
                        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{term_type}"/><Term Name="Text" Type="Edm.String"/><EntityType Name="E"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/><Property Name="Name" Type="Edm.String"/><NavigationProperty Name="Nav" Type="N.E"/></EntityType><EntityType Name="Derived" BaseType="N.E"><Property Name="Extra" Type="Edm.String"/></EntityType><Function Name="F"><Parameter Name="p" Type="N.E"/><ReturnType Type="N.E"/></Function><Action Name="A"><ReturnType Type="N.E"/></Action><EntityContainer Name="C"><EntitySet Name="Items" EntityType="N.E">{sx}</EntitySet><Singleton Name="One" Type="N.E">{ox}</Singleton><FunctionImport Name="Call" Function="N.F"/><ActionImport Name="Do" Action="N.A"/>{cx}</EntityContainer>{gx}</Schema></edmx:DataServices></edmx:Edmx>"#
                    );
                    let json = format!(
                        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"{term_type}"}},"Text":{{"$Kind":"Term"}},"E":{{"$Kind":"EntityType","$Key":["ID"],"ID":{{"$Type":"Edm.Int32"}},"Name":{{"$Nullable":true}},"Nav":{{"$Kind":"NavigationProperty","$Type":"N.E","$Nullable":true}}}},"Derived":{{"$Kind":"EntityType","$BaseType":"N.E","Extra":{{"$Nullable":true}}}},"F":[{{"$Kind":"Function","$Parameter":[{{"$Name":"p","$Type":"N.E","$Nullable":true}}],"$ReturnType":{{"$Type":"N.E","$Nullable":true}}}}],"A":[{{"$Kind":"Action","$ReturnType":{{"$Type":"N.E","$Nullable":true}}}}],"C":{{"$Kind":"EntityContainer","Items":{{"$Type":"N.E","$Collection":true{sj}}},"One":{{"$Type":"N.E"{oj}}},"Call":{{"$Function":"N.F"}},"Do":{{"$Action":"N.A"}}{cj}}}{gj}}}}}"#
                    );
                    for parsed in [
                        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                    ] {
                        let model =
                            Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                        validator::validate_document(&model).unwrap();
                        let annotations = model.schemas[0].annotations.get().unwrap();
                        assert_eq!(annotations.len(), 1);
                        let paths = &annotations[0].expression_paths;
                        assert_eq!(paths.len(), 1);
                        let resolved = &paths[0];
                        assert_eq!(
                            !resolved
                                .segments
                                .iter()
                                .any(|s| matches!(s, T::Unresolved(_))),
                            valid,
                            "{location}, external={external}, {path}: {resolved:?}"
                        );
                        if path.is_empty() {
                            let last = resolved.segments.last().unwrap();
                            assert!(
                                match location {
                                    "container" => matches!(last, T::Container(_)),
                                    "set" =>
                                        matches!(last, T::Resource(r) if matches!(r.as_ref(), C::EntitySet(_))),
                                    _ =>
                                        matches!(last, T::Resource(r) if matches!(r.as_ref(), C::Singleton(_))),
                                },
                                "{last:?}"
                            );
                        }
                        assert_eq!(
                            validator::validate_annotations(&model)
                                .iter()
                                .any(|d| d.issue
                                    == validator::AnnotationIssue::InvalidExpressionPath),
                            !(valid && endpoint_valid)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn operation_paths_start_with_a_parameter_or_return_type() {
    for operation in ["Action", "Function"] {
        for location in ["operation", "parameter", "return", "import"] {
            for external in [false, true] {
                for (path, valid) in [
                    ("p/Name", true),
                    ("$ReturnType/Name", true),
                    ("Name", false),
                    ("missing/Name", false),
                    ("", false),
                ] {
                    let ax =
                        format!(r#"<Annotation Term="N.Value"><Path>{path}</Path></Annotation>"#);
                    let aj = format!(r#", "@N.Value":{{"$Path":"{path}"}}"#);
                    let inline = |place: &str| {
                        if !external && location == place {
                            (ax.as_str(), aj.as_str())
                        } else {
                            ("", "")
                        }
                    };
                    let (ox, oj) = inline("operation");
                    let (px, pj) = inline("parameter");
                    let (rx, rj) = inline("return");
                    let (ix, ij) = inline("import");
                    let target = match location {
                        "operation" => "N.F",
                        "parameter" => "N.F/p",
                        "return" => "N.F/$ReturnType",
                        _ => "N.C/Call",
                    };
                    let gx = if external {
                        format!(r#"<Annotations Target="{target}">{ax}</Annotations>"#)
                    } else {
                        String::new()
                    };
                    let gj = if external {
                        format!(
                            r#", "$Annotations":{{"{target}":{{"@N.Value":{{"$Path":"{path}"}}}}}}"#
                        )
                    } else {
                        String::new()
                    };
                    let xml = format!(
                        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="Edm.String"/><ComplexType Name="Data"><Property Name="Name" Type="Edm.String"/></ComplexType><{operation} Name="F"><Parameter Name="p" Type="N.Data">{px}</Parameter><ReturnType Type="N.Data">{rx}</ReturnType>{ox}</{operation}><EntityContainer Name="C"><{operation}Import Name="Call" {operation}="N.F">{ix}</{operation}Import></EntityContainer>{gx}</Schema></edmx:DataServices></edmx:Edmx>"#
                    );
                    let json = format!(
                        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term"}},"Data":{{"$Kind":"ComplexType","Name":{{"$Nullable":true}}}},"F":[{{"$Kind":"{operation}","$Parameter":[{{"$Name":"p","$Type":"N.Data","$Nullable":true{pj}}}],"$ReturnType":{{"$Type":"N.Data","$Nullable":true{rj}}}{oj}}}],"C":{{"$Kind":"EntityContainer","Call":{{"${operation}":"N.F"{ij}}}}}{gj}}}}}"#
                    );
                    for parsed in [
                        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                    ] {
                        let model =
                            Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                        validator::validate_document(&model).unwrap();
                        assert_eq!(
                            model.schemas[0].annotations.get().unwrap().len(),
                            1,
                            "{model:?}"
                        );
                        let diagnostics = validator::validate_annotations(&model);
                        assert_eq!(
                            diagnostics.is_empty(),
                            valid,
                            "{operation} {location} external={external}, {path}: {diagnostics:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn property_path_context_depends_on_embedded_or_external_annotation() {
    for entity in [false, true] {
        for navigation in [false, true] {
            for external in [false, true] {
                for (path, ty, embedded_valid, external_valid) in [
                    (
                        if navigation { "Local/Name" } else { "Local" },
                        "Edm.String",
                        true,
                        false,
                    ),
                    ("RootValue", "Edm.String", false, true),
                    (
                        if navigation {
                            "Nested/Local/Name"
                        } else {
                            "Nested/Local"
                        },
                        "Edm.String",
                        false,
                        true,
                    ),
                    ("Missing", "Edm.String", false, false),
                    ("", "N.Inner", true, false),
                    ("", "N.Outer", false, true),
                    ("N.InnerDerived/DerivedValue", "Edm.String", true, !entity),
                    ("N.OuterDerived/DerivedValue", "Edm.String", !entity, true),
                    ("N.Missing/DerivedValue", "Edm.String", false, false),
                    ("@N.Text", "Edm.String", true, true),
                    ("@N.Missing", "Edm.String", false, false),
                ] {
                    let annotation_xml =
                        format!(r#"<Annotation Term="N.Value"><Path>{path}</Path></Annotation>"#);
                    let annotation_json = format!(r#", "@N.Value":{{"$Path":"{path}"}}"#);
                    let (inline_xml, inline_json, group_xml, group_json) = if external {
                        (
                            String::new(),
                            String::new(),
                            format!(
                                r#"<Annotations Target="N.Outer/Nested/Local">{annotation_xml}</Annotations>"#
                            ),
                            format!(
                                r#", "$Annotations":{{"N.Outer/Nested/Local":{{"@N.Value":{{"$Path":"{path}"}}}}}}"#
                            ),
                        )
                    } else {
                        (
                            annotation_xml,
                            annotation_json,
                            String::new(),
                            String::new(),
                        )
                    };
                    let (member_xml, member_json) = if navigation {
                        (
                            format!(
                                r#"<NavigationProperty Name="Local" Type="N.Target">{inline_xml}</NavigationProperty>"#
                            ),
                            format!(
                                r#""$Kind":"NavigationProperty","$Type":"N.Target","$Nullable":true{inline_json}"#
                            ),
                        )
                    } else {
                        (
                            format!(
                                r#"<Property Name="Local" Type="Edm.String">{inline_xml}</Property>"#
                            ),
                            format!(r#""$Nullable":true{inline_json}"#),
                        )
                    };
                    let xml = format!(
                        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="N"><Term Name="Value" Type="{ty}"/><Term Name="Text" Type="Edm.String"/><EntityType Name="Target" Abstract="true"><Property Name="Name" Type="Edm.String"/></EntityType><ComplexType Name="Inner">{member_xml}</ComplexType><ComplexType Name="Outer"><Property Name="RootValue" Type="Edm.String"/><Property Name="Nested" Type="N.Inner"/></ComplexType><ComplexType Name="InnerDerived" BaseType="N.Inner"><Property Name="DerivedValue" Type="Edm.String"/></ComplexType><ComplexType Name="OuterDerived" BaseType="N.Outer"><Property Name="DerivedValue" Type="Edm.String"/></ComplexType>{group_xml}</Schema></edmx:DataServices></edmx:Edmx>"#
                    );
                    let json = format!(
                        r#"{{"$Version":"4.01","N":{{"Value":{{"$Kind":"Term","$Type":"{ty}"}},"Text":{{"$Kind":"Term"}},"Target":{{"$Kind":"EntityType","$Abstract":true,"Name":{{"$Nullable":true}}}},"Inner":{{"$Kind":"ComplexType","Local":{{{member_json}}}}},"Outer":{{"$Kind":"ComplexType","RootValue":{{"$Nullable":true}},"Nested":{{"$Type":"N.Inner","$Nullable":true}}}},"InnerDerived":{{"$Kind":"ComplexType","$BaseType":"N.Inner","DerivedValue":{{"$Nullable":true}}}},"OuterDerived":{{"$Kind":"ComplexType","$BaseType":"N.Outer","DerivedValue":{{"$Nullable":true}}}}{group_json}}}}}"#
                    );
                    let xml = if entity {
                        xml.replace("<ComplexType Name=\"Inner\">", "<EntityType Name=\"Inner\" Abstract=\"true\">")
                    .replace(&format!("{member_xml}</ComplexType>"), &format!("{member_xml}</EntityType>"))
                    .replace("<ComplexType Name=\"InnerDerived\" BaseType=\"N.Inner\"><Property Name=\"DerivedValue\" Type=\"Edm.String\"/></ComplexType>", "<EntityType Name=\"InnerDerived\" BaseType=\"N.Inner\" Abstract=\"true\"><Property Name=\"DerivedValue\" Type=\"Edm.String\"/></EntityType>")
                    .replace("<Property Name=\"Nested\" Type=\"N.Inner\"/>", "<NavigationProperty Name=\"Nested\" Type=\"N.Inner\"/>")
                    } else {
                        xml
                    };
                    let json = if entity {
                        json.replace(
                            r#""Inner":{"$Kind":"ComplexType""#,
                            r#""Inner":{"$Kind":"EntityType","$Abstract":true"#,
                        )
                        .replace(
                            r#""InnerDerived":{"$Kind":"ComplexType""#,
                            r#""InnerDerived":{"$Kind":"EntityType","$Abstract":true"#,
                        )
                        .replace(
                            r#""Nested":{"$Type":"N.Inner""#,
                            r#""Nested":{"$Kind":"NavigationProperty","$Type":"N.Inner""#,
                        )
                    } else {
                        json
                    };
                    for parsed in [
                        parser::from_xml_reader_with_profile(xml.as_bytes(), P::Strict),
                        parser::from_json_reader_with_profile(json.as_bytes(), P::Strict),
                    ] {
                        let model =
                            Resolver::resolve_document(parsed.unwrap().edmx.unwrap()).unwrap();
                        validator::validate_document(&model).unwrap();
                        assert_eq!(
                            model.schemas[0].annotations.get().unwrap().len(),
                            1,
                            "{model:?}"
                        );
                        let diagnostics = validator::validate_annotations(&model);
                        assert_eq!(
                            diagnostics.is_empty(),
                            if external {
                                external_valid
                            } else {
                                embedded_valid
                            },
                            "external={external}, {path}: {diagnostics:?}"
                        );
                        if !diagnostics.is_empty() && !path.is_empty() {
                            assert!(
                                diagnostics.iter().any(|d| d.issue
                                    == validator::AnnotationIssue::InvalidExpressionPath),
                                "{diagnostics:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
