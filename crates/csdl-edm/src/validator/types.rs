use super::ValidationError;
use crate::csdl::{MaxLengthFacet, ScaleFacet};
use crate::edm::*;

pub(super) fn validate(model: &Model, version: &str) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for element in &model.elements {
        match element.as_ref() {
            SchemaElement::TypeDefinition(t) => check(
                &t.name,
                Some(&t.underlying_type),
                None,
                &t.facets,
                version,
                &mut errors,
            ),
            SchemaElement::EntityType(t) => {
                properties(&t.name, t.properties(), version, &mut errors)
            }
            SchemaElement::ComplexType(t) => {
                properties(&t.name, t.properties(), version, &mut errors)
            }
            SchemaElement::Term(t) => {
                if let Some(ty) = t.ty() {
                    check_term(&t.name, ty, &t.facets, version, &mut errors);
                }
            }
            SchemaElement::Function(f) => operation(
                &f.name,
                &f.parameters,
                f.return_type.as_ref(),
                version,
                &mut errors,
            ),
            SchemaElement::Action(a) => operation(
                &a.name,
                &a.parameters,
                a.return_type.as_ref(),
                version,
                &mut errors,
            ),
            _ => {}
        }
    }
    errors
}

fn properties(
    owner: &str,
    properties: &[std::sync::Arc<Property>],
    version: &str,
    errors: &mut Vec<ValidationError>,
) {
    for p in properties {
        let name = format!("{owner}/{}", p.name);
        let (primitive, definition) = match &p.ty {
            ResolvedType::Primitive(p) => (Some(p), None),
            ResolvedType::TypeDefinition(t) => (Some(&t.underlying_type), Some(t.as_ref())),
            _ => (None, None),
        };
        check(&name, primitive, definition, &p.facets, version, errors);
    }
}

fn operation(
    name: &str,
    parameters: &[OperationParameter],
    return_type: Option<&OperationReturnType>,
    version: &str,
    errors: &mut Vec<ValidationError>,
) {
    for p in parameters {
        check_term(
            &format!("{name}/{}", p.name),
            &p.ty,
            &p.facets,
            version,
            errors,
        );
    }
    if let Some(r) = return_type {
        check_term(
            &format!("{name}/$ReturnType"),
            &r.ty,
            &r.facets,
            version,
            errors,
        );
    }
}

fn check_term(
    name: &str,
    ty: &TermType,
    facets: &TypeFacets,
    version: &str,
    errors: &mut Vec<ValidationError>,
) {
    let (primitive, definition) = match ty {
        TermType::Primitive(p) => (Some(p), None),
        TermType::TypeDefinition(t) => (Some(&t.underlying_type), Some(t.as_ref())),
        _ => (None, None),
    };
    check(name, primitive, definition, facets, version, errors);
}

fn check(
    name: &str,
    primitive: Option<&PrimitiveType>,
    definition: Option<&TypeDefinition>,
    facets: &TypeFacets,
    version: &str,
    errors: &mut Vec<ValidationError>,
) {
    use PrimitiveType::*;
    let mut fail = |facet, reason| {
        errors.push(ValidationError::InvalidFacet {
            element: name.into(),
            facet,
            reason,
        })
    };
    let mut effective = facets.clone();
    if let Some(t) = definition {
        macro_rules! inherit {
            ($field:ident, $name:literal) => {
                if t.facets.$field.is_some() {
                    if facets.$field.is_some() {
                        fail($name, "Type-definition facets must not be re-specified");
                    }
                    effective.$field = t.facets.$field.clone();
                }
            };
        }
        inherit!(max_length, "MaxLength");
        inherit!(precision, "Precision");
        inherit!(scale, "Scale");
        inherit!(srid, "SRID");
        inherit!(unicode, "Unicode");
    }
    if let Some(length) = effective.max_length {
        if !matches!(primitive, Some(Binary | String | Stream)) {
            fail("MaxLength", "MaxLength requires binary, string, or stream");
        }
        if length == MaxLengthFacet::Number(0) {
            fail("MaxLength", "MaxLength must be positive");
        }
    }
    let precision = effective
        .precision
        .as_ref()
        .and_then(|p| p.parse::<u64>().ok());
    if effective.precision.is_some() {
        match (primitive, precision) {
            (Some(Decimal), Some(p)) if p > 0 => {}
            (Some(DateTimeOffset | Duration | TimeOfDay), Some(p)) if p <= 12 => {}
            _ => fail(
                "Precision",
                "Precision requires positive decimal digits or temporal precision from zero to twelve",
            ),
        }
    }
    if let Some(scale) = effective.scale {
        if primitive != Some(&Decimal) {
            fail("Scale", "Scale requires Decimal");
        }
        if let (ScaleFacet::Number(scale), Some(precision)) = (scale, precision) {
            if scale > precision {
                fail("Scale", "Scale must not exceed Precision");
            }
        }
        if scale == ScaleFacet::Floating && version == "4.0" {
            fail("Scale", "Floating scale requires CSDL 4.01");
        }
    }
    if effective.unicode.is_some() && primitive != Some(&String) {
        fail("Unicode", "Unicode requires String");
    }
    if effective.srid.is_some()
        && !matches!(
            primitive,
            Some(
                Geography
                    | GeographyPoint
                    | GeographyLineString
                    | GeographyPolygon
                    | GeographyMultiPoint
                    | GeographyMultiLineString
                    | GeographyMultiPolygon
                    | GeographyCollection
                    | Geometry
                    | GeometryPoint
                    | GeometryLineString
                    | GeometryPolygon
                    | GeometryMultiPoint
                    | GeometryMultiLineString
                    | GeometryMultiPolygon
                    | GeometryCollection
            )
        )
    {
        fail("SRID", "SRID requires a spatial type");
    }
}
