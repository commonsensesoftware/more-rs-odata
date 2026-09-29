#![cfg(all(feature = "resolver", feature = "validator"))]

use csdl_edm::edm::{ConstraintPathSegment, SchemaElement};
use csdl_edm::resolver::Resolver;
use csdl_edm::validator::{ValidationError, validate_document};
use csdl_edm::{CsdlDocument, parser};

fn documents(body: &str) -> Vec<CsdlDocument> {
    let xml = format!(
        r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Demo" Alias="D">{body}</Schema></edmx:DataServices></edmx:Edmx>"#
    );
    let document = parser::from_xml_reader(xml.as_bytes()).unwrap();
    let json = document.to_json_value().to_string();
    vec![document, parser::from_json_reader(json.as_bytes()).unwrap()]
}

fn invalid(body: &str, reason: &str) {
    for document in documents(body) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        let errors = validate_document(&model).unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, ValidationError::InvalidNavigation { reason: actual, .. } if *actual == reason)), "{errors:?}");
    }
}

#[test]
fn resolves_constraints_through_complex_properties_on_complex_owners() {
    for document in documents(
        r#"
        <ComplexType Name="Value"><Property Name="Code" Type="Edm.Int32" Nullable="false"/></ComplexType>
        <ComplexType Name="Owner"><Property Name="Value" Type="D.Value" Nullable="false"/>
          <NavigationProperty Name="Target" Type="D.Target" Nullable="false"><ReferentialConstraint Property="Value/Code" ReferencedProperty="Value/Code"/></NavigationProperty>
        </ComplexType>
        <EntityType Name="Target"><Property Name="Value" Type="D.Value" Nullable="false"/></EntityType>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let SchemaElement::ComplexType(owner) = model.schemas[0].elements[1].as_ref() else {
            panic!()
        };
        let constraint = &owner.navigation_properties()[0].referential_constraints[0];
        let path = constraint.property_path.get().unwrap();
        assert_eq!(path.len(), 2);
        assert!(
            matches!(&path[1], ConstraintPathSegment::Property(p) if p.upgrade().unwrap().name == "Code")
        );
    }
}

#[test]
fn validates_constraint_types_cardinality_and_nullability() {
    for (nav_type, dependent_type, nullable, reason) in [
        (
            "D.Target",
            "Edm.String",
            "false",
            "Dependent and principal constraint types must match",
        ),
        (
            "Collection(D.Target)",
            "Edm.Int32",
            "false",
            "Referential constraints require a single-valued navigation",
        ),
        (
            "D.Target",
            "Edm.Int32",
            "true",
            "Dependent nullability must equal navigation or principal nullability",
        ),
    ] {
        invalid(
            &format!(
                r#"
            <EntityType Name="Owner"><Property Name="FK" Type="{dependent_type}" Nullable="{nullable}"/>
              <NavigationProperty Name="Target" Type="{nav_type}" Nullable="false"><ReferentialConstraint Property="FK" ReferencedProperty="Code"/></NavigationProperty>
            </EntityType>
            <EntityType Name="Target"><Property Name="Code" Type="Edm.Int32" Nullable="false"/></EntityType>"#
            ),
            reason,
        );
    }
}

#[test]
fn validates_partner_direction_and_reciprocity() {
    invalid(
        r#"
        <EntityType Name="A"><NavigationProperty Name="B" Type="D.B" Partner="Wrong"/></EntityType>
        <EntityType Name="B"><NavigationProperty Name="Wrong" Type="D.B"/></EntityType>"#,
        "Partner must target the declaring entity type or a base type",
    );
    invalid(
        r#"
        <EntityType Name="A"><NavigationProperty Name="B" Type="D.B" Partner="Back"/><NavigationProperty Name="Other" Type="D.B"/></EntityType>
        <EntityType Name="B"><NavigationProperty Name="Back" Type="D.A" Partner="Other"/></EntityType>"#,
        "Partner must point back to the current navigation",
    );
    invalid(
        r#"
        <ComplexType Name="C"><NavigationProperty Name="B" Type="D.B" Partner="Self"/></ComplexType>
        <EntityType Name="B"><NavigationProperty Name="Self" Type="D.B"/></EntityType>"#,
        "Navigation declared on a complex type cannot specify Partner",
    );
}

#[test]
fn partner_paths_allow_derived_casts_but_not_navigation_traversal() {
    for document in documents(
        r#"
        <EntityType Name="A"><NavigationProperty Name="B" Type="D.B" Partner="D.Derived/Back"/></EntityType>
        <EntityType Name="B"/>
        <EntityType Name="Derived" BaseType="D.B"><NavigationProperty Name="Back" Type="D.A"/></EntityType>"#,
    ) {
        validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap()).unwrap();
    }
    for document in documents(
        r#"
        <EntityType Name="A"><NavigationProperty Name="B" Type="D.B" Partner="Next/Back"/></EntityType>
        <EntityType Name="B"><NavigationProperty Name="Next" Type="D.B"/><NavigationProperty Name="Back" Type="D.A"/></EntityType>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        assert!(
            validate_document(&model)
                .unwrap_err()
                .iter()
                .any(|e| matches!(e, ValidationError::UnknownNavigationPartner { .. }))
        );
    }
}

