# Semantic Validation Checklist

This checklist is now split into:

- rules already implemented in `src/validator.rs`
- remaining semantic backlog

## Implemented now

- [x] No duplicate key properties.
- [x] Key properties must exist on the declaring entity.
- [x] Key properties cannot be complex-valued.
- [x] No duplicate schema child names.
- [x] No duplicate entity child names (property/navigation combined).
- [x] No duplicate complex child names (property/navigation combined).
- [x] No duplicate enum member names.
- [x] No duplicate entity container child names (entity set/singleton).

## Remaining backlog

### A. Entity type semantics

- [x] For OData 4.0, non-abstract entity types define or inherit a key. In 4.01,
      key requirements depend on usage (entity sets and collection containment).
- [x] Key paths are single-valued and non-nullable with permitted scalar types.
- [x] Key paths and aliases are inherited; aliases are required for nested paths, unique, and forbidden for direct properties.
- [x] CSDL 4.01 related-entity keys require the entire directly related key; 4.0 rejects navigation key paths.

### B. Entity type inheritance semantics

- [x] Derived types cannot redeclare an inherited key (a keyless base permits a key).
- [x] Derived types do not add properties to an inherited key.
- [x] In 4.01, inherited properties and navigation properties may only be
      redefined with a derived structured type, preserving collection cardinality.
- [x] Document-scope inheritance hierarchy contains no cycles.
- [x] Entity base types are entity types.
- [x] Derived open types remain open; derived media entities preserve HasStream.
- [x] Abstract entity types cannot derive from concrete entity types.
- [x] OData 4.0 member-redefinition restrictions.
- [x] Cross-schema base resolution, forward aliases, and qualified-name identity.

### C. Structural property semantics

- [ ] No conflicts with inherited property/navigation names.
- [ ] Default values are type-compatible.
- [ ] Facets (`Precision`, `Scale`, `MaxLength`) are type-valid.

### D. Complex type semantics

- [x] Document-scope complex type inheritance contains no cycles.
- [x] Derived complex types allow only valid CSDL 4.01 subtype narrowing.
- [x] Derived open complex types remain open.

### E. Navigation property semantics

- [ ] Multiplicity is semantically valid for target entity.
- [x] Partner paths resolve through complex properties/derived casts, target the
      declaring entity or a base, and reciprocate when both partners are declared.
- [x] Complex-type navigations cannot declare Partner; collection navigations
      cannot declare Nullable.
- [x] Collection containment requires a keyed target; recursive partners are
      nullable and single-valued; non-recursive partners are non-nullable.
- [x] An inheritance chain has at most one navigation with a containment partner.
- [x] OData 4.0 rejects collections of complex values containing navigation targets.

### F. Referential constraint semantics

- [x] Constraint paths resolve to structural or navigation properties, including
      nested complex properties, on entity and complex owners.
- [x] Dependent/principal types and collection cardinalities match (distinct
      complex types are allowed by CSDL 4.01).
- [x] Constraints are declared only on single-valued navigations and dependent
      paths are unique.
- [x] Dependent nullability equals navigation-or-principal nullability.
- [x] OnDelete requires one valid Action and cannot be repeated.

Principal properties need not be keys or non-nullable. Referential cycles are
not forbidden by the CSDL 4.01 constraint rules. Containment instance identity
and execution of delete actions require service data, not schema-only validation.

### G. Entity container semantics

- [x] NavigationPropertyBinding.Path resolves from the enclosing entity set/singleton declared entity type.
- [x] NavigationPropertyBinding.Path segments are restricted to: type casts, complex properties, and containment navigation properties, followed by a final non-containment navigation property.
- [x] NavigationPropertyBinding.Path MUST NOT contain non-containment navigation properties before the final segment.
- [x] NavigationPropertyBinding.Path final segment MUST identify a navigation property (or a 4.01 terminal type-cast variant, supported in 4.01).
- [x] NavigationPropertyBinding.Path type-cast segments are valid and in-scope, and cast position constraints are enforced.
- [x] NavigationPropertyBinding.Path complex-property traversal is valid for the current structured type at each segment.
- [x] NavigationPropertyBinding.Path containment traversal is valid, including collection-valued containment semantics (binding applies to all items).
- [x] NavigationPropertyBinding.Path recursive sub-paths are accepted and interpreted recursively (positive-cycle semantics).
- [x] No duplicate NavigationPropertyBinding.Path values within the same binding source, retaining distinct type-cast-specialized paths for consumers to select the most specific match.
- [x] NavigationPropertyBinding.Target simple identifier resolves to an entity set/singleton in the same entity container.
- [x] NavigationPropertyBinding.Target target path resolves to an in-scope entity set, singleton, or direct/indirect containment navigation property of a singleton.
- [x] NavigationPropertyBinding.Target target path traversal before the final containment segment is restricted to single-valued complex properties and single-valued containment navigation properties.
- [x] NavigationPropertyBinding.Target target path MUST NOT contain non-containment navigation properties before the final segment.
- [x] NavigationPropertyBinding.Target terminal segment kind is validated against allowed target kinds (entity set, singleton, containment nav property).

### H. Function semantics

- [x] Overloads have distinct unordered parameter-name sets and ordered type
      signatures, grouped by binding type; return types agree within each group.
- [x] Bound functions declare a binding parameter (any type is permitted).
- [x] Parameter names are non-empty and unique.
- [x] EntitySetPath starts at the binding parameter, traverses navigation/casts,
      and ends at an entity type compatible with the entity-valued return.
- [x] Entity collection returns cannot specify Nullable.

### I. Action semantics

- [x] Bound overloads differ by binding type; unbound actions cannot overload.
- [x] Bound actions declare a binding parameter (any type is permitted).
- [x] Parameter names are non-empty and unique.

================================================
