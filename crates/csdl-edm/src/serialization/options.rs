use super::*;
use crate::conformance::ValidationProfile;
use crate::expr::CsdlAnnotationExpression as E;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// JSON type-control emission policy. Explicit record types are retained even
/// with `None` when omission could change the expression's interpretation.
pub enum MetadataLevel {
    #[default]
    Minimal,
    Full,
    None,
}

#[derive(Debug, Clone)]
/// Representation options; these do not negotiate HTTP media types.
pub struct SerializationOptions {
    /// Encode typed Int64 and Decimal values as JSON strings.
    pub ieee754_compatible: bool,
    pub metadata: MetadataLevel,
    /// Absolute source document URI used to resolve XML Base and references.
    pub source_uri: Option<String>,
    pub profile: ValidationProfile,
}
impl Default for SerializationOptions {
    fn default() -> Self {
        Self {
            ieee754_compatible: false,
            metadata: MetadataLevel::Minimal,
            source_uri: None,
            profile: ValidationProfile::Strict,
        }
    }
}

impl CsdlDocument {
    /// Buffered source/representation checks. Use the loader variant when a
    /// complete referenced graph must also be semantically validated.
    pub fn write_as_with_options<W: io::Write>(
        &self,
        mut writer: W,
        format: CsdlFormat,
        options: &SerializationOptions,
    ) -> io::Result<()> {
        let edmx = self
            .edmx
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing Edmx"))?;
        self.write_with_catalog(&mut writer, format, options, &Catalog::new(edmx))
    }
    fn write_with_catalog<W: io::Write>(
        &self,
        mut writer: W,
        format: CsdlFormat,
        options: &SerializationOptions,
        catalog: &Catalog,
    ) -> io::Result<()> {
        let mut document = self.clone();
        let edmx = document
            .edmx
            .as_mut()
            .ok_or_else(|| io::Error::other("Missing Edmx"))?;
        rebase(edmx, options)?;
        catalog.nullability(edmx, format)?;
        if format == CsdlFormat::Xml {
            super::defaults::materialize_with_catalog(edmx, catalog);
            return document.write_as_with_profile(writer, format, options.profile);
        }
        let edmx = edmx.clone();
        let mut value = document.to_json_value();
        catalog.document(&mut value, &edmx, options)?;
        let json = serde_json::to_vec_pretty(&value).map_err(io::Error::other)?;
        crate::parser::from_json_reader_with_profile(json.as_slice(), options.profile)?;
        writer.write_all(&json)
    }

    #[cfg(all(feature = "resolver", feature = "validator"))]
    /// Validate both source and emitted graphs using cached caller-provided
    /// references. Output is buffered until all checks succeed.
    pub fn write_as_checked_with_loader<W: io::Write>(
        &self,
        mut writer: W,
        format: CsdlFormat,
        options: &SerializationOptions,
        root_uri: &str,
        loader: &mut impl crate::resolver::DocumentLoader,
        annotation_options: &crate::validator::AnnotationValidationOptions,
    ) -> io::Result<()> {
        let mut loader = CachedLoader {
            inner: loader,
            documents: BTreeMap::new(),
        };
        let original =
            crate::resolver::Resolver::resolve_with_loader(self.clone(), root_uri, &mut loader)
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
        validate_graph(&original, options.profile, annotation_options)?;
        let mut options = options.clone();
        options.source_uri.get_or_insert_with(|| root_uri.into());
        let mut output = Vec::new();
        let mut catalog = Catalog::new(
            self.edmx
                .as_ref()
                .ok_or_else(|| io::Error::other("Missing Edmx"))?,
        );
        for document in loader.documents.values() {
            if let Some(edmx) = &document.edmx {
                catalog.include_document(edmx);
            }
        }
        self.write_with_catalog(&mut output, format, &options, &catalog)?;
        let parsed = match format {
            CsdlFormat::Xml => crate::parser::from_xml_reader(output.as_slice()),
            CsdlFormat::Json => crate::parser::from_json_reader(output.as_slice()),
        }?;
        let resolved =
            crate::resolver::Resolver::resolve_with_loader(parsed, root_uri, &mut loader)
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
        validate_graph(&resolved, options.profile, annotation_options)?;
        writer.write_all(&output)
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
struct CachedLoader<'a, L> {
    inner: &'a mut L,
    documents: BTreeMap<String, CsdlDocument>,
}
#[cfg(all(feature = "resolver", feature = "validator"))]
impl<L: crate::resolver::DocumentLoader> crate::resolver::DocumentLoader for CachedLoader<'_, L> {
    fn load(&mut self, uri: &str) -> Result<CsdlDocument, String> {
        if let Some(doc) = self.documents.get(uri) {
            return Ok(doc.clone());
        }
        let doc = self.inner.load(uri)?;
        self.documents.insert(uri.into(), doc.clone());
        Ok(doc)
    }
    fn inspect_reference(
        &mut self,
        request: crate::resolver::ReferenceRequest<'_>,
    ) -> Result<(), String> {
        self.inner.inspect_reference(request)
    }
}