#[test]
fn recursive_containment_requires_optional_single_parent_and_keyed_children() {
    let body = r#"<EntityType Name="Node"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/>
        <NavigationProperty Name="Children" Type="Collection(D.Node)" ContainsTarget="true" Partner="Parent"/>
        <NavigationProperty Name="Parent" Type="D.Node" Nullable="true" Partner="Children"/>
    </EntityType>"#;
    for document in documents(body) {
        validate_document(&Resolver::resolve_document(document.edmx.unwrap()).unwrap()).unwrap();
    }
    invalid(
        &body.replace("Nullable=\"true\"", "Nullable=\"false\""),
        "Recursive containment partner must be nullable and single-valued",
    );
    invalid(
        r#"<EntityType Name="Node"><NavigationProperty Name="Children" Type="Collection(D.Node)" ContainsTarget="true"/></EntityType>"#,
        "Collection containment target requires a key",
    );
}

#[test]
fn rejects_missing_duplicate_and_invalid_on_delete_actions() {
    for action in [
        "<OnDelete/>",
        "<OnDelete Action=\"Unknown\"/>",
        "<OnDelete Action=\"Cascade\"/><OnDelete Action=\"None\"/>",
    ] {
        let xml = format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.01"><edmx:DataServices><Schema Namespace="Demo"><EntityType Name="Item"><NavigationProperty Name="Self" Type="Demo.Item">{action}</NavigationProperty></EntityType></Schema></edmx:DataServices></edmx:Edmx>"#
        );
        assert!(parser::from_xml_reader(xml.as_bytes()).is_err());
    }
}

#[test]
fn rejects_unresolved_constraint_paths_including_empty_segments() {
    for path in ["Missing", "Value//Code", "Value/Unknown", "Value/Code/"] {
        for document in documents(&format!(
            r#"
            <ComplexType Name="Value"><Property Name="Code" Type="Edm.Int32"/></ComplexType>
            <EntityType Name="Owner"><Property Name="Value" Type="D.Value"/>
              <NavigationProperty Name="Target" Type="D.Owner"><ReferentialConstraint Property="{path}" ReferencedProperty="Value/Code"/></NavigationProperty>
            </EntityType>"#
        )) {
            let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
            assert!(
                validate_document(&model)
                    .unwrap_err()
                    .iter()
                    .any(|e| matches!(
                        e,
                        ValidationError::UnknownReferentialConstraintProperty { .. }
                    ))
            );
        }
    }
}

#[test]
fn nonrecursive_containment_partner_must_be_required() {
    invalid(
        r#"
        <EntityType Name="Parent"><NavigationProperty Name="Child" Type="D.Child" ContainsTarget="true" Partner="Parent"/></EntityType>
        <EntityType Name="Child"><NavigationProperty Name="Parent" Type="D.Parent" Nullable="true" Partner="Child"/></EntityType>"#,
        "Non-recursive containment partner must be non-nullable",
    );
}

#[test]
fn collection_of_containing_complex_values_is_version_specific() {
    for document in documents(
        r#"
        <EntityType Name="Item"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/><Property Name="Values" Type="Collection(D.Value)" Nullable="false"/></EntityType>
        <ComplexType Name="Value"><NavigationProperty Name="Child" Type="D.Item" ContainsTarget="true"/></ComplexType>"#,
    ) {
        let mut edmx = document.edmx.unwrap();
        validate_document(&Resolver::resolve_document(edmx.clone()).unwrap()).unwrap();
        edmx.version = Some("4.0".into());
        let model = Resolver::resolve_document(edmx).unwrap();
        assert!(
            validate_document(&model)
                .unwrap_err()
                .iter()
                .any(|e| matches!(e, ValidationError::InvalidContainmentProperty { .. }))
        );
    }
}

#[test]
fn entity_valued_constraints_resolve_to_navigation_endpoints() {
    for document in documents(
        r#"
        <EntityType Name="Related"/>
        <EntityType Name="Owner"><NavigationProperty Name="Related" Type="D.Related" Nullable="false"/>
          <NavigationProperty Name="Target" Type="D.Target" Nullable="false"><ReferentialConstraint Property="Related" ReferencedProperty="Related"/></NavigationProperty>
        </EntityType>
        <EntityType Name="Target"><NavigationProperty Name="Related" Type="D.Related" Nullable="false"/></EntityType>"#,
    ) {
        let model = Resolver::resolve_document(document.edmx.unwrap()).unwrap();
        validate_document(&model).unwrap();
        let SchemaElement::EntityType(owner) = model.schemas[0].elements[1].as_ref() else {
            panic!()
        };
        assert!(matches!(
            &owner.navigation_properties()[1].referential_constraints[0]
                .property_path
                .get()
                .unwrap()[0],
            ConstraintPathSegment::NavigationProperty(_)
        ));
    }
}

#[test]
fn json_navigation_attributes_reject_wrong_value_kinds() {
    for attribute in [
        r#""$OnDelete":1"#,
        r#""$Partner":false"#,
        r#""$Nullable":"false"#,
        r#""$ContainsTarget":0"#,
        r#""$ReferentialConstraint":[]"#,
        r#""$ReferentialConstraint":{"ID":1}"#,
    ] {
        let json = format!(
            r#"{{"$Version":"4.01","Demo":{{"Item":{{"$Kind":"EntityType","Self":{{"$Kind":"NavigationProperty","$Type":"Demo.Item",{attribute}}}}}}}}}"#
        );
        assert!(parser::from_json_reader(json.as_bytes()).is_err(), "{json}");
    }
}
