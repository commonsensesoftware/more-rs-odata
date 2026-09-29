# The EDM Semantic Graph

> Tables in this document are generated from
> [`csdl-attribute-catalog.toml`](./csdl-attribute-catalog.toml) by
> `scripts/sync_attribute_catalog.py`. Prose is authored by hand. Do not edit the
> regions between `<!-- GENERATED:… -->` markers.

## Introduction & motivation

This document and its source catalog describe the CSDL specification model.
For the crate's delivered behavior and remaining limits, see the
[csdl-edm coverage record](../crates/csdl-edm/TODO/csdl-coverage-gaps.md).
The resolver now materializes typed structural and annotation paths; annotation
authoring diagnostics are documented separately in
[annotation validation](../crates/csdl-edm/docs/annotation-validation.md).
The [feature policy](../crates/csdl-edm/docs/features.md) describes optional
semantic-model, resolver, and validator compilation.

A CSDL document — the XML or JSON you author to describe an OData service — is a
**syntactic tree**. Resolving it (binding every symbolic name to the element it
denotes) yields the **EDM**, which is best understood not as a tree but as a
**semantic graph**: the model's elements are _nodes_, the links between them are
_edges_, and certain attributes describe ordered _paths_ through that graph.

That graph is valuable **as information in its own right**, independent of any
drawing of it. A directed graph of nodes, edges, and paths is a precise,
queryable mathematical structure, and most questions one actually asks of a model
are graph questions:

- _Dependency / impact analysis_ — what references this type; what breaks if I
  remove it; what is reachable from this entity set.
- _Validation_ — does every reference resolve; are there cycles where there
  should be none; do key and binding paths land on legal targets.
- _Diffing_ — how did the graph change between two versions of a model.
- _Documentation and exploration_ — including, as **one consumer among many**,
  the interactive visualizer in `apps/edm-graph`.

This document defines the graph precisely: what becomes a node, what becomes an
edge, and what becomes a path — by classifying every CSDL **attribute** into one
of three categories. That classification currently exists only as tacit knowledge
and in code (the resolver's typed path segments and the graph exporter in
`crates/csdl-edm/src/graph.rs`). It is written down here so the EDM semantic graph
can be reasoned about and built consistently by any consumer.

## Terminology

This project keeps two word-pairs strictly separated and **never crosses them**:

| Concept            | Artifact                     | Shape                        |
| ------------------ | ---------------------------- | ---------------------------- |
| **syntactic tree** | **CSDL** document (XML/JSON) | nested elements & attributes |
| **semantic graph** | **EDM** (after resolution)   | nodes, edges, paths          |

So it is the _EDM_ semantic graph and the _CSDL_ syntactic tree — never a "CSDL
semantic graph." Resolution is the bridge from one to the other.

Two more conventions:

- **"attribute"** is used in the XML sense (the `Type` _attribute_ of a `Property`
  _element_). This avoids awkwardly mixing model and meta-model vocabulary and
  keeps a single word for the thing being classified.
- **"attribute categories"** (not "classes" — `class` is overloaded).

_Semantic graph_ is an established term: compilers speak of an **abstract
semantic graph** (an AST whose name references have been resolved into edges), and
data modeling / knowledge graphs use it for the same idea — entities linked by
typed relationships. The EDM is exactly this for an OData model.

On _meta-model_: CSDL genuinely **is** the meta-model for OData service models —
it is the language in which service models are written. This document does not
claim to be that meta-model; it describes one **facet** of it: how a model's
attributes project into the semantic graph.

## The three attribute categories

Every attribute of a CSDL element falls into exactly one category. The decision
rule:

1. **value** — carries scalar or structured _data_ with no symbolic link to
   another element (`Nullable`, `MaxLength`, `Abstract`, `DefaultValue`, an
   element's own `Name`). Carries no graph information.
2. **reference** — names _exactly one_ model element (`Type`, `BaseType`,
   `EntityType`, `Function`). Resolves to a single target.
3. **path** — an ordered `a/b/c` sequence of named elements, each connected to the
   previous by an _implicit_ reference (`PropertyRef.Name`, `Partner`,
   `NavigationPropertyBinding.Path`, `EntitySetPath`). Resolves to a walk.

Separately, the syntactic tree's **containment** (parent/child _has-a_ nesting —
a `ComplexType` _has_ `Property`s) is itself a reference relationship. It is not
an attribute, but it produces edges too — and is in fact the **majority** of
edges.

## Projection to the EDM semantic graph

| Source              | EDM role    | Notes                                                    |
| ------------------- | ----------- | -------------------------------------------------------- |
| element             | **node**    | one node per model element                               |
| containment (has-a) | **edge**    | labelled `has`; the bulk of edges                        |
| reference attribute | **edge**    | labelled with the attribute name (`Type`, `BaseType`, …) |
| path attribute      | **path**    | an ordered walk that rides existing edges                |
| value attribute     | **dropped** | no graph information. Could be added to property graphs  |