#[cfg(all(feature = "resolver", feature = "validator"))]
fn validate_graph(
    model: &crate::edm::DocumentModel,
    profile: ValidationProfile,
    options: &crate::validator::AnnotationValidationOptions,
) -> io::Result<()> {
    crate::validator::validate_document(model).map_err(|e| io::Error::other(format!("{e:?}")))?;
    let diagnostics = crate::validator::validate_annotations_with_options(model, options);
    if profile == ValidationProfile::Strict && !diagnostics.is_empty() {
        return Err(io::Error::other(format!("{diagnostics:?}")));
    }
    Ok(())
}

fn rebase(edmx: &mut Edmx, options: &SerializationOptions) -> io::Result<()> {
    let has_base = edmx.base_uri.is_some() || edmx.references.iter().any(|r| r.base_uri.is_some());
    let Some(source) = &options.source_uri else {
        if has_base {
            return Err(io::Error::other(
                "source_uri is required to rebase XML Base",
            ));
        }
        return Ok(());
    };
    let mut base = url::Url::parse(source).map_err(io::Error::other)?;
    if let Some(part) = edmx.base_uri.take() {
        base = base.join(&part).map_err(io::Error::other)?;
    }
    for reference in &mut edmx.references {
        let mut base = base.clone();
        if let Some(part) = reference.base_uri.take() {
            base = base.join(&part).map_err(io::Error::other)?;
        }
        reference.uri = base
            .join(&reference.uri)
            .map_err(io::Error::other)?
            .to_string();
    }
    Ok(())
}

