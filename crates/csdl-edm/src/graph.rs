//! Export the resolved EDM semantic model as a node/edge/path graph for the
//! `edm-graph` visualizer.
//!
//! Mapping (see `TODO/graph-export-path-attribute-gaps.md`):
//! - value-attributes  -> dropped
//! - reference-attributes -> edges (labelled with the CSDL attribute name)
//! - path-attributes (`Arc<[*PathSegment]>` fields) -> paths
//!
//! Node identity is by `Arc` pointer: every model element is registered once in
//! a first pass, then references (which hold `Weak`/`Arc`, not names) are mapped
//! back to the node id of the element they point at.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;

use crate::edm::{
    Action, BindingPathSegment, DocumentModel, EntityContainer, EntityContainerElement,
    EntitySetPathSegment, EntityType, Function, KeyPathSegment, Model, NavigationProperty,
    NavigationPropertyBinding, OperationParameter, Property, ResolvedType, SchemaElement, TermType,
    binding_path_to_string, entity_set_path_to_string, key_path_to_string,
};

#[derive(Debug, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub paths: Vec<Path>,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct Segment {
    pub node: String,
    pub order: u32,
}

#[derive(Debug, Serialize)]
pub struct Path {
    pub id: String,
    pub owner: String,
    pub label: String,
    pub segments: Vec<Segment>,
}

/// Build the semantic graph from a resolved document model.
pub fn build_graph(model: &DocumentModel) -> Graph {
    let mut b = Builder::default();
    for schema in &model.schemas {
        b.collect_nodes(schema);
    }
    for schema in &model.schemas {
        b.collect_edges_and_paths(schema);
    }
    Graph {
        nodes: b.nodes,
        edges: b.edges,
        paths: b.paths,
    }
}

fn arc_ptr<T: ?Sized>(a: &Arc<T>) -> usize {
    Arc::as_ptr(a) as *const () as usize
}

#[derive(Default)]
struct Builder {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    paths: Vec<Path>,
    id_by_ptr: HashMap<usize, String>,
    used_node_ids: HashSet<String>,
    used_edge_ids: HashSet<String>,
}

impl Builder {
    /// Register a node, deduplicating its id, and remember the element pointer
    /// so later references can resolve to this node. Returns the final id.
    fn node(&mut self, base_id: String, kind: &str, label: &str, ptr: Option<usize>) -> String {
        let mut id = base_id;
        if self.used_node_ids.contains(&id) {
            let mut n = 2;
            while self.used_node_ids.contains(&format!("{id}~{n}")) {
                n += 1;
            }
            id = format!("{id}~{n}");
        }
        self.used_node_ids.insert(id.clone());
        self.nodes.push(Node {
            id: id.clone(),
            kind: kind.to_string(),
            label: label.to_string(),
        });
        if let Some(p) = ptr {
            self.id_by_ptr.insert(p, id.clone());
        }
        id
    }

    fn edge(&mut self, source: &str, target: &str, kind: &str, label: &str) {
        let id = format!("{source}->{target}:{kind}");
        if !self.used_edge_ids.insert(id.clone()) {
            return;
        }
        self.edges.push(Edge {
            id,
            source: source.to_string(),
            target: target.to_string(),
            kind: kind.to_string(),
            label: label.to_string(),
        });
    }

    fn lookup(&self, ptr: usize) -> Option<String> {
        self.id_by_ptr.get(&ptr).cloned()
    }

    // ---- Pass 1: nodes ----------------------------------------------------

