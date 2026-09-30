use super::{ResolveError, Resolver};
use crate::{csdl::*, edm::DocumentModel, expr::CsdlAnnotationExpression as E};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use url::Url;

/// Resolves document identities; the caller chooses storage/network policy.
pub trait DocumentLoader {
    fn load(&mut self, absolute_uri: &str) -> Result<CsdlDocument, String>;

    /// Called for every reference, including cache hits. Callers can inspect
    /// version hints, configure retrieval, or reject conflicting requirements.
    /// The resolver neither rewrites the URI nor performs network requests.
    fn inspect_reference(&mut self, _request: ReferenceRequest<'_>) -> Result<(), String> {
        Ok(())
    }
}

pub struct ReferenceRequest<'a> {
    pub absolute_uri: &'a str,
    pub reference: &'a Reference,
    /// Constant Core.SchemaVersion value, with document-local aliases resolved.
    pub schema_version: Option<&'a str>,
}

fn schema_version<'a>(reference: &'a Reference, document: &Edmx) -> Option<&'a str> {
    let mut aliases = super::collect_document_aliases(&document.references);
    aliases.extend(
        document
            .schemas
            .iter()
            .filter_map(|s| s.alias.as_ref().map(|a| (a.clone(), s.namespace.clone()))),
    );
    reference.annotations.iter().find_map(|a| {
        let term = super::qualify_name(&a.term, "", &aliases)?;
        if term != "Org.OData.Core.V1.SchemaVersion" || a.qualifier.is_some() {
            return None;
        }
        match &a.expression {
            Some(E::String(value)) => Some(value.as_str()),
            _ => None,
        }
    })
}

impl<F> DocumentLoader for F
where
    F: FnMut(&str) -> Result<CsdlDocument, String>,
{
    fn load(&mut self, uri: &str) -> Result<CsdlDocument, String> {
        self(uri)
    }
}

#[derive(Default)]
pub struct DocumentRegistry {
    documents: BTreeMap<String, CsdlDocument>,
}

impl DocumentRegistry {
    pub fn insert(&mut self, uri: &str, document: CsdlDocument) -> Result<(), ResolveError> {
        self.documents.insert(absolute(uri)?.to_string(), document);
        Ok(())
    }
}

impl DocumentLoader for DocumentRegistry {
    fn load(&mut self, uri: &str) -> Result<CsdlDocument, String> {
        self.documents
            .get(uri)
            .cloned()
            .ok_or_else(|| format!("Document is not registered: {uri}"))
    }
}

fn error(uri: &str, reason: impl Into<String>) -> ResolveError {
    ResolveError::ReferenceDocument {
        uri: uri.into(),
        reason: reason.into(),
    }
}
fn absolute(uri: &str) -> Result<Url, ResolveError> {
    Url::parse(uri).map_err(|e| error(uri, e.to_string()))
}
fn relative(base: &str, uri: &str) -> Result<String, ResolveError> {
    absolute(base)?
        .join(uri)
        .map(|u| u.to_string())
        .map_err(|e| error(uri, e.to_string()))
}

fn reference_uri(uri: &str, doc: &Edmx, reference: &Reference) -> Result<String, ResolveError> {
    let mut base = uri.to_owned();
    for part in [doc.base_uri.as_ref(), reference.base_uri.as_ref()]
        .into_iter()
        .flatten()
    {
        base = relative(&base, part)?;
    }
    relative(&base, &reference.uri)
}