pub(super) struct Catalog {
    aliases: BTreeMap<String, String>,
    preferred: BTreeMap<String, String>,
    terms: BTreeMap<String, Term>,
    types: BTreeMap<String, SchemaElement>,
}
impl Catalog {
    fn nullability(&self, edmx: &mut Edmx, format: CsdlFormat) -> io::Result<()> {
        for schema in &mut edmx.schemas {
            for element in &mut schema.elements {
                let properties = match element {
                    SchemaElement::EntityType(t) => Some(&t.properties),
                    SchemaElement::ComplexType(t) => Some(&t.properties),
                    _ => None,
                };
                if format == CsdlFormat::Json
                    && properties
                        .is_some_and(|p| p.iter().any(|p| p.is_collection && p.nullable.is_none()))
                {
                    return Err(io::Error::other(
                        "JSON cannot preserve unknown collection-property nullability",
                    ));
                }
                let (parameters, return_type) = match element {
                    SchemaElement::Function(f) => (&mut f.parameters, &mut f.return_type),
                    SchemaElement::Action(a) => (&mut a.parameters, &mut a.return_type),
                    _ => continue,
                };
                if format == CsdlFormat::Json {
                    for p in parameters {
                        if p.nullable.is_none() && p.nullable_default.is_none() {
                            p.nullable = Some(true);
                        }
                    }
                }
                if let Some(r) = return_type {
                    let ty = self.canonical(
                        r.type_name.as_deref().unwrap_or("Edm.String"),
                        &schema.namespace,
                    );
                    let entity = ty == "Edm.EntityType"
                        || matches!(self.types.get(&ty), Some(SchemaElement::EntityType(_)));
                    if r.is_collection && !ty.starts_with("Edm.") && !self.types.contains_key(&ty) {
                        return Err(io::Error::other(
                            "Referenced return-type metadata is required for lossless nullability conversion",
                        ));
                    }
                    if r.is_collection && entity {
                        if format == CsdlFormat::Xml {
                            r.nullable_default = None;
                        }
                    } else if format == CsdlFormat::Json
                        && r.nullable.is_none()
                        && r.nullable_default.is_none()
                    {
                        r.nullable = Some(true);
                    }
                }
            }
        }
        Ok(())
    }
    #[cfg(all(feature = "resolver", feature = "validator"))]
    fn include_document(&mut self, edmx: &Edmx) {
        let external = Self::new(edmx);
        for (name, source) in &external.types {
            let ns = name.rsplit_once('.').unwrap().0;
            let mut element = source.clone();
            let normalize = |value: &mut Option<String>| {
                if let Some(t) = value {
                    *t = external.canonical(t, ns);
                }
            };
            match &mut element {
                SchemaElement::Term(t) => {
                    normalize(&mut t.type_name);
                    normalize(&mut t.base_term);
                    self.terms.entry(name.clone()).or_insert_with(|| t.clone());
                }
                SchemaElement::EntityType(t) => {
                    normalize(&mut t.base_type);
                    for p in &mut t.properties {
                        normalize(&mut p.type_name);
                    }
                    for n in &mut t.navigation_properties {
                        normalize(&mut n.type_name);
                    }
                }
                SchemaElement::ComplexType(t) => {
                    normalize(&mut t.base_type);
                    for p in &mut t.properties {
                        normalize(&mut p.type_name);
                    }
                    for n in &mut t.navigation_properties {
                        normalize(&mut n.type_name);
                    }
                }
                _ => {}
            }
            self.types.entry(name.clone()).or_insert(element);
        }
    }
    pub(super) fn new(edmx: &Edmx) -> Self {
        let mut c = Self {
            aliases: BTreeMap::new(),
            preferred: BTreeMap::new(),
            terms: BTreeMap::new(),
            types: BTreeMap::new(),
        };
        for include in edmx.references.iter().flat_map(|r| &r.includes) {
            if let Some(alias) = &include.alias {
                c.aliases.insert(alias.clone(), include.namespace.clone());
                c.preferred.insert(include.namespace.clone(), alias.clone());
            }
        }
        for schema in &edmx.schemas {
            if let Some(alias) = &schema.alias {
                c.aliases.insert(alias.clone(), schema.namespace.clone());
                c.preferred.insert(schema.namespace.clone(), alias.clone());
            }
            for element in &schema.elements {
                let name = match element {
                    SchemaElement::Term(t) => {
                        c.terms
                            .insert(format!("{}.{}", schema.namespace, t.name), t.clone());
                        &t.name
                    }
                    SchemaElement::EntityType(t) => &t.name,
                    SchemaElement::ComplexType(t) => &t.name,
                    SchemaElement::TypeDefinition(t) => &t.name,
                    SchemaElement::EnumType(t) => &t.name,
                    _ => continue,
                };
                c.types
                    .insert(format!("{}.{}", schema.namespace, name), element.clone());
            }
        }
        c
    }
    fn canonical(&self, name: &str, ns: &str) -> String {
        match name.rsplit_once('.') {
            Some((prefix, short)) => format!(
                "{}.{short}",
                self.aliases
                    .get(prefix)
                    .map(String::as_str)
                    .unwrap_or(prefix)
            ),
            None => format!("{ns}.{name}"),
        }
    }
    fn underlying(&self, name: &str) -> String {
        match self.types.get(name) {
            Some(SchemaElement::TypeDefinition(t)) => t.underlying_type.clone(),
            _ => name.into(),
        }
    }
    pub(super) fn xml_annotation(&self, annotation: &mut Annotation, ns: &str) {
        if annotation.source_format == Some(CsdlFormat::Xml) {
            return;
        }
        let term = self.canonical(&annotation.term, ns);
        let expected = self.terms.get(&term).map(|t| {
            self.canonical(
                t.type_name.as_deref().unwrap_or("Edm.String"),
                term.rsplit_once('.').map(|p| p.0).unwrap_or(ns),
            )
        });
        if let Some(e) = &mut annotation.expression {
            self.xml_expression(e, expected.as_deref(), ns);
        }
    }
    fn xml_expression(&self, e: &mut E, expected: Option<&str>, ns: &str) {
        match e {
            E::String(s) if expected.is_some() => {
                let ty = self.underlying(expected.unwrap());
                if ty.starts_with("Edm.") {
                    *e = crate::expr::default_literal(&ty, Some(s));
                } else if let Some(SchemaElement::EnumType(t)) = self.types.get(&ty) {
                    let mut next = Some(0i64);
                    let members = t
                        .members
                        .iter()
                        .filter_map(|m| {
                            let value = m.value.or(next)?;
                            next = value.checked_add(1);
                            Some((m.name.as_str(), value))
                        })
                        .collect::<Vec<_>>();
                    let mut names = Vec::new();
                    for part in s.split(',').map(str::trim) {
                        if members.iter().any(|(name, _)| *name == part) {
                            names.push(part);
                            continue;
                        }
                        let Ok(number) = part.parse::<i64>() else {
                            return;
                        };
                        if let Some((name, _)) = members.iter().find(|(_, n)| *n == number) {
                            names.push(name);
                            continue;
                        }
                        if !t.is_flags.unwrap_or(false) || number < 0 {
                            return;
                        }
                        let mut remaining = number;
                        for (name, n) in &members {
                            if *n > 0 && number & n == *n {
                                names.push(name);
                                remaining &= !n;
                            }
                        }
                        if remaining != 0 || names.is_empty() {
                            return;
                        }
                    }
                    *e = E::EnumMember(
                        names
                            .into_iter()
                            .map(|n| format!("{ty}/{n}"))
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }
            }
            E::Float(n) if expected.is_some_and(|t| self.underlying(t) == "Edm.Decimal") => {
                *e = E::Decimal(crate::expr::float_literal(*n))
            }
            E::Int(n) if expected.is_some_and(|t| self.underlying(t) == "Edm.Decimal") => {
                *e = E::Decimal(n.to_string())
            }
            E::Record {
                type_,
                properties,
                annotations,
            } => {
                let ty = type_
                    .as_ref()
                    .map(|t| self.canonical(t, ns))
                    .or_else(|| expected.map(str::to_owned));
                for p in properties {
                    let expected = ty
                        .as_ref()
                        .and_then(|t| self.property_type(t, &p.property, 0));
                    if let Some(e) = &mut p.value {
                        self.xml_expression(e, expected.as_deref(), ns);
                    }
                    for a in &mut p.annotations {
                        self.xml_annotation(a, ns);
                    }
                }
                for a in annotations {
                    self.xml_annotation(a, ns);
                }
            }
            E::Collection(values) => {
                for e in values {
                    self.xml_expression(e, expected, ns);
                }
            }
            E::Annotated {
                expression,
                annotations,
            } => {
                self.xml_expression(expression, expected, ns);
                for a in annotations {
                    self.xml_annotation(a, ns);
                }
            }
            E::Faceted { expression, .. }
            | E::LabeledElement {
                expr: expression, ..
            } => self.xml_expression(expression, expected, ns),
            E::If { then_, else_, .. } => {
                self.xml_expression(then_, expected, ns);
                if let Some(e) = else_ {
                    self.xml_expression(e, expected, ns);
                }
            }
            E::Cast { type_, expr } => {
                let ty = self.canonical(
                    type_
                        .strip_prefix("Collection(")
                        .and_then(|s| s.strip_suffix(')'))
                        .unwrap_or(type_),
                    ns,
                );
                if matches!(expr.as_ref(), E::String(_)) {
                    self.xml_expression(expr, Some(&ty), ns);
                } else if let E::Collection(items) = expr.as_mut() {
                    for item in items {
                        if matches!(item, E::String(_)) {
                            self.xml_expression(item, Some(&ty), ns);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn preferred(&self, name: &str) -> String {
        name.rsplit_once('.')
            .and_then(|(ns, short)| {
                self.preferred
                    .get(ns)
                    .map(|alias| format!("{alias}.{short}"))
            })
            .unwrap_or_else(|| name.into())
    }
    fn property_type(&self, name: &str, property: &str, depth: usize) -> Option<String> {
        if depth > 64 {
            return None;
        }
        let (properties, navs, base) = match self.types.get(name)? {
            SchemaElement::EntityType(t) => (&t.properties, &t.navigation_properties, &t.base_type),
            SchemaElement::ComplexType(t) => {
                (&t.properties, &t.navigation_properties, &t.base_type)
            }
            _ => return None,
        };
        let ns = name.rsplit_once('.')?.0;
        if let Some(p) = properties.iter().find(|p| p.name == property) {
            return Some(self.canonical(p.type_name.as_deref().unwrap_or("Edm.String"), ns));
        }
        if let Some(p) = navs.iter().find(|p| p.name == property) {
            return p.type_name.as_ref().map(|t| self.canonical(t, ns));
        }
        base.as_ref()
            .and_then(|base| self.property_type(&self.canonical(base, ns), property, depth + 1))
    }
    fn expression(
        &self,
        expression: &E,
        expected: Option<&str>,
        ns: &str,
        options: &SerializationOptions,
    ) -> io::Result<Value> {
        let mut value = super::expr_to_json(expression);
        match expression {
            E::Decimal(s) => {
                value =
                    if options.ieee754_compatible || matches!(s.as_str(), "INF" | "-INF" | "NaN") {
                        Value::String(s.clone())
                    } else {
                        Value::Number(s.parse::<Number>().map_err(io::Error::other)?)
                    }
            }
            E::Int(n)
                if options.ieee754_compatible
                    && expected.is_some_and(|t| self.underlying(t) == "Edm.Int64") =>
            {
                value = Value::String(n.to_string())
            }
            E::String(s)
                if expected.is_some_and(|t| {
                    matches!(self.underlying(t).as_str(), "Edm.Decimal" | "Edm.Int64")
                }) =>
            {
                value =
                    if options.ieee754_compatible || matches!(s.as_str(), "INF" | "-INF" | "NaN") {
                        Value::String(s.clone())
                    } else {
                        Value::Number(s.parse::<Number>().map_err(io::Error::other)?)
                    };
            }
            E::Collection(items) => {
                value = Value::Array(
                    items
                        .iter()
                        .map(|e| self.expression(e, expected, ns, options))
                        .collect::<io::Result<_>>()?,
                )
            }
            E::Record {
                type_,
                properties,
                annotations,
            } => {
                let ty = type_
                    .as_deref()
                    .map(|t| self.canonical(t, ns))
                    .or_else(|| expected.map(str::to_owned));
                let mut map = Map::new();
                if let Some(ty) = ty
                    .as_ref()
                    .filter(|_| type_.is_some() || options.metadata == MetadataLevel::Full)
                {
                    map.insert(
                        "@type".into(),
                        Value::String(format!("#{}", self.preferred(ty))),
                    );
                }
                for p in properties {
                    if let Some(e) = &p.value {
                        let expected = ty
                            .as_ref()
                            .and_then(|t| self.property_type(t, &p.property, 0));
                        map.insert(
                            p.property.clone(),
                            self.expression(e, expected.as_deref(), ns, options)?,
                        );
                        self.control(
                            &mut map,
                            &p.property,
                            expected.as_deref(),
                            matches!(e, E::Collection(_)),
                            options,
                        );
                        self.annotations(&mut map, &p.annotations, ns, options, &p.property)?;
                    }
                }
                self.annotations(&mut map, annotations, ns, options, "")?;
                value = Value::Object(map);
            }
            E::Annotated {
                expression,
                annotations,
            } => {
                value = self.expression(expression, expected, ns, options)?;
                if let Some(map) = value.as_object_mut() {
                    self.annotations(map, annotations, ns, options, "")?;
                }
            }
            E::Faceted { expression, .. } => {
                let inner = self.expression(expression, expected, ns, options)?;
                if let (Some(map), Some(inner)) = (value.as_object_mut(), inner.as_object()) {
                    map.extend(inner.clone());
                }
            }
            E::If { test, then_, else_ } => {
                let mut items = vec![
                    self.expression(test, Some("Edm.Boolean"), ns, options)?,
                    self.expression(then_, expected, ns, options)?,
                ];
                if let Some(e) = else_ {
                    items.push(self.expression(e, expected, ns, options)?);
                }
                value["$If"] = Value::Array(items);
            }
            E::Apply { args, .. } => {
                value["$Apply"] = Value::Array(
                    args.iter()
                        .map(|e| self.expression(e, None, ns, options))
                        .collect::<io::Result<_>>()?,
                )
            }
            E::BinaryExpression { lhs, rhs, .. } => {
                if let Some(map) = value.as_object_mut() {
                    if let Some((_, v)) = map.iter_mut().next() {
                        *v = Value::Array(vec![
                            self.expression(lhs, None, ns, options)?,
                            self.expression(rhs, None, ns, options)?,
                        ]);
                    }
                }
            }
            E::Cast { type_, expr } | E::IsOf { type_, expr } => {
                let key = if matches!(expression, E::Cast { .. }) {
                    "$Cast"
                } else {
                    "$IsOf"
                };
                value[key] = self.expression(
                    expr,
                    Some(
                        &self.canonical(
                            type_
                                .strip_prefix("Collection(")
                                .and_then(|s| s.strip_suffix(')'))
                                .unwrap_or(type_),
                            ns,
                        ),
                    ),
                    ns,
                    options,
                )?;
            }
            E::Not(e) | E::Neg(e) | E::UrlRef(e) | E::LabeledElement { expr: e, .. } => {
                let key = match expression {
                    E::Not(_) => "$Not",
                    E::Neg(_) => "$Neg",
                    E::UrlRef(_) => "$UrlRef",
                    _ => "$LabeledElement",
                };
                let expected = match expression {
                    E::Not(_) => Some("Edm.Boolean"),
                    E::UrlRef(_) => Some("Edm.String"),
                    _ => expected,
                };
                value[key] = self.expression(e, expected, ns, options)?;
            }
            _ => {}
        }
        Ok(value)
    }
    fn control(
        &self,
        map: &mut Map<String, Value>,
        key: &str,
        ty: Option<&str>,
        collection: bool,
        options: &SerializationOptions,
    ) {
        if options.metadata != MetadataLevel::Full {
            return;
        }
        if let Some(ty) = ty.filter(|t| {
            collection
                || !matches!(
                    *t,
                    "Edm.String" | "Edm.Boolean" | "Edm.Int32" | "Edm.Double"
                )
        }) {
            let ty = self.preferred(ty);
            map.insert(
                format!("{key}@type"),
                Value::String(if collection {
                    format!("#Collection({ty})")
                } else {
                    format!("#{ty}")
                }),
            );
        }
    }
    fn annotations(
        &self,
        map: &mut Map<String, Value>,
        annotations: &[Annotation],
        ns: &str,
        options: &SerializationOptions,
        prefix: &str,
    ) -> io::Result<()> {
        for a in annotations {
            let term = self.canonical(&a.term, ns);
            let key = format!(
                "{prefix}{}@{}{}",
                a.target.as_deref().unwrap_or(""),
                self.preferred(&term),
                a.qualifier
                    .as_ref()
                    .map(|q| format!("#{q}"))
                    .unwrap_or_default()
            );
            let expected = self.terms.get(&term).map(|t| {
                self.canonical(
                    t.type_name.as_deref().unwrap_or("Edm.String"),
                    term.rsplit_once('.').map(|p| p.0).unwrap_or(ns),
                )
            });
            if let Some(e) = &a.expression {
                map.insert(
                    key.clone(),
                    self.expression(e, expected.as_deref(), ns, options)?,
                );
            }
            self.control(
                map,
                &key,
                expected.as_deref(),
                self.terms.get(&term).is_some_and(|t| t.is_collection),
                options,
            );
            self.annotations(map, &a.annotations, ns, options, &key)?;
        }
        Ok(())
    }
    fn document(
        &self,
        value: &mut Value,
        edmx: &Edmx,
        options: &SerializationOptions,
    ) -> io::Result<()> {
        for reference in &edmx.references {
            let body = value["$Reference"][&reference.uri]
                .as_object_mut()
                .ok_or_else(|| io::Error::other("Missing reference output"))?;
            self.annotations(body, &reference.annotations, "", options, "")?;
            for (index, include) in reference.includes.iter().enumerate() {
                if let Some(map) = body
                    .get_mut("$Include")
                    .and_then(Value::as_array_mut)
                    .and_then(|a| a.get_mut(index))
                    .and_then(Value::as_object_mut)
                {
                    self.annotations(map, &include.annotations, "", options, "")?;
                }
            }
        }
        for schema in &edmx.schemas {
            let groups = crate::csdl::annotation_groups(schema);
            self.declarations(
                &mut value[&schema.namespace],
                &schema.namespace,
                &schema.namespace,
                &groups,
                options,
            )?;
            if let Some(members) = value[&schema.namespace].as_object_mut() {
                for (name, container) in members {
                    if container["$Kind"] != "EntityContainer" {
                        continue;
                    }
                    let prefixes = [
                        format!("{}.{name}/", schema.namespace),
                        format!(
                            "{}.{name}/",
                            schema.alias.as_deref().unwrap_or(&schema.namespace)
                        ),
                    ];
                    if let Some(children) = container.as_object_mut() {
                        for child in children.values_mut() {
                            if let Some(bindings) = child
                                .get_mut("$NavigationPropertyBinding")
                                .and_then(Value::as_object_mut)
                            {
                                for target in bindings.values_mut() {
                                    if let Some(short) = target.as_str().and_then(|t| {
                                        prefixes.iter().find_map(|p| t.strip_prefix(p))
                                    }) {
                                        *target = Value::String(short.into());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        self.qualify(value, "");
        Ok(())
    }
    fn declarations(
        &self,
        value: &mut Value,
        target: &str,
        ns: &str,
        groups: &[Annotations],
        options: &SerializationOptions,
    ) -> io::Result<()> {
        let Some(map) = value.as_object_mut() else {
            return Ok(());
        };
        if let Some(default) = map.get("$DefaultValue").cloned() {
            let ty = self.canonical(
                map.get("$Type")
                    .and_then(Value::as_str)
                    .unwrap_or("Edm.String"),
                ns,
            );
            if matches!(self.underlying(&ty).as_str(), "Edm.Decimal" | "Edm.Int64") {
                let text = default
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| default.to_string());
                map.insert(
                    "$DefaultValue".into(),
                    self.expression(&E::String(text), Some(&ty), ns, options)?,
                );
            }
        }
        for group in groups.iter().filter(|g| g.target == target) {
            self.annotations(map, &group.annotations, ns, options, "")?;
        }
        for (key, child) in map {
            if key.contains('@') {
                continue;
            }
            if key == "$Annotations" {
                if let Some(groups_json) = child.as_object_mut() {
                    for (path, v) in groups_json {
                        self.declarations(v, path, ns, groups, options)?;
                    }
                }
            } else if key == "$Parameter" {
                if let Some(params) = child.as_array_mut() {
                    for p in params {
                        let name = p["$Name"].as_str().unwrap_or("").to_owned();
                        self.declarations(p, &format!("{target}/{name}"), ns, groups, options)?;
                    }
                }
            } else if key == "$ReturnType" {
                self.declarations(child, &format!("{target}/$ReturnType"), ns, groups, options)?;
            } else if !key.starts_with('$') {
                let path = if target == ns {
                    format!("{ns}.{key}")
                } else {
                    format!("{target}/{key}")
                };
                if let Some(overloads) = child.as_array_mut() {
                    for overload in overloads {
                        let signature = overload["$Parameter"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|p| {
                                let ty = p["$Type"].as_str().unwrap_or("Edm.String");
                                if p["$Collection"] == true {
                                    format!("Collection({ty})")
                                } else {
                                    ty.into()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                        self.declarations(
                            overload,
                            &format!("{path}({signature})"),
                            ns,
                            groups,
                            options,
                        )?;
                    }
                } else {
                    self.declarations(child, &path, ns, groups, options)?;
                }
            }
        }
        Ok(())
    }
    fn qualify(&self, value: &mut Value, owner: &str) {
        if let Some(items) = value.as_array_mut() {
            for item in items {
                self.qualify(item, owner);
            }
        }
        let Some(map) = value.as_object_mut() else {
            return;
        };
        let original = std::mem::take(map);
        for (key, mut value) in original {
            let reference = matches!(
                key.as_str(),
                "$Type"
                    | "$BaseType"
                    | "$BaseTerm"
                    | "$UnderlyingType"
                    | "$Action"
                    | "$Function"
                    | "$Extends"
                    | "$Partner"
                    | "$EntitySetPath"
                    | "$EntitySet"
                    | "$Path"
                    | "$PropertyPath"
                    | "$NavigationPropertyPath"
                    | "$AnnotationPath"
                    | "$ModelElementPath"
                    | "$LabeledElementReference"
                    | "$EnumMember"
            ) || key.ends_with("@type")
                || owner == "$NavigationPropertyBinding";
            if reference {
                if let Some(s) = value.as_str() {
                    value = Value::String(self.reference(s));
                }
            }
            self.qualify(&mut value, &key);
            let key = if key.contains('@')
                || owner == "$Annotations"
                || owner == "$NavigationPropertyBinding"
            {
                self.reference(&key)
            } else {
                key
            };
            map.insert(key, value);
        }
    }
    fn reference(&self, value: &str) -> String {
        let mut out = String::new();
        let mut token = String::new();
        let mut quoted = false;
        for ch in value.chars().chain(std::iter::once('\0')) {
            if ch == '\''
                || !quoted && matches!(ch, '/' | '@' | '(' | ')' | ',' | '#' | '=' | ' ' | '\0')
            {
                out.push_str(&self.preferred(&token));
                token.clear();
                if ch == '\'' {
                    quoted = !quoted;
                }
                if ch != '\0' {
                    out.push(ch);
                }
            } else if quoted {
                out.push(ch);
            } else {
                token.push(ch);
            }
        }
        out
    }
}