    fn collect_nodes(&mut self, schema: &Arc<Model>) {
        let ns = &schema.namespace;
        let schema_id = self.node(ns.clone(), "Schema", ns, Some(arc_ptr(schema)));

        for el in &schema.elements {
            match el.as_ref() {
                SchemaElement::EntityType(et) => self.collect_structured_nodes(
                    ns,
                    "EntityType",
                    &et.name,
                    arc_ptr(et),
                    et.properties(),
                    et.navigation_properties(),
                ),
                SchemaElement::ComplexType(ct) => self.collect_structured_nodes(
                    ns,
                    "ComplexType",
                    &ct.name,
                    arc_ptr(ct),
                    ct.properties(),
                    ct.navigation_properties(),
                ),
                SchemaElement::EnumType(en) => {
                    let id = self.node(format!("{ns}.{}", en.name), "EnumType", &en.name, Some(arc_ptr(en)));
                    for m in &en.members {
                        self.node(format!("{id}/{}", m.name), "EnumMember", &m.name, Some(arc_ptr(m)));
                    }
                }
                SchemaElement::TypeDefinition(td) => {
                    self.node(format!("{ns}.{}", td.name), "TypeDefinition", &td.name, Some(arc_ptr(td)));
                }
                SchemaElement::Term(t) => {
                    self.node(format!("{ns}.{}", t.name), "Term", &t.name, Some(arc_ptr(t)));
                }
                SchemaElement::Function(f) => {
                    let id = self.node(format!("{ns}.{}", f.name), "Function", &f.name, Some(arc_ptr(f)));
                    self.collect_parameter_nodes(&id, &f.parameters);
                }
                SchemaElement::Action(a) => {
                    let id = self.node(format!("{ns}.{}", a.name), "Action", &a.name, Some(arc_ptr(a)));
                    self.collect_parameter_nodes(&id, &a.parameters);
                }
            }
        }

        for container in schema.containers() {
            let cid = self.node(
                format!("{ns}.{}", container.name),
                "EntityContainer",
                &container.name,
                Some(arc_ptr(container)),
            );
            for el in &container.elements {
                match el.as_ref() {
                    EntityContainerElement::EntitySet(s) => {
                        self.node(format!("{cid}/{}", s.name), "EntitySet", &s.name, Some(arc_ptr(s)));
                    }
                    EntityContainerElement::Singleton(s) => {
                        self.node(format!("{cid}/{}", s.name), "Singleton", &s.name, Some(arc_ptr(s)));
                    }
                    EntityContainerElement::FunctionImport(s) => {
                        self.node(format!("{cid}/{}", s.name), "FunctionImport", &s.name, Some(arc_ptr(s)));
                    }
                    EntityContainerElement::ActionImport(s) => {
                        self.node(format!("{cid}/{}", s.name), "ActionImport", &s.name, Some(arc_ptr(s)));
                    }
                }
            }
        }
        let _ = schema_id;
    }

    fn collect_structured_nodes(
        &mut self,
        ns: &str,
        kind: &str,
        name: &str,
        type_ptr: usize,
        properties: &[Arc<Property>],
        navs: &[Arc<NavigationProperty>],
    ) {
        let id = self.node(format!("{ns}.{name}"), kind, name, Some(type_ptr));
        for p in properties {
            self.node(format!("{id}/{}", p.name), "Property", &p.name, Some(arc_ptr(p)));
        }
        for n in navs {
            self.node(format!("{id}/{}", n.name), "NavigationProperty", &n.name, Some(arc_ptr(n)));
        }
    }

    fn collect_parameter_nodes(&mut self, owner_id: &str, params: &[OperationParameter]) {
        for p in params {
            // Parameters are not `Arc`-shared and are not referenced by any
            // path-attribute, so no pointer registration is needed.
            self.node(format!("{owner_id}/{}", p.name), "Parameter", &p.name, None);
        }
    }

    // ---- Pass 2: edges + paths -------------------------------------------

    fn collect_edges_and_paths(&mut self, schema: &Arc<Model>) {
        let Some(schema_id) = self.lookup(arc_ptr(schema)) else {
            return;
        };

        for el in &schema.elements {
            match el.as_ref() {
                SchemaElement::EntityType(et) => {
                    if let Some(id) = self.lookup(arc_ptr(et)) {
                        self.edge(&schema_id, &id, "Contains", "has");
                        self.structured_edges(&id, et.properties(), et.navigation_properties());
                        self.entity_key_paths(&id, et);
                        self.partner_paths(et.navigation_properties());
                    }
                }
                SchemaElement::ComplexType(ct) => {
                    if let Some(id) = self.lookup(arc_ptr(ct)) {
                        self.edge(&schema_id, &id, "Contains", "has");
                        self.structured_edges(&id, ct.properties(), ct.navigation_properties());
                        self.partner_paths(ct.navigation_properties());
                    }
                }
                SchemaElement::EnumType(en) => {
                    if let Some(id) = self.lookup(arc_ptr(en)) {
                        self.edge(&schema_id, &id, "Contains", "has");
                        for m in &en.members {
                            if let Some(mid) = self.lookup(arc_ptr(m)) {
                                self.edge(&id, &mid, "Declares", "has");
                            }
                        }
                    }
                }
                SchemaElement::TypeDefinition(td) => {
                    if let Some(id) = self.lookup(arc_ptr(td)) {
                        self.edge(&schema_id, &id, "Contains", "has");
                    }
                }
                SchemaElement::Term(t) => {
                    if let Some(id) = self.lookup(arc_ptr(t)) {
                        self.edge(&schema_id, &id, "Contains", "has");
                        if let Some(tt) = t.ty() {
                            if let Some(tid) = self.term_type_id(tt) {
                                self.edge(&id, &tid, "TermType", "Type");
                            }
                        }
                    }
                }
                SchemaElement::Function(f) => {
                    if let Some(id) = self.lookup(arc_ptr(f)) {
                        self.edge(&schema_id, &id, "Contains", "has");
                        self.callable_edges_and_paths(&id, &f.parameters, f.return_type_ty());
                        self.entity_set_path(&id, f.entity_set_path.as_deref(), &f.parameters);
                    }
                }
                SchemaElement::Action(a) => {
                    if let Some(id) = self.lookup(arc_ptr(a)) {
                        self.edge(&schema_id, &id, "Contains", "has");
                        self.callable_edges_and_paths(&id, &a.parameters, a.return_type_ty());
                        self.entity_set_path(&id, a.entity_set_path.as_deref(), &a.parameters);
                    }
                }
            }
        }

        for container in schema.containers() {
            if let Some(cid) = self.lookup(arc_ptr(container)) {
                self.edge(&schema_id, &cid, "Contains", "has");
                self.container_edges_and_paths(&cid, container);
            }
        }
    }

