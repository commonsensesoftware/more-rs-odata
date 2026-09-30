# Strict source parity and provenance

Audit follow-up 1. See [conformance audit](../docs/conformance-audit.md).

- Enforce JSON alias-only qualified references, except `$EntityContainer`.
- Normalize/reject same-container qualified binding targets in JSON.
- Cover every known JSON member owner, type, default, and cardinality.
- Retain authored versus defaulted nullability for collection parameters/returns,
  including the prohibition on Nullable for entity-collection returns.
- Preserve semantically significant source order and explicitly document any
  remaining serialization-only interleaving normalization.
- Add positive/negative paired-format tests and update matrix dispositions.

Key paragraphs: JSON 4.2-2, 5.1-2, 12.8-2, 13.4.2-4; XML 2.4-1, 12.8-2.
