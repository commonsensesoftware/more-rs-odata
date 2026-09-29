//! Shared spatial-default representation for CSDL XML and JSON.
use serde_json::{Value, json};

pub(crate) struct SpatialValue {
    pub srid: u64,
    pub geometry: Value,
}

impl SpatialValue {
    pub fn parse(value: &str, default_srid: u64) -> Option<Self> {
        if value.starts_with('{') {
            return Self::from_json(serde_json::from_str(value).ok()?, default_srid);
        }
        let (srid, geometry) = value.split_once(';')?;
        let (label, number) = srid.split_once('=')?;
        if !label.eq_ignore_ascii_case("SRID") {
            return None;
        }
        let srid = number.parse().ok()?;
        let mut parser = Wkt {
            rest: geometry,
            depth: 0,
        };
        let geometry = parser.geometry()?;
        if !parser.rest.trim().is_empty() || !valid_geometry(&geometry, 0) {
            return None;
        }
        Some(Self { srid, geometry })
    }

    pub fn from_json(mut geometry: Value, default_srid: u64) -> Option<Self> {
        let srid = match geometry.get("crs") {
            None => default_srid,
            Some(crs) => {
                if crs.get("type")?.as_str()? != "name" {
                    return None;
                }
                let name = crs.get("properties")?.get("name")?.as_str()?;
                if name == "urn:ogc:def:crs:OGC:1.3:CRS84" {
                    4326
                } else {
                    name.rsplit(':').next()?.parse().ok()?
                }
            }
        };
        geometry.as_object_mut()?.remove("crs");
        if !valid_geometry(&geometry, 0) {
            return None;
        }
        Some(Self { srid, geometry })
    }

    pub fn to_json(&self, default_srid: u64) -> Value {
        let mut geometry = self.geometry.clone();
        if self.srid != default_srid {
            geometry["crs"] =
                json!({"type":"name","properties":{"name":format!("EPSG:{}",self.srid)}});
        }
        geometry
    }

    pub fn to_wkt(&self) -> String {
        format!("SRID={};{}", self.srid, geometry_wkt(&self.geometry))
    }
}

fn position(value: &Value) -> bool {
    value.as_array().is_some_and(|a| {
        (2..=4).contains(&a.len()) && a.iter().all(|n| n.as_f64().is_some_and(f64::is_finite))
    })
}

fn line(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|a| a.len() >= 2 && a.iter().all(position))
}
fn ring(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|a| a.len() >= 4 && a.first() == a.last() && a.iter().all(position))
}
fn polygon(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|a| !a.is_empty() && a.iter().all(ring))
}
fn list(value: &Value, check: fn(&Value) -> bool) -> bool {
    value.as_array().is_some_and(|a| a.iter().all(check))
}

fn valid_geometry(value: &Value, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return false;
    };
    let coordinates = &value["coordinates"];
    match kind {
        "Point" => position(coordinates),
        "LineString" => line(coordinates),
        "Polygon" => polygon(coordinates),
        "MultiPoint" => list(coordinates, position),
        "MultiLineString" => list(coordinates, line),
        "MultiPolygon" => list(coordinates, polygon),
        "GeometryCollection" => value["geometries"]
            .as_array()
            .is_some_and(|a| a.iter().all(|v| valid_geometry(v, depth + 1))),
        _ => false,
    }
}

