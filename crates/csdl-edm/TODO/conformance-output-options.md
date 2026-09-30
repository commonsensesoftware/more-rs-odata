# Lossless configurable CSDL output

Audit follow-up 5; depends on source/type provenance and expression checking.

Implemented by `SerializationOptions`, `write_as_with_options`, and
`write_as_checked_with_loader`; regression evidence is in `tests/output_options.rs`.

- Add explicit IEEE754Compatible and metadata-level options with arbitrary
  precision Decimal handling; no f64 normalization of arbitrary JSON decimals.
- Normalize nested typed literals, enum values and aliases for the target format.
- Rebase XML Base/reference URIs with caller source URI before JSON conversion.
- Preserve collection-nullability semantics instead of inventing omitted values.
- Validate complete resolved-graph output with caller-provided referenced documents.
- Keep legacy APIs compatible while making the checked contract precise and tested.

Key paragraphs: JSON 2.1.1-1, 2.1.2.2-1, 4.2-2, 5.1-2; XML/JSON literal and version clauses.

## API contract

The options writer buffers grammar/version validation before writing. The loader
variant additionally resolves and validates the original and emitted documents,
including annotation types with caller-supplied function/URL-result signatures.
Referenced documents are cached across the two checks. Neither API executes
expressions or proves arbitrary runtime expression equivalence.

`ieee754_compatible` selects strings for typed Int64/Decimal constants and defaults;
ordinary numeric output retains arbitrary decimal precision. Metadata `Full` adds
known type controls to records and annotation/property values; `Minimal` emits
authored record types. `None` retains explicitly authored record types when removing
them could change interpretation. This is the deliberate policy for the SHOULD
omit-control recommendation. Missing referenced metadata requires the loader API.

`source_uri` resolves XML Base and relative reference identities before output.
Unknown XML 4.0 collection-property nullability is rejected when targeting JSON.
JSON numeric enum values convert to named XML members when representable; checked
output rejects unnamed values that cannot be expressed losslessly in XML.

Legacy signatures remain available. The checked graph contract requires resolver
and validator features; representation-only output works without those features.
Exhaustive clause evidence remains in follow-up 6.