The catalog stores only the **category**; the graph role above is _derived_ from
it (value→drop, reference→edge, path→path, containment→edge) — fully determined,
which is why role is not stored.

Refinements:

- **Primitive types are intrinsic nodes.** `Edm.String`, `Edm.Int32`, … are nodes
  of the semantic graph even though they are not authored in CSDL — they are
  built-in and "just there." A reference to one (`Type`, `UnderlyingType`) is an
  ordinary edge, so every reference resolves to a node uniformly (ergonomic for an
  API over the graph). A consumer **may elide** primitive nodes to reduce clutter
  — the visualizer does — but that is a _presentation filter_, not a property of
  the graph.
- **Paths thread implicit nodes.** A path `a/b` is really
  `X →(has) a →(Type) A →(has) b`: only the named segments (`a`, `b`) appear in
  CSDL; the connecting types (`A`) are implicit but are real nodes, so the path
  passes through them and rides existing edges rather than cutting across the
  graph.
- **Inheritance is flattened.** The resolved EDM folds `BaseType` members into the
  derived type; path threading follows base types where needed.
- **Collection wrappers decompose.** `Collection(…)` is not a construct of its
  own — it is the wire _syntax_ of a `Type` attribute (e.g.
  `Type="Collection(Edm.String)"`). It splits into two independent facts: the
  **reference edge is just to the wrapped type** (`Edm.String`), identical to a
  non-collection `Type="Edm.String"`; and **collection-ness becomes a synthetic
  value attribute** of the owning element (the reader normalizes it to an
  `is_collection` flag — `$Collection` in CSDL JSON). As a value attribute it
  carries no graph information and drops. So `Type="Collection(X)"` and
  `Type="X"` yield the same edge, differing only in that synthetic flag.
- **Name-or-path unions** (`FunctionImport.EntitySet`) are treated as paths.

## Containment relationships

Structural `has-a` edges of the EDM semantic graph (rendered `has`).

<!-- GENERATED:containment -->
| Parent | Child | Edge label |
| --- | --- | --- |
| Edmx | Reference | has |
|  | DataServices | has |
| DataServices | Schema | has |
| Reference | Include | has |
|  | IncludeAnnotations | has |
| Schema | EntityType | has |
|  | ComplexType | has |
|  | EnumType | has |
|  | TypeDefinition | has |
|  | Term | has |
|  | Function | has |
|  | Action | has |
|  | EntityContainer | has |
|  | Annotations | has |
| EntityType | Key | has |
|  | Property | has |
|  | NavigationProperty | has |
| ComplexType | Property | has |
|  | NavigationProperty | has |
| EnumType | Member | has |
| Key | PropertyRef | has |
| NavigationProperty | ReferentialConstraint | has |
|  | OnDelete | has |
| Function | Parameter | has |
|  | ReturnType | has |
| Action | Parameter | has |
|  | ReturnType | has |
| EntityContainer | EntitySet | has |
|  | Singleton | has |
|  | FunctionImport | has |
|  | ActionImport | has |
| EntitySet | NavigationPropertyBinding | has |
| Singleton | NavigationPropertyBinding | has |
| Annotations | Annotation | has |
<!-- END:containment -->

## Reference attributes

Each table row below names one target element and becomes an edge labelled with the attribute name.
(Targets include intrinsic primitive type nodes, so this is uniform — no exceptions.)

<!-- GENERATED:attributes-reference -->
| Element | Attribute | Card. | Target / Segment | Notes |
| --- | --- | --- | --- | --- |
| EntityType | BaseType | optional | EntityType |  |
| ComplexType | BaseType | optional | ComplexType |  |
| EnumType | UnderlyingType | optional | PrimitiveType | target is an Edm integral primitive (an intrinsic node) |
| TypeDefinition | UnderlyingType | required | PrimitiveType | target is an Edm primitive (an intrinsic node) |
| Term | Type | required | PrimitiveType, EnumType, ComplexType, EntityType, TypeDefinition | may be wrapped in Collection(...) |
|  | BaseTerm | optional | Term |  |
| Property | Type | required | PrimitiveType, EnumType, ComplexType, TypeDefinition | may be wrapped in Collection(...) |
| NavigationProperty | Type | required | EntityType | may be wrapped in Collection(...) |
| Parameter | Type | required | PrimitiveType, EnumType, ComplexType, EntityType, TypeDefinition | may be wrapped in Collection(...) |
| ReturnType | Type | required | PrimitiveType, EnumType, ComplexType, EntityType, TypeDefinition | may be wrapped in Collection(...) |
| EntityContainer | Extends | optional | EntityContainer |  |
| EntitySet | EntityType | required | EntityType |  |
| Singleton | Type | required | EntityType |  |
| FunctionImport | Function | required | Function |  |
| ActionImport | Action | required | Action |  |
| Annotation | Term | required | Term |  |
<!-- END:attributes-reference -->

