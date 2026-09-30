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

This is implementation tracking, not the normative CSDL element catalog.
The spec model is maintained in
[csdl-attribute-catalog.toml](../../../docs/csdl-attribute-catalog.toml)
and rendered in [edm-semantic-graph.md](../../../docs/edm-semantic-graph.md).
"Done" below refers to the delivery scope in the numbered checklist.

| Element group | Parse + serialize | Resolver | Validation | Implemented behavior |
| --- | --- | --- | --- | --- |
| Edmx / Schema | done | done | partial | Shared document namespace registry, aliases, declaration flags, schema-child uniqueness including containers |
| Reference / Include / IncludeAnnotations | done | done | partial | Reference metadata and aliases retained; referenced documents are not fetched |
| EntityType / ComplexType | done | done | done | Shared cross-schema inheritance, effective members/keys, cycles, subtype narrowing, versioned 4.0 rules |
| EnumType / TypeDefinition / Term | done | done | done | Concrete/built-in types, enum numeric values, facets, defaults, collection terms, base-term checks |
| Property / NavigationProperty / Key | done | done | done | Typed key, partner, and constraint paths; aliases, nullability, cardinality, compatible endpoint types |
| OnDelete / Partner / Containment | done | done | done | Typed delete actions, reciprocal partners, recursive/nonrecursive containment, versioned multiplicity rules |
| Function / Action / Parameter / ReturnType | done | done | done | Overloads, binding signatures, returns, facets/defaults, typed EntitySetPath |
| EntityContainer / EntitySet / Singleton | done | done | done | All containers, inherited resources, cycles, compatible targets and entity-set keys |
| Imports / NavigationPropertyBinding | done | done | done | Resolved operation overloads, cross-container targets, typed casts/traversal, duplicate paths, service-document rules |
| Annotation attachment forms | done | done | done | Typed targets/expression paths and explicit nonfatal authoring diagnostics |

## Delivery and limits

The original implementation tasks in sections 1–6 are complete. Regression suites cover
XML/JSON parsing, resolution, validation, round trips, expected failures, and
the five example entrypoints. Feature configurations preserve the full default
API and permit parsing/serialization without the EDM, resolver, or validator.

This does not claim exhaustive conformance for every possible CSDL document:

- Schema/reference validation remains partial; namespace/alias lexical rules
  and reference-document consistency are not exhaustively checked.
- References do not load external schemas or vocabularies. Required external
  type definitions must be supplied in the document's schemas.
- Annotation diagnostics are explicit and nonfatal. Value-shape checks and
  resolved expression paths do not execute dynamic expressions or service data.
  See [annotation validation](../docs/annotation-validation.md).
- Existing fixture pairs with different authored semantics remain expected
  mismatches; serialization does not erase those differences.

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
      abstract targets and their containment/key restrictions are also covered.
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
- [x] Add inheritance-related validation: cycles, effective keys, compatible
      member redefinitions, and open/abstract/media flags, including 4.0 differences.
- [x] Add navigation validation (partner consistency, referential constraints,
      containment rules).
- [x] Add container-level binding/import validation, including entity-set keys,
      path/target kinds and multiplicity, target type compatibility, overload
      selection, and function-import service-document rules.
- [x] Add annotation semantic validation (term applicability and target checks):
      inline, sibling, and external groups resolve to typed targets; overload
      signatures and expression paths resolve in the resolver. Explicit
      annotation diagnostics cover unknown terms/targets, duplicate applications,
      qualifiers, applicability, base-term applications, and value shapes.
      Annotation diagnostics are non-fatal to document consumption, as CSDL requires.

## 3. Primitive and Type Coverage

- [x] Scalar defaults: JSON numbers/booleans, type-definition representation,
      integer/enum ranges, string facets, decimal facets, and temporal literals.
- [x] Spatial default representations: WKT/GeoJSON conversion, shape checks,
      coordinate structure, ring closure, and effective SRID validation.
- [x] Omitted XML annotation expressions use the declared term default;
      JSON serialization materializes explicit values without fabricating true.

- [x] Enumeration underlying integer types, flags, explicit/implicit values,
      numeric range checks, and XML implicit-to-JSON numeric serialization.

- [x] All concrete primitives, including Stream and spatial families, plus
      built-in abstract and vocabulary path types with usage restrictions.
- [x] Track unsupported primitive names as explicit expected failures with clear
      diagnostics.
- [x] Introduce typed facet/value unions in parse+serialize+resolver model:
      `MaxLength` (`int | max`), `Scale` (`int | variable | floating`), `SRID` (`int | variable`), and typed `AppliesTo` target set (unknown future targets are preserved).
      Facet applicability, ranges, type-definition overrides, and versioned floating scale are validated.

