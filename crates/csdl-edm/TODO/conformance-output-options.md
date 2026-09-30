# Lossless configurable CSDL output

Audit follow-up 5; depends on source/type provenance and expression checking.

- Add explicit IEEE754Compatible and metadata-level options with arbitrary
  precision Decimal handling; no f64 normalization of arbitrary JSON decimals.
- Normalize nested typed literals, enum values and aliases for the target format.
- Rebase XML Base/reference URIs with caller source URI before JSON conversion.
- Preserve collection-nullability semantics instead of inventing omitted values.
- Validate complete resolved-graph output with caller-provided referenced documents.
- Keep legacy APIs compatible while making the checked contract precise and tested.

Key paragraphs: JSON 2.1.1-1, 2.1.2.2-1, 4.2-2, 5.1-2; XML/JSON literal and version clauses.