## Path attributes

The most elaborate part of the model: ordered walks through the graph. The **segment domain** column in the table below
described what type of nodes the path can walk through .

_Note: Entries marked `not yet typed` are are not yet specified in this doc nor implemented in code and carried as raw strings (see
`crates/csdl-edm/TODO/path-attribute-resolution-architecture.md`); they are path
attributes by definition and are expected to become typed segments_

_Note: see the resolver's typed segment enum for their usage `crates/csdl-edm/src/edm.rs`_

<!-- GENERATED:attributes-path -->
| Element | Attribute | Card. | Target / Segment | Notes |
| --- | --- | --- | --- | --- |
| PropertyRef | Name | required | KeyPathSegment | terminal: `PrimitiveType`; descent: `ComplexType`. an entity key |
| NavigationProperty | Partner | optional | BindingPathSegment | terminal: `NavigationProperty`; descent: `ComplexType` |
| ReferentialConstraint | Property | required | (not yet typed: String) | terminal: `PrimitiveType`; descent: `ComplexType`. resolver gap — stored as String today |
|  | ReferencedProperty | required | (not yet typed: String) | terminal: `PrimitiveType`; descent: `ComplexType`. resolver gap — stored as String today |
| Function | EntitySetPath | optional | EntitySetPathSegment | head: `BindingParameter` |
| Action | EntitySetPath | optional | EntitySetPathSegment | head: `BindingParameter` |
| NavigationPropertyBinding | Path | required | BindingPathSegment | terminal: `NavigationProperty`; descent: `ComplexType` |
|  | Target | required | BindingPathSegment | head: `EntityContainer`; terminal: `EntitySet`, `Singleton` |
| FunctionImport | EntitySet | optional | BindingPathSegment | terminal: `EntitySet` |
| ActionImport | EntitySet | optional | BindingPathSegment | terminal: `EntitySet` |
| Annotations | Target | required | (not yet typed: String) | model-element path; resolver gap |
| Annotation | Target | optional | (not yet typed: String) | Annotations.Target / external-targeting path; resolver gap |
| Annotation.expression | Path | optional | (not yet typed: String) | dynamic path expression; resolver gap |
|  | PropertyPath | optional | (not yet typed: String) | terminal: `PrimitiveType`; descent: `ComplexType`. resolver gap |
|  | NavigationPropertyPath | optional | (not yet typed: String) | terminal: `NavigationProperty`; descent: `ComplexType`. resolver gap |
|  | AnnotationPath | optional | (not yet typed: String) | terminal: `Annotation`. resolver gap |
|  | ModelElementPath | optional | (not yet typed: String) | spec construct; resolver gap |
<!-- END:attributes-path -->

### Segment domains

Each `Target / Segment` cell above names a _segment domain_ — the set of
node kinds a single segment in that path may resolve to.

<!-- GENERATED:segment-domains -->
- **KeyPathSegment** — `Property`. intrinsic to a single entity type; descends through ComplexType-valued structural properties, terminates at a primitive. Navigation properties are deliberately excluded — keys must be properties of the entity itself, not of related entities (contrast BindingPathSegment, which is the domain that traverses navigation).
- **BindingPathSegment** — `Property` | `NavigationProperty` | `EntityTypeCast` | `ComplexTypeCast` | `EntitySet` | `Singleton` | `EntityContainer`. used by NavigationPropertyBinding.Path/Target and NavigationProperty.Partner; head is a container element in `target` form, properties/navs in `path` form.
- **EntitySetPathSegment** — `BindingParameter` | `NavigationProperty` | `Property`. Function.EntitySetPath / Action.EntitySetPath; starts at the binding parameter.
<!-- END:segment-domains -->

## Value attributes

Data-only; they carry no graph information and contribute nothing to nodes,
edges, or paths. Listed for completeness.