fn numbers(value: &Value) -> String {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}
fn coordinates(value: &Value, depth: usize) -> String {
    if depth == 0 {
        return numbers(value);
    }
    format!(
        "({})",
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| coordinates(v, depth - 1))
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn geometry_wkt(value: &Value) -> String {
    let kind = value["type"].as_str().unwrap();
    let data = match kind {
        "Point" => format!("({})", numbers(&value["coordinates"])),
        "LineString" => coordinates(&value["coordinates"], 1),
        "Polygon" | "MultiLineString" => coordinates(&value["coordinates"], 2),
        "MultiPolygon" => coordinates(&value["coordinates"], 3),
        "MultiPoint" => format!(
            "({})",
            value["coordinates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| format!("({})", numbers(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        "GeometryCollection" => format!(
            "({})",
            value["geometries"]
                .as_array()
                .unwrap()
                .iter()
                .map(geometry_wkt)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => unreachable!(),
    };
    format!("{kind}{data}")
}

struct Wkt<'a> {
    rest: &'a str,
    depth: usize,
}
impl Wkt<'_> {
    fn token(&mut self, token: &str) -> bool {
        self.rest = self.rest.trim_start();
        if let Some(rest) = self.rest.strip_prefix(token) {
            self.rest = rest;
            true
        } else {
            false
        }
    }
    fn position(&mut self) -> Option<Value> {
        let mut values = Vec::new();
        loop {
            self.rest = self.rest.trim_start();
            let end = self
                .rest
                .find(|c: char| c.is_whitespace() || c == ',' || c == ')')
                .unwrap_or(self.rest.len());
            if end == 0 {
                break;
            }
            let number = self.rest[..end]
                .strip_prefix('+')
                .unwrap_or(&self.rest[..end]);
            let number: serde_json::Number = number.parse().ok()?;
            if !number.as_f64()?.is_finite() {
                return None;
            }
            values.push(Value::Number(number));
            self.rest = &self.rest[end..];
            if self.rest.trim_start().starts_with([',', ')']) {
                break;
            }
        }
        ((2..=4).contains(&values.len())).then_some(Value::Array(values))
    }
    fn coordinates(&mut self, depth: usize) -> Option<Value> {
        if depth == 0 {
            return self.position();
        }
        if !self.token("(") {
            return None;
        }
        let mut values = Vec::new();
        if self.token(")") {
            return Some(Value::Array(values));
        }
        loop {
            values.push(self.coordinates(depth - 1)?);
            if self.token(")") {
                break;
            }
            if !self.token(",") {
                return None;
            }
        }
        Some(Value::Array(values))
    }
    fn geometry(&mut self) -> Option<Value> {
        self.depth += 1;
        if self.depth > 64 {
            return None;
        }
        self.rest = self.rest.trim_start();
        let end = self.rest.find(|c: char| !c.is_ascii_alphabetic())?;
        let kind = self.rest[..end].to_ascii_lowercase();
        self.rest = &self.rest[end..];
        let value = match kind.as_str() {
            "point" => {
                if !self.token("(") {
                    return None;
                }
                let point = self.position()?;
                if !self.token(")") {
                    return None;
                }
                json!({"type":"Point","coordinates":point})
            }
            "linestring" => json!({"type":"LineString","coordinates":self.coordinates(1)?}),
            "polygon" => json!({"type":"Polygon","coordinates":self.coordinates(2)?}),
            "multilinestring" => {
                json!({"type":"MultiLineString","coordinates":self.coordinates(2)?})
            }
            "multipolygon" => json!({"type":"MultiPolygon","coordinates":self.coordinates(3)?}),
            "multipoint" => {
                if !self.token("(") {
                    return None;
                }
                let mut points = Vec::new();
                if !self.token(")") {
                    loop {
                        if !self.token("(") {
                            return None;
                        }
                        points.push(self.position()?);
                        if !self.token(")") {
                            return None;
                        }
                        if self.token(")") {
                            break;
                        }
                        if !self.token(",") {
                            return None;
                        }
                    }
                }
                json!({"type":"MultiPoint","coordinates":points})
            }
            "geometrycollection" => {
                if !self.token("(") {
                    return None;
                }
                let mut values = Vec::new();
                if !self.token(")") {
                    loop {
                        values.push(self.geometry()?);
                        if self.token(")") {
                            break;
                        }
                        if !self.token(",") {
                            return None;
                        }
                    }
                }
                json!({"type":"GeometryCollection","geometries":values})
            }
            _ => return None,
        };
        self.depth -= 1;
        Some(value)
    }
}
