# Complete expression path grammar

Audit follow-up 3; depends on complete document scope isolation.

- Add resolver-owned typed nodes for key predicates, ordered indexes and `$count`.
- Handle empty paths, implicit media annotation terms, and model-property/term
  cast syntax without reparsing authored paths in the validator.
- Enforce instance-path collection traversal limits and correct result multiplicity.
- Complete starting contexts for external property targets, operations/imports,
  parameters/returns, annotations and nested expressions.
- Test all 4.0/4.01 path syntax differences and both-format semantic round trips.
- Replace the expected-gap `$count` regression in `conformance_audit.rs`.

Key paragraphs: XML/JSON 14.4.1.1 and 14.4.1.2, plus non-MUST path permissions.