impl Resolver {
    /// Resolve a document graph with document-local alias scopes. References
    /// are cached before traversal, so reference cycles do not recurse forever.
    pub fn resolve_with_loader(
        root: CsdlDocument,
        root_uri: &str,
        loader: &mut impl DocumentLoader,
    ) -> Result<DocumentModel, ResolveError> {
        let root_uri = absolute(root_uri)?.to_string();
        let root = root.edmx.ok_or_else(|| error(&root_uri, "Missing Edmx"))?;
        let mut docs = BTreeMap::from([(root_uri.clone(), root.clone())]);
        let mut pending = VecDeque::from([root_uri.clone()]);
        while let Some(uri) = pending.pop_front() {
            let references = docs[&uri].references.clone();
            let mut seen = BTreeSet::new();
            for reference in references {
                let target = reference_uri(&uri, &docs[&uri], &reference)?;
                if !seen.insert(target.clone()) {
                    return Err(error(&uri, "Duplicate reference URI"));
                }
                loader
                    .inspect_reference(ReferenceRequest {
                        absolute_uri: &target,
                        reference: &reference,
                        schema_version: schema_version(&reference, &docs[&uri]),
                    })
                    .map_err(|e| error(&target, e))?;
                if !docs.contains_key(&target) {
                    let doc = loader
                        .load(&target)
                        .map_err(|e| error(&target, e))?
                        .edmx
                        .ok_or_else(|| error(&target, "Missing Edmx"))?;
                    docs.insert(target.clone(), doc);
                    pending.push_back(target);
                }
            }
        }
        let scopes = docs
            .iter()
            .map(|(uri, doc)| Scope::new(uri, doc, &docs).map(|s| (uri.clone(), s)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut selected = BTreeMap::<String, (String, Schema)>::new();
        let mut pending = VecDeque::new();
        pending.push_back((root_uri.clone(), String::new()));
        let mut full_schemas = BTreeSet::new();
        for schema in &root.schemas {
            full_schemas.insert((root_uri.clone(), schema.namespace.clone()));
            pending.push_back((root_uri.clone(), schema.namespace.clone()));
        }
        let mut visited = BTreeSet::new();
        let mut processed_docs = BTreeSet::new();
        let mut external = Vec::new();
        while let Some((uri, ns)) = pending.pop_front() {
            if !visited.insert((uri.clone(), ns.clone())) {
                continue;
            }
            let doc = &docs[&uri];
            let scope = &scopes[&uri];
            if !ns.is_empty() {
                let mut schema = doc
                    .schemas
                    .iter()
                    .find(|s| s.namespace == ns)
                    .cloned()
                    .ok_or_else(|| error(&uri, format!("Included namespace is missing: {ns}")))?;
                let mut dependencies = BTreeSet::new();
                canonical_schema(&mut schema, scope, &mut dependencies)?;
                if let Some((owner, _)) = selected.get(&ns) {
                    if owner != &uri {
                        return Err(error(
                            &uri,
                            format!("Namespace {ns} is defined by multiple documents"),
                        ));
                    }
                } else {
                    selected.insert(ns.clone(), (uri.clone(), schema));
                }
                for dependency in dependencies {
                    if dependency == "Edm" {
                        continue;
                    }
                    let owner = scope.namespaces.get(&dependency).ok_or_else(|| {
                        error(&uri, format!("Namespace is not in scope: {dependency}"))
                    })?;
                    pending.push_back((owner.clone(), dependency));
                }
            }
            if processed_docs.insert(uri.clone()) {
                for reference in &doc.references {
                    let target = reference_uri(&uri, doc, reference)?;
                    for include in &reference.includes {
                        full_schemas.insert((target.clone(), include.namespace.clone()));
                        pending.push_back((target.clone(), include.namespace.clone()));
                    }
                    for filter in &reference.include_annotations {
                        if let Some(owner) = scopes[&target].namespaces.get(&filter.term_namespace)
                        {
                            pending.push_back((owner.clone(), filter.term_namespace.clone()));
                        }
                        for source in &docs[&target].schemas {
                            if reference
                                .includes
                                .iter()
                                .any(|i| i.namespace == source.namespace)
                            {
                                continue;
                            }
                            for group in annotation_groups(source) {
                                let mut group = group;
                                let target_scope = &scopes[&target];
                                group.target = target_scope.path(&group.target);
                                group
                                    .annotations
                                    .iter_mut()
                                    .for_each(|a| target_scope.annotation(a));
                                group.annotations.retain(|a| {
                                    a.term
                                        .rsplit_once('.')
                                        .is_some_and(|(ns, _)| ns == filter.term_namespace)
                                        && filter
                                            .qualifier
                                            .as_ref()
                                            .is_none_or(|q| a.qualifier.as_ref() == Some(q))
                                        && filter.target_namespace.as_ref().is_none_or(|ns| {
                                            group
                                                .target
                                                .strip_prefix(ns)
                                                .is_some_and(|s| s.starts_with('.'))
                                        })
                                });
                                if !group.annotations.is_empty() && !external.contains(&group) {
                                    external.push(group);
                                }
                            }
                        }
                    }
                }
            }
        }
        let local: BTreeSet<_> = root.schemas.iter().map(|s| s.namespace.clone()).collect();
        let schema_scopes = selected
            .iter()
            .map(|(ns, (uri, _))| {
                let mut names = scopes[uri]
                    .namespaces
                    .keys()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                for reference in &docs[uri].references {
                    names.extend(
                        reference
                            .include_annotations
                            .iter()
                            .map(|a| a.term_namespace.clone()),
                    );
                }
                (ns.clone(), names)
            })
            .collect::<BTreeMap<_, _>>();
        let aliases = selected
            .iter()
            .map(|(ns, (uri, _))| {
                let alias = docs[uri]
                    .schemas
                    .iter()
                    .find(|s| &s.namespace == ns)
                    .and_then(|s| s.alias.clone());
                (ns.clone(), alias)
            })
            .collect::<BTreeMap<_, _>>();
        let versions = selected
            .iter()
            .map(|(ns, (uri, _))| {
                (
                    ns.clone(),
                    docs[uri].version.clone().unwrap_or_else(|| "4.01".into()),
                )
            })
            .collect();
        let mut schemas = selected
            .into_values()
            .map(|(uri, mut s)| {
                if !full_schemas.contains(&(uri, s.namespace.clone())) {
                    s.external_annotations.clear();
                }
                s
            })
            .collect::<Vec<_>>();
        if !external.is_empty() {
            let index = schemas
                .iter()
                .position(|s| local.contains(&s.namespace))
                .unwrap_or(0);
            let host = schemas
                .get_mut(index)
                .ok_or(ResolveError::NoSchemaInDocument)?;
            host.external_annotations.extend(external);
        }
        let original_references = root.references.clone();
        let mut result = Self::resolve_document_scoped(
            Edmx {
                base_uri: None,
                entity_container: root.entity_container,
                version: root.version,
                references: vec![],
                schemas,
            },
            Some(&schema_scopes),
            Some(versions),
        )?;
        for schema in &mut result.schemas {
            if let Some(model) = std::sync::Arc::get_mut(schema) {
                model.alias = aliases[&model.namespace].clone();
            }
        }
        let (mut local_schemas, imported): (Vec<_>, Vec<_>) = result
            .schemas
            .into_iter()
            .partition(|s| local.contains(&s.namespace));
        local_schemas.sort_by_key(|s| {
            root.schemas
                .iter()
                .position(|original| original.namespace == s.namespace)
        });
        result.schemas = local_schemas;
        result.referenced_schemas = imported;
        let root_scope = &scopes[&root_uri];
        let aliases = root_scope
            .aliases
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        result.references = super::annotations::references(
            &original_references,
            result
                .all_schemas()
                .filter(|m| root_scope.namespaces.contains_key(&m.namespace))
                .map(std::sync::Arc::as_ref),
            &aliases,
        );
        Ok(result)
    }
}

pub(super) fn annotation_groups(schema: &Schema) -> Vec<Annotations> {
    fn add(out: &mut Vec<Annotations>, path: &str, annotations: &[Annotation]) {
        for annotation in annotations {
            let mut annotation = annotation.clone();
            let target = annotation
                .target
                .take()
                .map(|p| format!("{path}/{p}"))
                .unwrap_or_else(|| path.into());
            out.push(Annotations {
                target,
                annotations: vec![annotation],
            });
        }
    }
    fn members(
        out: &mut Vec<Annotations>,
        path: &str,
        properties: &[Property],
        navs: &[NavigationProperty],
    ) {
        for p in properties {
            add(out, &format!("{path}/{}", p.name), &p.annotations);
        }
        for n in navs {
            let path = format!("{path}/{}", n.name);
            add(out, &path, &n.annotations);
            for c in &n.referential_constraints {
                add(out, &format!("{path}/{}", c.property), &c.annotations);
            }
        }
    }
    fn callable(
        out: &mut Vec<Annotations>,
        path: &str,
        params: &[Parameter],
        ret: &Option<ReturnType>,
        annotations: &[Annotation],
    ) {
        let signature = params
            .iter()
            .map(|p| {
                let ty = p.type_name.as_deref().unwrap_or("Edm.String");
                if p.is_collection {
                    format!("Collection({ty})")
                } else {
                    ty.into()
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let path = format!("{path}({signature})");
        add(out, &path, annotations);
        for p in params {
            add(out, &format!("{path}/{}", p.name), &p.annotations);
        }
        if let Some(r) = ret {
            add(out, &format!("{path}/$ReturnType"), &r.annotations);
        }
    }
    let mut out = schema.external_annotations.clone();
    add(&mut out, &schema.namespace, &schema.annotations);
    for e in &schema.elements {
        match e {
            SchemaElement::EntityType(t) => {
                let path = format!("{}.{}", schema.namespace, t.name);
                add(&mut out, &path, &t.annotations);
                members(&mut out, &path, &t.properties, &t.navigation_properties);
            }
            SchemaElement::ComplexType(t) => {
                let path = format!("{}.{}", schema.namespace, t.name);
                add(&mut out, &path, &t.annotations);
                members(&mut out, &path, &t.properties, &t.navigation_properties);
            }
            SchemaElement::EnumType(t) => {
                let path = format!("{}.{}", schema.namespace, t.name);
                add(&mut out, &path, &t.annotations);
                for m in &t.members {
                    add(&mut out, &format!("{path}/{}", m.name), &m.annotations);
                }
            }
            SchemaElement::TypeDefinition(t) => add(
                &mut out,
                &format!("{}.{}", schema.namespace, t.name),
                &t.annotations,
            ),
            SchemaElement::Term(t) => add(
                &mut out,
                &format!("{}.{}", schema.namespace, t.name),
                &t.annotations,
            ),
            SchemaElement::Function(f) => callable(
                &mut out,
                &format!("{}.{}", schema.namespace, f.name),
                &f.parameters,
                &f.return_type,
                &f.annotations,
            ),
            SchemaElement::Action(f) => callable(
                &mut out,
                &format!("{}.{}", schema.namespace, f.name),
                &f.parameters,
                &f.return_type,
                &f.annotations,
            ),
            SchemaElement::EntityContainer(c) => {
                let path = format!("{}.{}", schema.namespace, c.name);
                add(&mut out, &path, &c.annotations);
                for s in &c.entity_sets {
                    add(&mut out, &format!("{path}/{}", s.name), &s.annotations);
                }
                for s in &c.singletons {
                    add(&mut out, &format!("{path}/{}", s.name), &s.annotations);
                }
                for s in &c.function_imports {
                    add(&mut out, &format!("{path}/{}", s.name), &s.annotations);
                }
                for s in &c.action_imports {
                    add(&mut out, &format!("{path}/{}", s.name), &s.annotations);
                }
            }
        }
    }
    out
}

struct Scope {
    uri: String,
    aliases: BTreeMap<String, String>,
    namespaces: BTreeMap<String, String>,
}

impl Scope {
    fn new(uri: &str, doc: &Edmx, docs: &BTreeMap<String, Edmx>) -> Result<Self, ResolveError> {
        let mut scope = Self {
            uri: uri.into(),
            aliases: BTreeMap::new(),
            namespaces: BTreeMap::new(),
        };
        for schema in &doc.schemas {
            scope.add(&schema.namespace, schema.alias.as_deref(), uri)?;
        }
        for reference in &doc.references {
            let target = reference_uri(uri, doc, reference)?;
            for include in &reference.includes {
                if !docs[&target]
                    .schemas
                    .iter()
                    .any(|s| s.namespace == include.namespace)
                {
                    return Err(error(
                        uri,
                        format!("{} does not define {}", target, include.namespace),
                    ));
                }
                scope.add(&include.namespace, include.alias.as_deref(), &target)?;
            }
        }
        if scope
            .aliases
            .keys()
            .any(|a| scope.namespaces.contains_key(a))
        {
            return Err(error(uri, "Alias conflicts with a namespace"));
        }
        Ok(scope)
    }
    fn add(&mut self, ns: &str, alias: Option<&str>, owner: &str) -> Result<(), ResolveError> {
        if self.namespaces.insert(ns.into(), owner.into()).is_some() {
            return Err(error(&self.uri, format!("Duplicate namespace: {ns}")));
        }
        if let Some(alias) = alias {
            if matches!(alias, "Edm" | "odata" | "System" | "Transient")
                || self.aliases.insert(alias.into(), ns.into()).is_some()
            {
                return Err(error(&self.uri, format!("Conflicting alias: {alias}")));
            }
        }
        Ok(())
    }
    fn path(&self, value: &str) -> String {
        let mut result = String::new();
        let mut token = String::new();
        let mut quoted = false;
        let flush = |token: &mut String, result: &mut String| {
            if let Some((prefix, rest)) = token.split_once('.') {
                if let Some(ns) = self.aliases.get(prefix) {
                    result.push_str(ns);
                    result.push('.');
                    result.push_str(rest);
                    token.clear();
                    return;
                }
            }
            result.push_str(token);
            token.clear();
        };
        for ch in value.chars() {
            if ch == '\'' {
                flush(&mut token, &mut result);
                quoted = !quoted;
                result.push(ch);
            } else if quoted {
                result.push(ch);
            } else if matches!(ch, '/' | '@' | '(' | ')' | ',' | ' ' | '#' | '=') {
                flush(&mut token, &mut result);
                result.push(ch);
            } else {
                token.push(ch);
            }
        }
        flush(&mut token, &mut result);
        result
    }
    fn ty(
        &self,
        value: &mut Option<String>,
        ns: &str,
        deps: &mut BTreeSet<String>,
    ) -> Result<(), ResolveError> {
        if let Some(value) = value {
            *value = self.path(value);
            if !value.contains('.') {
                *value = format!("{ns}.{value}");
            }
            let namespace = value.rsplit_once('.').unwrap().0;
            if namespace != "Edm" && !self.namespaces.contains_key(namespace) {
                return Err(error(
                    &self.uri,
                    format!("Namespace is not in scope: {namespace}"),
                ));
            }
            deps.insert(namespace.into());
        }
        Ok(())
    }
    fn structural_path(&self, value: &mut String) -> Result<(), ResolveError> {
        *value = self.path(value);
        for segment in value.split(['/', '@', '(', ')', ',', '#']) {
            if let Some((namespace, _)) = segment.rsplit_once('.') {
                if namespace != "Edm" && !self.namespaces.contains_key(namespace) {
                    return Err(error(
                        &self.uri,
                        format!("Namespace is not in scope: {namespace}"),
                    ));
                }
            }
        }
        Ok(())
    }
    fn annotation(&self, a: &mut Annotation) {
        self.annotations(&mut a.annotations);
        a.term = self.path(&a.term);
        if let Some(target) = &mut a.target {
            *target = self.path(target);
        }
        if let Some(e) = &mut a.expression {
            self.expr(e);
        }
    }
    fn annotations(&self, values: &mut [Annotation]) {
        for a in values {
            self.annotation(a);
        }
    }
    fn expr(&self, e: &mut E) {
        match e {
            E::Annotated {
                expression,
                annotations,
            } => {
                self.expr(expression);
                self.annotations(annotations);
            }
            E::Faceted { expression, .. } | E::Neg(expression) => self.expr(expression),
            E::ModelElementPath(s) => *s = self.path(s),
            E::Path(s)
            | E::PropertyPath(s)
            | E::NavigationPropertyPath(s)
            | E::AnnotationPath(s)
            | E::EnumMember(s)
            | E::LabeledElementReference(s) => *s = self.path(s),
            E::Cast { type_, expr } | E::IsOf { type_, expr } => {
                *type_ = self.path(type_);
                self.expr(expr);
            }
            E::Apply { function, args } => {
                *function = self.path(function);
                for e in args {
                    self.expr(e);
                }
            }
            E::Not(e) | E::UrlRef(e) | E::LabeledElement { expr: e, .. } => self.expr(e),
            E::BinaryExpression { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            E::If { test, then_, else_ } => {
                self.expr(test);
                self.expr(then_);
                if let Some(e) = else_ {
                    self.expr(e);
                }
            }
            E::Collection(es) => {
                for e in es {
                    self.expr(e);
                }
            }
            E::Record {
                type_,
                properties,
                annotations,
            } => {
                if let Some(t) = type_ {
                    *t = self.path(t);
                }
                self.annotations(annotations);
                for p in properties {
                    if let Some(e) = &mut p.value {
                        self.expr(e);
                    }
                    self.annotations(&mut p.annotations);
                }
            }
            _ => {}
        }
    }
}

fn canonical_schema(
    schema: &mut Schema,
    scope: &Scope,
    deps: &mut BTreeSet<String>,
) -> Result<(), ResolveError> {
    let ns = schema.namespace.clone();
    schema.alias = None;
    scope.annotations(&mut schema.annotations);
    for group in &mut schema.external_annotations {
        group.target = scope.path(&group.target);
        scope.annotations(&mut group.annotations);
    }
    for element in &mut schema.elements {
        match element {
            SchemaElement::EntityType(t) => {
                scope.ty(&mut t.base_type, &ns, deps)?;
                if let Some(key) = &mut t.key {
                    for property in &mut key.property_refs {
                        scope.structural_path(&mut property.name)?;
                    }
                }
                members(
                    &mut t.properties,
                    &mut t.navigation_properties,
                    scope,
                    &ns,
                    deps,
                )?;
                scope.annotations(&mut t.annotations);
            }
            SchemaElement::ComplexType(t) => {
                scope.ty(&mut t.base_type, &ns, deps)?;
                members(
                    &mut t.properties,
                    &mut t.navigation_properties,
                    scope,
                    &ns,
                    deps,
                )?;
                scope.annotations(&mut t.annotations);
            }
            SchemaElement::EnumType(t) => {
                scope.ty(&mut t.underlying_type, &ns, deps)?;
                scope.annotations(&mut t.annotations);
                for m in &mut t.members {
                    scope.annotations(&mut m.annotations);
                }
            }
            SchemaElement::TypeDefinition(t) => {
                let mut ty = Some(t.underlying_type.clone());
                scope.ty(&mut ty, &ns, deps)?;
                t.underlying_type = ty.unwrap();
                scope.annotations(&mut t.annotations);
            }
            SchemaElement::Term(t) => {
                scope.ty(&mut t.type_name, &ns, deps)?;
                scope.ty(&mut t.base_term, &ns, deps)?;
                scope.annotations(&mut t.annotations);
            }
            SchemaElement::Action(t) => {
                callable(&mut t.parameters, &mut t.return_type, scope, &ns, deps)?;
                if let Some(p) = &mut t.entity_set_path {
                    scope.structural_path(p)?;
                }
                scope.annotations(&mut t.annotations);
            }
            SchemaElement::Function(t) => {
                callable(&mut t.parameters, &mut t.return_type, scope, &ns, deps)?;
                if let Some(p) = &mut t.entity_set_path {
                    scope.structural_path(p)?;
                }
                scope.annotations(&mut t.annotations);
            }
            SchemaElement::EntityContainer(c) => {
                scope.ty(&mut c.extends, &ns, deps)?;
                scope.annotations(&mut c.annotations);
                for s in &mut c.entity_sets {
                    scope.ty(&mut s.entity_type, &ns, deps)?;
                    scope.annotations(&mut s.annotations);
                    for b in &mut s.navigation_property_bindings {
                        scope.structural_path(&mut b.path)?;
                        scope.structural_path(&mut b.target)?;
                    }
                }
                for s in &mut c.singletons {
                    scope.ty(&mut s.type_name, &ns, deps)?;
                    scope.annotations(&mut s.annotations);
                    for b in &mut s.navigation_property_bindings {
                        scope.structural_path(&mut b.path)?;
                        scope.structural_path(&mut b.target)?;
                    }
                }
                for i in &mut c.function_imports {
                    scope.ty(&mut i.function, &ns, deps)?;
                    scope.annotations(&mut i.annotations);
                    if let Some(p) = &mut i.entity_set {
                        scope.structural_path(p)?;
                    }
                }
                for i in &mut c.action_imports {
                    scope.ty(&mut i.action, &ns, deps)?;
                    scope.annotations(&mut i.annotations);
                    if let Some(p) = &mut i.entity_set {
                        scope.structural_path(p)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn members(
    properties: &mut [Property],
    navs: &mut [NavigationProperty],
    scope: &Scope,
    ns: &str,
    deps: &mut BTreeSet<String>,
) -> Result<(), ResolveError> {
    for p in properties {
        scope.ty(&mut p.type_name, ns, deps)?;
        scope.annotations(&mut p.annotations);
    }
    for n in navs {
        scope.ty(&mut n.type_name, ns, deps)?;
        scope.annotations(&mut n.annotations);
        if let Some(p) = &mut n.partner {
            scope.structural_path(p)?;
        }
        for c in &mut n.referential_constraints {
            scope.structural_path(&mut c.property)?;
            scope.structural_path(&mut c.referenced_property)?;
            scope.annotations(&mut c.annotations);
        }
    }
    Ok(())
}
fn callable(
    parameters: &mut [Parameter],
    ret: &mut Option<ReturnType>,
    scope: &Scope,
    ns: &str,
    deps: &mut BTreeSet<String>,
) -> Result<(), ResolveError> {
    for p in parameters {
        scope.ty(&mut p.type_name, ns, deps)?;
        scope.annotations(&mut p.annotations);
    }
    if let Some(r) = ret {
        scope.ty(&mut r.type_name, ns, deps)?;
        scope.annotations(&mut r.annotations);
    }
    Ok(())
}
