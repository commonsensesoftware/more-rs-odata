use super::ValidationError;
use crate::csdl::{MaxLengthFacet, ScaleFacet};
use crate::edm::*;

pub(super) fn validate(model: &Model) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for element in &model.elements {
        match element.as_ref() {
            SchemaElement::EntityType(t) => properties(&t.name, t.properties(), &mut errors),
            SchemaElement::ComplexType(t) => properties(&t.name, t.properties(), &mut errors),
            SchemaElement::Term(t) => {
                if let Some(ty) = t.ty() {
                    check(
                        &t.name,
                        t.default_value.as_deref(),
                        ty,
                        t.is_collection,
                        &t.facets,
                        &mut errors,
                    );
                }
            }
            SchemaElement::Function(f) => parameters(&f.name, &f.parameters, &mut errors),
            SchemaElement::Action(a) => parameters(&a.name, &a.parameters, &mut errors),
            _ => {}
        }
    }
    errors
}

fn parameters(owner: &str, parameters: &[OperationParameter], errors: &mut Vec<ValidationError>) {
    for p in parameters {
        check(
            &format!("{owner}/{}", p.name),
            p.default_value.as_deref(),
            &p.ty,
            p.is_collection,
            &p.facets,
            errors,
        );
    }
}

fn properties(
    owner: &str,
    properties: &[std::sync::Arc<Property>],
    errors: &mut Vec<ValidationError>,
) {
    for p in properties {
        let ty = match &p.ty {
            ResolvedType::Builtin(t) => TermType::Builtin(*t),
            ResolvedType::Primitive(t) => TermType::Primitive(t.clone()),
            ResolvedType::Enum(t) => TermType::Enum(t.clone()),
            ResolvedType::Complex(t) => TermType::Complex(t.clone()),
            ResolvedType::TypeDefinition(t) => TermType::TypeDefinition(t.clone()),
        };
        check(
            &format!("{owner}/{}", p.name),
            p.default_value.as_deref(),
            &ty,
            p.is_collection,
            &p.facets,
            errors,
        );
    }
}

fn check(
    name: &str,
    value: Option<&str>,
    ty: &TermType,
    collection: bool,
    facets: &TypeFacets,
    errors: &mut Vec<ValidationError>,
) {
    let Some(value) = value else {
        return;
    };
    let mut effective = facets.clone();
    let valid = if collection {
        false
    } else {
        match ty {
            TermType::Primitive(p) => primitive(value, p, &effective),
            TermType::TypeDefinition(t) => {
                effective.max_length = t.facets.max_length.or(effective.max_length);
                effective.precision = t.facets.precision.clone().or(effective.precision);
                effective.scale = t.facets.scale.or(effective.scale);
                effective.unicode = t.facets.unicode.or(effective.unicode);
                effective.srid = t.facets.srid.or(effective.srid);
                primitive(value, &t.underlying_type, &effective)
            }
            TermType::Enum(t) => {
                let names = value.split(',').map(str::trim).collect::<Vec<_>>();
                (t.is_flags || names.len() == 1)
                    && names.iter().all(|name| {
                        t.members.iter().any(|m| m.name == *name)
                            || name.parse::<i64>().is_ok_and(|n| {
                                (!t.is_flags || n >= 0)
                                    && primitive(name, &t.underlying_type, &TypeFacets::default())
                            })
                    })
            }
            _ => false,
        }
    };
    if !valid {
        errors.push(ValidationError::InvalidDefaultValue {
            element: name.into(),
            value: value.into(),
        });
    }
}

pub(super) fn primitive(value: &str, ty: &PrimitiveType, facets: &TypeFacets) -> bool {
    let type_name = format!("{ty:?}");
    if let Some(shape) = type_name
        .strip_prefix("Geography")
        .or_else(|| type_name.strip_prefix("Geometry"))
    {
        let default_srid = if type_name.starts_with("Geography") {
            4326
        } else {
            0
        };
        let expected_srid = match facets.srid {
            Some(crate::csdl::SridFacet::Number(n)) => Some(n),
            Some(crate::csdl::SridFacet::Variable) => None,
            None => Some(default_srid),
        };
        let Some(spatial) =
            crate::spatial::SpatialValue::parse(value, expected_srid.unwrap_or(default_srid))
        else {
            return false;
        };
        let expected_shape = if shape == "Collection" {
            "GeometryCollection"
        } else {
            shape
        };
        return (shape.is_empty() || spatial.geometry["type"].as_str() == Some(expected_shape))
            && expected_srid.is_none_or(|srid| srid == spatial.srid);
    }
    use crate::edm::PrimitiveType::*;
    let precision = facets
        .precision
        .as_deref()
        .and_then(|p| p.parse::<usize>().ok());
    (match ty {
        String => facets.unicode != Some(false) || value.is_ascii(),
        Boolean => matches!(value, "true" | "false"),
        Byte => value.parse::<u8>().is_ok(),
        SByte => value.parse::<i8>().is_ok(),
        Int16 => value.parse::<i16>().is_ok(),
        Int32 => value.parse::<i32>().is_ok(),
        Int64 => value.parse::<i64>().is_ok(),
        Single => {
            matches!(value, "INF" | "-INF" | "NaN")
                || numeric(value, true) && value.parse::<f32>().is_ok_and(f32::is_finite)
        }
        Double => {
            matches!(value, "INF" | "-INF" | "NaN")
                || numeric(value, true) && value.parse::<f64>().is_ok_and(f64::is_finite)
        }
        Decimal => decimal(value, facets),
        Guid => {
            let groups = value.split('-').collect::<Vec<_>>();
            groups.len() == 5
                && groups
                    .iter()
                    .zip([8, 4, 4, 4, 12])
                    .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
        }
        Date => date(value),
        TimeOfDay => time(value, precision.unwrap_or(0)),
        DateTimeOffset => value.split_once(['T', 't']).is_some_and(|(d, t)| {
            if !date(d) {
                return false;
            }
            if let Some(t) = t.strip_suffix(['Z', 'z']) {
                return time(t, precision.unwrap_or(0));
            }
            let Some(index) = t.rfind(['+', '-']) else {
                return false;
            };
            let (t, offset) = t.split_at(index);
            let offset = &offset[1..];
            let Some((h, m)) = offset.split_once(':') else {
                return false;
            };
            h.len() == 2
                && m.len() == 2
                && h.parse::<u8>().is_ok_and(|h| h <= 14)
                && m.parse::<u8>()
                    .is_ok_and(|m| m < 60 && (h != "14" || m == 0))
                && time(t, precision.unwrap_or(0))
        }),
        Duration => duration(value, precision.unwrap_or(0)),
        Binary => {
            let unpadded = value.trim_end_matches('=');
            unpadded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                && unpadded.len() % 4 != 1
                && value.len() - unpadded.len() <= 2
        }
        PrimitiveType => true,
        // Streams are resources rather than scalar literal values.
        _ => false,
    }) && match facets.max_length {
        Some(MaxLengthFacet::Number(max)) if *ty == String => value.chars().count() as u64 <= max,
        Some(MaxLengthFacet::Number(max)) if *ty == Binary => {
            (value.trim_end_matches('=').len() * 6 / 8) as u64 <= max
        }
        _ => true,
    }
}

