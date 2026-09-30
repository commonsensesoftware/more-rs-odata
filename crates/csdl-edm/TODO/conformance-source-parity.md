# Strict source parity and provenance

Audit follow-up 1. See [conformance audit](../docs/conformance-audit.md).

Implemented: strict source checks and collection nullability provenance, with
regression evidence in `tests/source_parity.rs`. Expression-specific grammar,
output normalization, and tolerant recovery belong to follow-ups 3–6.

- Enforce JSON alias-only qualified references, except `$EntityContainer`.
- Normalize/reject same-container qualified binding targets in JSON.
- Cover every known JSON member owner, type, default, and cardinality.
- Retain authored versus defaulted nullability for collection parameters/returns,
  including the prohibition on Nullable for entity-collection returns.
- Preserve semantically significant source order and explicitly document any
  remaining serialization-only interleaving normalization.
- Add positive/negative paired-format tests and update matrix dispositions.

Key paragraphs: JSON 4.2-2, 5.1-2, 12.8-2, 13.4.2-4; XML 2.4-1, 12.8-2.

Order policy: enum members, operation parameters, overloads, collection items,
and expression operands retain source order. Structural and navigation properties,
container children of different kinds, and declaration/annotation interleaving
are grouped by the established syntactic model during serialization. These
interleavings have no positional CSDL semantics; serialization is not a lexical
source-preserving editor. Raw syntactic equality includes nullability provenance;
effective EDM nullability is the appropriate cross-format semantic comparison.