    fn structured_edges(
        &mut self,
        type_id: &str,
        properties: &[Arc<Property>],
        navs: &[Arc<NavigationProperty>],
    ) {
        for p in properties {
            if let Some(pid) = self.lookup(arc_ptr(p)) {
                self.edge(type_id, &pid, "Declares", "has");
                if let Some(tid) = self.resolved_type_id(&p.ty) {
                    self.edge(&pid, &tid, "PropertyType", "Type");
                }
            }
        }
        for n in navs {
            if let Some(nid) = self.lookup(arc_ptr(n)) {
                self.edge(type_id, &nid, "Declares", "has");
                if let Some(target) = n.target.upgrade() {
                    if let Some(tid) = self.lookup(arc_ptr(&target)) {
                        self.edge(&nid, &tid, "NavigationTarget", "Type");
                    }
                }
            }
        }
    }

    fn callable_edges_and_paths(
        &mut self,
        owner_id: &str,
        params: &[OperationParameter],
        return_ty: Option<&TermType>,
    ) {
        for p in params {
            let pid = format!("{owner_id}/{}", p.name);
            self.edge(owner_id, &pid, "Declares", "has");
            if let Some(tid) = self.term_type_id(&p.ty) {
                self.edge(&pid, &tid, "ParameterType", "Type");
            }
        }
        if let Some(rt) = return_ty {
            if let Some(tid) = self.term_type_id(rt) {
                self.edge(owner_id, &tid, "ReturnType", "ReturnType");
            }
        }
    }

    fn container_edges_and_paths(&mut self, cid: &str, container: &Arc<EntityContainer>) {
        for el in &container.elements {
            match el.as_ref() {
                EntityContainerElement::EntitySet(s) => {
                    if let Some(id) = self.lookup(arc_ptr(s)) {
                        self.edge(cid, &id, "Contains", "has");
                        if let Some(tid) = self.lookup(arc_ptr(&s.target)) {
                            self.edge(&id, &tid, "EntityType", "EntityType");
                        }
                        self.binding_paths(&id, &s.target, s.navigation_property_bindings(), cid);
                    }
                }
                EntityContainerElement::Singleton(s) => {
                    if let Some(id) = self.lookup(arc_ptr(s)) {
                        self.edge(cid, &id, "Contains", "has");
                        if let Some(tid) = self.lookup(arc_ptr(&s.target)) {
                            self.edge(&id, &tid, "SingletonType", "Type");
                        }
                        self.binding_paths(&id, &s.target, s.navigation_property_bindings(), cid);
                    }
                }
                EntityContainerElement::FunctionImport(s) => {
                    if let Some(id) = self.lookup(arc_ptr(s)) {
                        self.edge(cid, &id, "Contains", "has");
                        self.import_entity_set_path(&id, s.entity_set.as_deref(), cid);
                    }
                }
                EntityContainerElement::ActionImport(s) => {
                    if let Some(id) = self.lookup(arc_ptr(s)) {
                        self.edge(cid, &id, "Contains", "has");
                        self.import_entity_set_path(&id, s.entity_set.as_deref(), cid);
                    }
                }
            }
        }
    }

    // ---- Paths ------------------------------------------------------------