## 4. Tests and Compatibility

- [x] Add fixture-backed end-to-end tests for each sample in `data/inputs`:
      parse -> resolve -> validate.
- [x] Add expected-failure tests for unsupported semantics (inheritance,
      operations, nav constraints, imports/bindings).
- [x] Add snapshot tests for XML<->JSON round-trip deltas to make serializer
      changes explicit.
- [x] Add parity tests for all five `examples/` entrypoints: format comparisons,
      pipeline loading, graph export, and XML/JSON URL expansion. Examples run
      as test targets. The pipeline default now uses the valid example89 fixture;
      URL expansion declares non-nullable keys.

## 5. Packaging Backlog

- [x] Add crate feature flags so parse/serialize-only users can opt out of
      resolver + validator.
- [x] Start with in-crate feature gating before considering a hard crate split
      (`csdl` vs `edm`).
- [x] Define default feature set and compatibility policy for examples and
      public APIs: see [features](../docs/features.md). Default features preserve
      the complete API; resolver and validator independently enable the EDM graph.

## 6. Documentation Hygiene

- [x] Authoritative spec model lives in `docs/csdl-attribute-catalog.toml`
      (top-level repo) and the rendered `docs/edm-semantic-graph.md`. Keep
      this TODO file focused on delivery tracking; the catalog is the source
      of truth for what CSDL 4.01 expresses.

## 7. Full Conformance Follow-up — dependency order

Checkpoint: completion of sections 1–6 is not a claim of full CSDL conformance.
Complete the following tasks in order, moving a dependency earlier only when
required. Commit each completed task separately, including its tests and status.

- [x] **1. Document and declaration validation.** Validate XML namespaces,
      document structure, required attributes, element cardinalities, supported
      versions, identifier/namespace Unicode syntax and lengths, reserved aliases,
      and document-wide alias collisions. Distinguish malformed known constructs
      from unknown extensions that tolerant consumers must ignore. Introduce
      explicit tolerant-consumption and strict-authoring validation profiles;
      retain annotation diagnostics without making tolerant consumption fail.
      Implemented additive profile-aware parsers and semantic reports; see
      [conformance profiles](../docs/conformance.md). Legacy entrypoints remain
      compatible. Strict checks happen before normalization loses source details.
- [x] **2. Service-document identity.** Preserve, resolve, validate, and serialize
      JSON `$EntityContainer`. Distinguish service metadata from vocabulary/model
      documents, applying service-specific container requirements only where
      appropriate. Do not choose a service container merely by declaration order.
- [x] **3. Referenced-document resolution.** Add a caller-supplied document loader
      or registry, relative-URI resolution, document-local alias scopes, schema
      inclusion and `IncludeAnnotations` filtering. Resolve external types and
      terms without requiring manual schema merging. Handle repeated/cyclic
      references deterministically; automatic network access is not required.
- [x] **4. Complete annotation expression representation.** Implement
      `ModelElementPath`, `Has`, `In`, `Add`, `Sub`, `Neg`, `Mul`, `Div`,
      `DivBy`, and `Mod` throughout model, XML/JSON parsing, serialization, and
      resolution. Preserve annotations on expressions and cast/type-test facets
      wherever permitted. Reject malformed known expressions instead of
      fabricating values or silently discarding extra operands.
- [ ] **5. Recursive annotation type checking.** Validate record types/members,
      property values, enum members, literal lexical values/facets, operator
      operands, conditional branches, function applications, casts, and label
      references. Check expression-path result types against expected term
      types. Keep strict authoring diagnostics separate from tolerant consumption.
- [ ] **6. Version and serialization conformance.** Audit every applicable
      4.0/4.01 difference, including expressions, paths, vocabulary types,
      constraints, facet placement, and collection nullability. Validate the
      declared output version and prevent silent emission of incompatible
      constructs. Cover XML/JSON representation defaults and semantic round trips.
- [ ] **7. Conformance evidence.** Build a normative requirement-to-code-to-test
      matrix with positive/negative XML/JSON cases, referenced-document fixtures,
      and semantic round-trip checks. Audit every applicable MUST/MUST NOT and
      account explicitly for SHOULD rules, profiles, and unsupported cases.
      Passing existing examples alone is not evidence of exhaustive conformance.

Static CSDL validation is the scope. Runtime expression evaluation, service-data
integrity, and execution of delete actions are separate concerns. The normative
sources are the OASIS CSDL XML and JSON 4.01 specifications and their referenced
ABNF/schema artifacts; the local catalog supports traceability but does not
replace a clause-by-clause audit.