fn numeric(value: &str, exponent: bool) -> bool {
    let value = value.strip_prefix(['+', '-']).unwrap_or(value);
    let (mantissa, exp) = value
        .split_once(['e', 'E'])
        .map(|(m, e)| (m, Some(e)))
        .unwrap_or((value, None));
    if let Some(exp) = exp {
        let exp = exp.strip_prefix(['+', '-']).unwrap_or(exp);
        if !exponent || exp.is_empty() || !exp.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    let parts = mantissa.split('.').collect::<Vec<_>>();
    parts.len() <= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn decimal(value: &str, facets: &TypeFacets) -> bool {
    let floating = facets.scale == Some(ScaleFacet::Floating);
    if matches!(value, "INF" | "-INF" | "NaN") {
        return floating;
    }
    if !numeric(value, floating) {
        return false;
    }
    let unsigned = value.trim_start_matches(['+', '-']);
    let mantissa = unsigned.split(['e', 'E']).next().unwrap();
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let precision = facets
        .precision
        .as_deref()
        .and_then(|p| p.parse::<usize>().ok());
    let significant = integer.trim_start_matches('0').len() + fraction.len();
    if precision.is_some_and(|p| significant > p) {
        return false;
    }
    match facets.scale.unwrap_or(ScaleFacet::Number(0)) {
        ScaleFacet::Number(scale) => {
            fraction.len() as u64 <= scale
                && precision.is_none_or(|p| {
                    integer.trim_start_matches('0').len() <= p.saturating_sub(scale as usize)
                })
        }
        _ => true,
    }
}

fn date(value: &str) -> bool {
    let value = value.strip_prefix('-').unwrap_or(value);
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() != 3
        || parts[0].len() < 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        parts[0].parse::<u64>(),
        parts[1].parse::<u8>(),
        parts[2].parse::<u8>(),
    ) else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    };
    day > 0 && day <= days
}

fn time(value: &str, precision: usize) -> bool {
    let (value, fraction) = value
        .split_once('.')
        .map(|(v, f)| (v, Some(f)))
        .unwrap_or((value, None));
    if fraction.is_some_and(|f| {
        f.is_empty() || f.len() > precision || !f.bytes().all(|b| b.is_ascii_digit())
    }) {
        return false;
    }
    let parts = value.split(':').collect::<Vec<_>>();
    (parts.len() == 3 || parts.len() == 2 && fraction.is_none())
        && parts.iter().zip([24, 60, 61]).all(|(p, max)| {
            p.len() == 2
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u8>().is_ok_and(|n| n < max)
        })
}

fn duration(value: &str, precision: usize) -> bool {
    let Some(mut rest) = value.strip_prefix('-').unwrap_or(value).strip_prefix('P') else {
        return false;
    };
    let mut found = false;
    if let Some((days, tail)) = rest.split_once('D') {
        if days.is_empty() || !days.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        found = true;
        rest = tail;
    }
    if rest.is_empty() {
        return found;
    }
    let Some(mut rest) = rest.strip_prefix('T') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    for unit in ['H', 'M', 'S'] {
        if let Some((number, tail)) = rest.split_once(unit) {
            let valid = if unit == 'S' {
                let (whole, frac) = number.split_once('.').unwrap_or((number, ""));
                !whole.is_empty()
                    && whole.bytes().all(|b| b.is_ascii_digit())
                    && frac.len() <= precision
                    && frac.bytes().all(|b| b.is_ascii_digit())
                    && !number.ends_with('.')
            } else {
                !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
            };
            if !valid {
                return false;
            }
            found = true;
            rest = tail;
        }
    }
    found && rest.is_empty()
}
