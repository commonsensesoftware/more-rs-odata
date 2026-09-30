# Tolerant recovery and clause-level evidence

Audit follow-up 6; closes evidence after the preceding implementation items.

- Recover from unavailable/future annotation expression constructs in tolerant
  parsing without rejecting otherwise well-formed documents; preserve diagnostics.
- Add independent positive/negative XML and JSON cases per applicable obligation,
  including source-shape, cross-document, semantic and round-trip cases.
- Import and classify applicable XSD/JSON-schema and ABNF test families.
- Split mixed paragraphs into individual obligations where needed; do not turn a
  broad suite link into a claim of complete clause coverage.
- Account for every SHOULD with implemented warnings or explicit caller policy.
- Move partial/unsupported matrix rows to covered only after direct evidence exists.

Key paragraphs: XML/JSON 14-1 and 17 client conformance; all partial matrix rows.