    /// One path per entity key; threads anchor -> property -> (complex type) -> ...
    fn entity_key_paths(&mut self, entity_id: &str, entity: &Arc<EntityType>) {
        for (i, key) in entity.keys().iter().enumerate() {
            let mut nodes = vec![entity_id.to_string()];
            for seg in key.iter() {
                if let KeyPathSegment::NavigationProperty(w) = seg {
                    if let Some(nav) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&nav)) { nodes.push(id); }
                        if let Some(target) = nav.target.upgrade() {
                            if let Some(id) = self.lookup(arc_ptr(&target)) { nodes.push(id); }
                        }
                    }
                }
                if let KeyPathSegment::Property(w) = seg {
                    if let Some(p) = w.upgrade() {
                        if let Some(pid) = self.lookup(arc_ptr(&p)) {
                            nodes.push(pid);
                            if let Some(tid) = self.resolved_type_id(&p.ty) {
                                nodes.push(tid);
                            }
                        }
                    }
                }
            }
            self.push_path(
                format!("{entity_id}/key/{i}"),
                entity_id,
                &format!("Key: {}", key_path_to_string(key)),
                nodes,
            );
        }
    }

    fn partner_paths(&mut self, navs: &[Arc<NavigationProperty>]) {
        for n in navs {
            let Some(partner) = n.partner() else { continue };
            let Some(nav_id) = self.lookup(arc_ptr(n)) else {
                continue;
            };
            let Some(target) = n.target.upgrade() else {
                continue;
            };
            let Some(anchor) = self.lookup(arc_ptr(&target)) else {
                continue;
            };
            let mut nodes = vec![anchor];
            self.append_binding_segments(&mut nodes, partner);
            self.push_path(
                format!("{nav_id}/partner"),
                &nav_id,
                &format!("Partner: {}", binding_path_to_string(partner)),
                nodes,
            );
        }
    }

    fn binding_paths(
        &mut self,
        owner_id: &str,
        bound_type: &Arc<EntityType>,
        bindings: &[NavigationPropertyBinding],
        container_id: &str,
    ) {
        for (i, binding) in bindings.iter().enumerate() {
            if let Some(anchor) = self.lookup(arc_ptr(bound_type)) {
                let mut nodes = vec![anchor];
                self.append_binding_segments(&mut nodes, &binding.path);
                self.push_path(
                    format!("{owner_id}/binding/{i}/path"),
                    owner_id,
                    &format!("Binding path: {}", binding_path_to_string(&binding.path)),
                    nodes,
                );
            }
            let mut tnodes = vec![container_id.to_string()];
            self.append_binding_segments(&mut tnodes, &binding.target);
            self.push_path(
                format!("{owner_id}/binding/{i}/target"),
                owner_id,
                &format!("Binding target: {}", binding_path_to_string(&binding.target)),
                tnodes,
            );
        }
    }

    fn import_entity_set_path(
        &mut self,
        import_id: &str,
        entity_set: Option<&[BindingPathSegment]>,
        container_id: &str,
    ) {
        let Some(segs) = entity_set else { return };
        let mut nodes = vec![container_id.to_string()];
        self.append_binding_segments(&mut nodes, segs);
        self.push_path(
            format!("{import_id}/entityset"),
            import_id,
            &format!("EntitySet: {}", binding_path_to_string(segs)),
            nodes,
        );
    }

    fn entity_set_path(
        &mut self,
        owner_id: &str,
        segs: Option<&[EntitySetPathSegment]>,
        params: &[OperationParameter],
    ) {
        let Some(segs) = segs else { return };
        let mut nodes = Vec::new();
        for seg in segs {
            match seg {
                EntitySetPathSegment::BindingParameter(name) => {
                    // Anchor at the binding parameter's entity type, if any.
                    if let Some(p) = params.iter().find(|p| &p.name == name) {
                        if let TermType::Entity(e) = &p.ty {
                            if let Some(id) = self.lookup(arc_ptr(e)) {
                                nodes.push(id);
                            }
                        }
                    }
                }
                EntitySetPathSegment::NavigationProperty(w) => {
                    if let Some(n) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&n)) {
                            nodes.push(id);
                            if let Some(t) = n.target.upgrade() {
                                if let Some(tid) = self.lookup(arc_ptr(&t)) {
                                    nodes.push(tid);
                                }
                            }
                        }
                    }
                }
                EntitySetPathSegment::Property(w) => {
                    if let Some(p) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&p)) {
                            nodes.push(id);
                            if let Some(tid) = self.resolved_type_id(&p.ty) {
                                nodes.push(tid);
                            }
                        }
                    }
                }
                EntitySetPathSegment::EntityTypeCast(w) => {
                    if let Some(ty) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&ty)) {
                            nodes.push(id);
                        }
                    }
                }
                EntitySetPathSegment::Unresolved(_) => {}
            }
        }
        self.push_path(
            format!("{owner_id}/entitysetpath"),
            owner_id,
            &format!("EntitySetPath: {}", entity_set_path_to_string(segs)),
            nodes,
        );
    }

    /// Append binding-path segments, threading implicit type nodes where the
    /// segment kind makes the connector unambiguous (property -> its type,
    /// navigation/entity-set -> its target entity). Best-effort: unresolved
    /// segments and casts are appended as-is without a connector.
    fn append_binding_segments(&mut self, nodes: &mut Vec<String>, segs: &[BindingPathSegment]) {
        for seg in segs {
            match seg {
                BindingPathSegment::Property(w) => {
                    if let Some(p) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&p)) {
                            nodes.push(id);
                            if let Some(tid) = self.resolved_type_id(&p.ty) {
                                nodes.push(tid);
                            }
                        }
                    }
                }
                BindingPathSegment::NavigationProperty(w) => {
                    if let Some(n) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&n)) {
                            nodes.push(id);
                            if let Some(t) = n.target.upgrade() {
                                if let Some(tid) = self.lookup(arc_ptr(&t)) {
                                    nodes.push(tid);
                                }
                            }
                        }
                    }
                }
                BindingPathSegment::EntitySet(w) => {
                    if let Some(s) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&s)) {
                            nodes.push(id);
                            if let Some(tid) = self.lookup(arc_ptr(&s.target)) {
                                nodes.push(tid);
                            }
                        }
                    }
                }
                BindingPathSegment::Singleton(w) => {
                    if let Some(s) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&s)) {
                            nodes.push(id);
                            if let Some(tid) = self.lookup(arc_ptr(&s.target)) {
                                nodes.push(tid);
                            }
                        }
                    }
                }
                BindingPathSegment::EntityTypeCast(w) => {
                    if let Some(t) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&t)) {
                            nodes.push(id);
                        }
                    }
                }
                BindingPathSegment::ComplexTypeCast(w) => {
                    if let Some(t) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&t)) {
                            nodes.push(id);
                        }
                    }
                }
                BindingPathSegment::EntityContainer(w) => {
                    if let Some(c) = w.upgrade() {
                        if let Some(id) = self.lookup(arc_ptr(&c)) {
                            nodes.push(id);
                        }
                    }
                }
                BindingPathSegment::Unresolved(_) => {}
            }
        }
    }

    /// Emit a path only if it threads at least one edge (>= 2 nodes), dropping
    /// consecutive duplicate node ids that arise when a named segment and the
    /// implicit connector coincide.
    fn push_path(&mut self, id: String, owner: &str, label: &str, nodes: Vec<String>) {
        let mut deduped: Vec<String> = Vec::with_capacity(nodes.len());
        for n in nodes {
            if deduped.last() != Some(&n) {
                deduped.push(n);
            }
        }
        if deduped.len() < 2 {
            return;
        }
        let segments = deduped
            .into_iter()
            .enumerate()
            .map(|(i, node)| Segment {
                node,
                order: i as u32 + 1,
            })
            .collect();
        self.paths.push(Path {
            id,
            owner: owner.to_string(),
            label: label.to_string(),
            segments,
        });
    }

    // ---- Type-node lookups -----------------------------------------------

    fn resolved_type_id(&self, ty: &ResolvedType) -> Option<String> {
        match ty {
            ResolvedType::Complex(a) => self.lookup(arc_ptr(a)),
            ResolvedType::Enum(a) => self.lookup(arc_ptr(a)),
            ResolvedType::TypeDefinition(a) => self.lookup(arc_ptr(a)),
            ResolvedType::Primitive(_) => None,
        }
    }

    fn term_type_id(&self, ty: &TermType) -> Option<String> {
        match ty {
            TermType::Complex(a) => self.lookup(arc_ptr(a)),
            TermType::Enum(a) => self.lookup(arc_ptr(a)),
            TermType::TypeDefinition(a) => self.lookup(arc_ptr(a)),
            TermType::Entity(a) => self.lookup(arc_ptr(a)),
            TermType::Primitive(_) => None,
        }
    }
}

// Small accessors so the builder can read return types uniformly. These mirror
// the existing accessor style on the EDM types.
trait ReturnTy {
    fn return_type_ty(&self) -> Option<&TermType>;
}
impl ReturnTy for Function {
    fn return_type_ty(&self) -> Option<&TermType> {
        self.return_type.as_ref().map(|r| &r.ty)
    }
}
impl ReturnTy for Action {
    fn return_type_ty(&self) -> Option<&TermType> {
        self.return_type.as_ref().map(|r| &r.ty)
    }
}