<!-- GENERATED:attributes-value -->
| Element | Attribute | Card. | Type | Notes |
| --- | --- | --- | --- | --- |
| Edmx | Version | required | `string` | CSDL/OData version string, e.g. "4.01" |
| Reference | Uri | required | `string` | absolute URI of the referenced CSDL document |
| Include | Namespace | required | `namespace` | namespace to import from the referenced document |
|  | Alias | optional | `identifier` |  |
| IncludeAnnotations | TermNamespace | required | `namespace` |  |
|  | TargetNamespace | optional | `namespace` |  |
|  | Qualifier | optional | `identifier` |  |
| Schema | Namespace | required | `namespace` | identity of the schema node |
|  | Alias | optional | `identifier` |  |
| EntityType | Name | required | `identifier` | identity |
|  | Abstract | optional | `boolean` |  |
|  | OpenType | optional | `boolean` |  |
|  | HasStream | optional | `boolean` |  |
| PropertyRef | Alias | optional | `identifier` | renames the key path for use in URLs |
| ComplexType | Name | required | `identifier` |  |
|  | Abstract | optional | `boolean` |  |
|  | OpenType | optional | `boolean` |  |
| EnumType | Name | required | `identifier` |  |
|  | IsFlags | optional | `boolean` |  |
| Member | Name | required | `identifier` |  |
|  | Value | optional | `int64` |  |
| TypeDefinition | Name | required | `identifier` |  |
|  | MaxLength | optional | `(integer > 0) \| 'max'` |  |
|  | Precision | optional | `integer >= 0` |  |
|  | Scale | optional | `(integer >= 0) \| 'floating' \| 'variable'` |  |
|  | SRID | optional | `(integer >= 0) \| 'variable'` |  |
|  | Unicode | optional | `boolean` |  |
| Term | Name | required | `identifier` |  |
|  | AppliesTo | optional | `string` | a set of target-kind identifiers, not a symbolic link |
|  | DefaultValue | optional | `string` |  |
|  | Nullable | optional | `boolean` |  |
|  | MaxLength | optional | `(integer > 0) \| 'max'` |  |
|  | Precision | optional | `integer >= 0` |  |
|  | Scale | optional | `(integer >= 0) \| 'floating' \| 'variable'` |  |
|  | SRID | optional | `(integer >= 0) \| 'variable'` |  |
|  | Unicode | optional | `boolean` |  |
| Property | Name | required | `identifier` |  |
|  | Nullable | optional | `boolean` |  |
|  | MaxLength | optional | `(integer > 0) \| 'max'` |  |
|  | Precision | optional | `integer >= 0` |  |
|  | Scale | optional | `(integer >= 0) \| 'floating' \| 'variable'` |  |
|  | SRID | optional | `(integer >= 0) \| 'variable'` |  |
|  | Unicode | optional | `boolean` |  |
|  | DefaultValue | optional | `string` |  |
| NavigationProperty | Name | required | `identifier` |  |
|  | Nullable | optional | `boolean` |  |
|  | ContainsTarget | optional | `boolean` |  |
| OnDelete | Action | required | `'Cascade' \| 'None' \| 'SetNull' \| 'SetDefault'` |  |
| Function | Name | required | `identifier` |  |
|  | IsBound | optional | `boolean` |  |
|  | IsComposable | optional | `boolean` |  |
| Action | Name | required | `identifier` |  |
|  | IsBound | optional | `boolean` |  |
| Parameter | Name | required | `identifier` |  |
|  | Nullable | optional | `boolean` |  |
|  | MaxLength | optional | `(integer > 0) \| 'max'` |  |
|  | Precision | optional | `integer >= 0` |  |
|  | Scale | optional | `(integer >= 0) \| 'floating' \| 'variable'` |  |
|  | SRID | optional | `(integer >= 0) \| 'variable'` |  |
|  | Unicode | optional | `boolean` |  |
|  | DefaultValue | optional | `string` |  |
| ReturnType | Nullable | optional | `boolean` |  |
|  | MaxLength | optional | `(integer > 0) \| 'max'` |  |
|  | Precision | optional | `integer >= 0` |  |
|  | Scale | optional | `(integer >= 0) \| 'floating' \| 'variable'` |  |
|  | SRID | optional | `(integer >= 0) \| 'variable'` |  |
|  | Unicode | optional | `boolean` |  |
| EntityContainer | Name | required | `identifier` |  |
| EntitySet | Name | required | `identifier` |  |
|  | IncludeInServiceDocument | optional | `boolean` |  |
| Singleton | Name | required | `identifier` |  |
|  | IncludeInServiceDocument | optional | `boolean` |  |
| FunctionImport | Name | required | `identifier` |  |
|  | IncludeInServiceDocument | optional | `boolean` |  |
| ActionImport | Name | required | `identifier` |  |
|  | IncludeInServiceDocument | optional | `boolean` |  |
| Annotations | Qualifier | optional | `identifier` |  |
| Annotation | Qualifier | optional | `identifier` |  |
<!-- END:attributes-value -->

## References

- OData CSDL XML v4.01 —
  <https://docs.oasis-open.org/odata/odata-csdl-xml/v4.01/odata-csdl-xml-v4.01.html>
- OData CSDL JSON v4.01 —
  <https://docs.oasis-open.org/odata/odata-csdl-json/v4.01/odata-csdl-json-v4.01.html>
