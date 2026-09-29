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
- [ ] Key properties are non-nullable.
- [ ] Key identity is consistent across inheritance.

### B. Entity type inheritance semantics

- [x] Derived types cannot redeclare an inherited key (a keyless base permits a key).
- [x] Derived types do not add properties to an inherited key.
- [x] In 4.01, inherited properties and navigation properties may only be
      redefined with a derived structured type, preserving collection cardinality.
- [x] Same-schema inheritance hierarchy contains no cycles.
- [x] Entity base types are entity types.
- [x] Derived open types remain open; derived media entities preserve HasStream.
- [x] Abstract entity types cannot derive from concrete entity types.
- [x] OData 4.0 member-redefinition restrictions.
- [ ] Cross-schema base resolution.

### C. Structural property semantics

- [ ] No conflicts with inherited property/navigation names.
- [ ] Default values are type-compatible.
- [ ] Facets (`Precision`, `Scale`, `MaxLength`) are type-valid.

### D. Complex type semantics

- [x] Same-schema complex type inheritance contains no cycles.
- [x] Derived complex types allow only valid CSDL 4.01 subtype narrowing.
- [x] Derived open complex types remain open.

### E. Navigation property semantics

- [ ] Multiplicity is semantically valid for target entity.
- [ ] If a Partner navigation property exists it points back correctly.
- [ ] Containment navigation properties are used in valid container contexts.

### F. Referential constraint semantics

- [ ] Principal key properties are non-nullable.
- [ ] Dependent properties exist and match principal key types.
- [ ] Constraint cardinalities match.
- [ ] Referential constraints do not form cycles.

### G. Entity container semantics

- [ ] NavigationPropertyBinding.Path resolves from the enclosing entity set/singleton declared entity type.
- [ ] NavigationPropertyBinding.Path segments are restricted to: type casts, complex properties, and containment navigation properties, followed by a final non-containment navigation property.
- [ ] NavigationPropertyBinding.Path MUST NOT contain non-containment navigation properties before the final segment.
- [ ] NavigationPropertyBinding.Path final segment MUST identify a navigation property (or a 4.01 terminal type-cast variant, when supported).
- [ ] NavigationPropertyBinding.Path type-cast segments are valid and in-scope, and cast position constraints are enforced.
- [ ] NavigationPropertyBinding.Path complex-property traversal is valid for the current structured type at each segment.
- [ ] NavigationPropertyBinding.Path containment traversal is valid, including collection-valued containment semantics (binding applies to all items).
- [ ] NavigationPropertyBinding.Path recursive sub-paths are accepted and interpreted recursively (positive-cycle semantics).
- [ ] No duplicate NavigationPropertyBinding.Path values within the same binding source, with "most specific path wins" behavior for type-cast-specialized paths.
- [ ] NavigationPropertyBinding.Target simple identifier resolves to an entity set/singleton in the same entity container.
- [ ] NavigationPropertyBinding.Target target path resolves to an in-scope entity set, singleton, or direct/indirect containment navigation property of a singleton.
- [ ] NavigationPropertyBinding.Target target path traversal before the final containment segment is restricted to single-valued complex properties and single-valued containment navigation properties.
- [ ] NavigationPropertyBinding.Target target path MUST NOT contain non-containment navigation properties before the final segment.
- [ ] NavigationPropertyBinding.Target terminal segment kind is validated against allowed target kinds (entity set, singleton, containment nav property).

### H. Function semantics

- [ ] Overloads differ by parameter signature.
- [ ] Bound functions declare a valid binding parameter.
- [ ] Parameter names are unique.

### I. Action semantics

- [ ] Overloads differ by parameter signature.
- [ ] Bound actions declare a valid binding parameter.
- [ ] Parameter names are unique.

================================================
