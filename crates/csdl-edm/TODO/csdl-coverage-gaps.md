# CSDL Coverage Gaps

This is the authoritative implementation backlog for remaining CSDL 4.01
coverage work.

Current implemented pipeline:

- reader (JSON/XML) -> parser -> CSDL model -> resolver -> EDM model -> validator

Notes:

- `src/csdl.rs` is now the canonical CSDL model.
- `src/model.rs` and `src/conversion.rs` were removed.

Status legend used below:

- `done`: implemented in current crate.
- `partial`: represented, but not fully resolved and/or validated.
- `missing`: not yet supported in resolver/validation flow.

## Coverage Matrix Snapshot

| Element Group          | Model Element (4.01)                                      | parse+serialize | resolver | validation | Notes                                                                                                                                                                      |
| ---------------------- | --------------------------------------------------------- | --------------- | -------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Schema Core            | Edmx / Schema                                             | done            | done     | partial    | Canonical CSDL model (`src/csdl.rs`) resolves to EDM model.                                                                                                                |
| Schema Core            | Reference / Include / IncludeAnnotations                  | done            | done     | partial    | Alias wiring exists; deeper semantic checks are minimal.                                                                                                                   |
| Type Definitions       | EntityType / ComplexType / EnumType                       | done            | partial  | partial    | BaseType member inheritance is partially supported, and the resolver now rejects derived member redeclarations; full inheritance semantics remain.                         |
| Type Definitions       | TypeDefinition / Term                                     | done            | partial  | partial    | Term resolution includes collection-valued terms, and term base-term cycle validation is now covered; broader inheritance semantics remain.                                |
| Structural Members     | Property / NavigationProperty / Key                       | done            | partial  | partial    | Core resolution works (including collection-valued structural properties); PropertyRef key paths are resolved from owning entity context; nav semantics remain incomplete. |
| Navigation Constraints | ReferentialConstraint / OnDelete / Partner / Containment  | done            | partial  | missing    | Resolver maps fields, including typed `OnDelete.Action` and `ContainsTarget` passthrough; deeper containment/navigation semantics remain.                                  |
| Operations             | Function / Action / Parameter / ReturnType                | done            | partial  | missing    | Resolver maps operation signatures and now rejects Function without ReturnType; deeper operation semantics and validation are still pending.                               |
| Container              | EntityContainer / EntitySet / Singleton                   | done            | done     | partial    | Core container target resolution works.                                                                                                                                    |
| Container              | FunctionImport / ActionImport / NavigationPropertyBinding | done            | partial  | missing    | Resolver now maps imports and nav bindings and validates function/action import targets; deeper target/operation semantics remain.                                         |
| Annotations            | Annotation attachment forms                               | done            | missing  | missing    | Round-trip support; semantic consumption/validation not implemented.                                                                                                       |

## Gap Summary

1. Resolver semantic gaps:
   Inheritance, nav constraints/containment/partner, operations, imports,
   bindings, container inheritance, collection-valued terms.
2. Validation depth gaps:
   Only baseline uniqueness/key checks are implemented; most semantic checklist
   rules are pending.
3. Test coverage gaps:
   Need systematic fixture-backed end-to-end and expected-failure coverage.

Related detailed design TODO:

- [path-attribute-resolution-architecture](path-attribute-resolution-architecture.md)
  Resolver-owned typed/resolved path architecture for CSDL path-like
  attributes, with `NavigationPropertyBinding` as a reference implementation.

## 1. Resolver Semantics Gaps

- [x] Complete inheritance semantics (`EntityType.BaseType`,
      `ComplexType.BaseType`): document-scope base identity, inherited member visibility,
      effective keys, cycle/wrong-kind rejection, CSDL 4.01 subtype narrowing,
      open/abstract/media inheritance checks, and empty inherited-key redeclaration
      rejection are implemented with XML/JSON regression coverage. Cross-schema
      bases use a shared qualified-name registry, including forward aliases;
      version-specific 4.0 key and member-redefinition rules are implemented.
- [x] Navigation semantics for named entity targets: typed partner paths (including
      derived casts), reciprocal/type checks, complex-owner restrictions, recursive
      and non-recursive containment, typed structural/navigation constraint paths,
      type/cardinality/nullability checks, and strict OnDelete parsing. XML/JSON
      nullability defaults and Int32 key serialization are preserved. Built-in
      abstract targets remain part of primitive/type coverage below.
- [x] Operations: `Function`, `Action`, binding/composability, parameter uniqueness,
      function/action overload rules, consistent overload returns, entity-return
      nullability, and typed EntitySetPath navigation/casts with return compatibility.
      JSON default types/nullability and malformed declaration checks are covered.
      General type/facet coverage remains tracked in sections 2 and 3.
- [x] Container features: imports retain resolved unbound operations; import return
      compatibility and service-document inclusion are validated. Binding paths
      support entity/complex casts, containment and complex traversal, duplicate
      detection, and compatible targets across document containers/schemas.
- [x] Entity container inheritance (`EntityContainer.Extends`) across document
      schemas, with cycle checks, shared inherited resources, subtype narrowing,
      and all declared containers retained by `Model::containers()`.
- [x] Collection-valued term support.

## 2. Validation Gaps

- [x] Promote key checklist items from
      `docs/Semantic Validation Checklist.md` into executable rules: allowed types,
      non-nullable paths, aliases, non-empty declarations, and versioned related-entity keys.
- [ ] Add inheritance-related validation (base-type cycles, key compatibility,
      member consistency) (key compatibility is now enforced in resolver; broader validation remains).
- [ ] Add navigation validation (partner consistency, referential constraints,
      containment rules).
- [x] Add container-level binding/import validation, including entity-set keys,
      path/target kinds and multiplicity, target type compatibility, overload
      selection, and function-import service-document rules.
- [ ] Add annotation semantic validation (term applicability and target checks).

## 3. Primitive and Type Coverage

- [ ] Expand primitive support beyond the currently implemented subset where
      CSDL inputs require it.
- [ ] Track unsupported primitive names as explicit expected failures with clear
      diagnostics.
- [ ] Introduce typed facet/value unions in parse+serialize+resolver model:
      `MaxLength` (`int | max`), `Scale` (`int | variable`), `SRID` (`int | variable`), and typed `AppliesTo` target set.

## 4. Tests and Compatibility

- [x] Add fixture-backed end-to-end tests for each sample in `data/inputs`:
      parse -> resolve -> validate.
- [x] Add expected-failure tests for unsupported semantics (inheritance,
      operations, nav constraints, imports/bindings).
- [x] Add snapshot tests for XML<->JSON round-trip deltas to make serializer
      changes explicit.
- [ ] Add parity tests for `examples/` entrypoints so API-level behavior is
      stable.

## 5. Packaging Backlog

- [ ] Add crate feature flags so parse/serialize-only users can opt out of
      resolver + validator.
- [ ] Start with in-crate feature gating before considering a hard crate split
      (`csdl` vs `edm`).
- [ ] Define default feature set and compatibility policy for examples and
      public APIs.

## 6. Documentation Hygiene

- [ ] Authoritative spec model lives in `docs/csdl-attribute-catalog.toml`
      (top-level repo) and the rendered `docs/edm-semantic-graph.md`. Keep
      this TODO file focused on delivery tracking; the catalog is the source
      of truth for what CSDL 4.01 expresses.
