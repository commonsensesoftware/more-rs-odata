# Path Attribute Resolution Architecture

## Purpose

Define the architecture for how CSDL path-like attributes are handled in
`csdl-edm`: resolved in resolver, stored as typed semantic values in EDM, and
consumed by validator and service-side projections without re-parsing strings.

`NavigationPropertyBinding.Path` and `NavigationPropertyBinding.Target` are
reference implementations of this architecture, not special cases.

## Architectural Boundary

Path handling is split into three explicit phases:

1. Resolve phase (resolver)
2. Store phase (EDM semantic model)
3. Consume phase (validator and service)

Rules:

- Resolver is the only component that interprets path syntax and binds segment
  references.
- EDM stores typed segment/references for semantic use.
- Validator and service consume typed values only; they do not split or
  re-resolve path strings.

This preserves separation of concerns and avoids duplicate path logic.

## Path Attribute Inventory

Implementation status: navigation binding paths/targets, key paths, partners,
referential constraints, operation entity-set paths, import entity-set targets,
and annotation targets/expression paths are now resolved into typed EDM values.
Their validators consume these values. Raw authored strings remain available
for compatibility and diagnostics; removing them would require a separate API
compatibility decision.

Annotation targets may intentionally retain unresolved segments: the explicit
annotation diagnostic API reports them without rejecting the document.
Resolver errors for structural paths still vary by domain; the uniform
segment-index/context diagnostic format described below remains a follow-up.

The architecture applies to all path-like attributes, including at least:

- `NavigationPropertyBinding.Path`
- `NavigationPropertyBinding.Target`
- `PropertyRef.Name` (key path)
- `ReferentialConstraint.Property`
- `ReferentialConstraint.ReferencedProperty`
- `NavigationProperty.Partner`
- `Action.EntitySetPath`
- `Function.EntitySetPath`
- `ActionImport.EntitySet` (name-or-path union)
- `FunctionImport.EntitySet` (name-or-path union)
- `Annotations.Target`
- `Annotation.Path`

Each attribute/domain defines its own legal segment kinds and traversal
constraints, but all use the same lifecycle.

## Resolve Phase (Resolver)

Resolver responsibilities:

1. Parse raw attribute text into syntactic segments.
2. Resolve segments against current semantic context.
3. Produce typed segment arrays in semantic order.
4. Emit diagnostics at resolver time for invalid path syntax/semantics.

Resolver diagnostics should carry:

- attribute identity (for example `Path`, `Target`, `EntitySetPath`)
- source context (container/type/member + source attribute owner)
- failing segment index and source text
- reason category (unknown member, invalid cast, invalid traversal, invalid
  start segment, etc.)

## Store Phase (EDM)

EDM stores resolved path values as typed immutable sequences.

Recommended representation:

- `Arc<[Segment]>` for shared immutable segment sequences
- domain-specific segment enums per path domain

Compatibility path during migration:

- Keep raw string fields temporarily for backward compatibility.
- Mark raw string fields transitional.
- Preserve raw fields while the public API remains additive.

## Consume Phase (Validator and Service)

Consumer responsibilities:

- Validator checks semantic constraints over typed segments only.
- Service projections/router assembly consume typed semantic values; no path
  string parsing in service crate.

Consumer non-responsibilities:

- no `split('/')`
- no type-cast parsing
- no ad-hoc symbol lookup from raw path strings

## Ownership and Memory Model

Use `Arc<[Segment]>` (or `Vec<Segment>` as migration step) for ordered path
values.

Why `Arc + Vec/slice` is correct:

- A path is an immutable value sequence, not an independently linked graph
  node.
- The segment collection itself does not create reference cycles.
- Segment variants may hold `Arc` references to existing EDM nodes; this does
  not add reverse ownership to the segment array.
- Existing cycle-prone graph links continue to use `Weak` where appropriate.

Why not `Vec<Weak<PathSegment>>` / `Weak<Vec<PathSegment>>`:

- Weak segment ownership models paths as optional/disappearing graph nodes,
  which is wrong for deterministic semantic values.
- It adds pervasive `upgrade()` handling with no architectural gain.

## Reference Implementation: NavigationPropertyBinding

`NavigationPropertyBinding.Path` and `NavigationPropertyBinding.Target` should
be implemented first because they exercise multiple segment classes and
traversal constraints.

Example segment domains:

- `NavigationBindingPathSegment`
  - `TypeCastEntity(Arc<EntityType>)`
  - `TypeCastComplex(Arc<ComplexType>)`
  - `ComplexProperty(Arc<Property>)`
  - `NavigationProperty(Arc<NavigationProperty>)`

- `NavigationBindingTargetSegment`
  - `ContainerEntitySet(Arc<EntitySet>)`
  - `ContainerSingleton(Arc<Singleton>)`
  - `ComplexProperty(Arc<Property>)`
  - `ContainmentNavigation(Arc<NavigationProperty>)`

After this reference implementation is stable, apply the same architecture to
remaining path domains in prioritized follow-up items.

## Migration Plan

1. Introduce typed segment enums and resolved fields for
   `NavigationPropertyBinding`.
2. Populate resolved segments in resolver while keeping raw strings
   transitional.
3. Switch validator navigation-binding rules to resolved segments.
4. Add resolver diagnostics tests for segment index/context/reason reporting.
5. Remove navigation-binding string parsing from validator.
6. Repeat by path domain (`EntitySetPath`, key/reference paths,
   annotation/model-target paths).
7. Retain authored strings for compatibility; consider removal only through
   an explicit public API compatibility decision.

## Test Strategy

Add tests per path domain for:

- valid resolution shape (segment kinds + order)
- invalid segment diagnostics (index + reason category)
- invalid traversal diagnostics
- domain-specific rules (for example containment ordering, singleton start,
  terminal constraints)
- consumer behavior over typed segments (validator/service) without string
  parsing

## Scope Notes

In scope:

- architecture and migration path for CSDL path attributes in `csdl-edm`

Out of scope:

- URL/query parsing (`url` crate)
- service-only concepts not derived from EDM
- unrelated inheritance/type-system redesign
