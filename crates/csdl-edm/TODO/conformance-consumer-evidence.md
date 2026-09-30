# Tolerant recovery and clause-level evidence

Audit follow-up 6; closes evidence after the preceding implementation items.

Status: **in progress**. Parser recovery and the grammar/advisory evidence
infrastructure below are delivered; exhaustive clause coverage is still open.

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

## Delivered checkpoint

- `from_xml_reader_with_report` and `from_json_reader_with_report` preserve recovery
  diagnostics while tolerant profile parsing omits unavailable/invalid expression
  annotations. Recovery drops the annotation, never just an operand; syntax that
  is not well-formed XML/JSON still fails. Strict and legacy contracts remain explicit.
- The five original OASIS XSD/JSON-schema/ABNF files are imported with source URLs
  and SHA-256 hashes. `scripts/inventory-grammar.ps1` classifies 192 families and
  all 813 ABNF cases. Sixty-six primitive/identifier cases execute; primitive cases
  use independently constructed XML and JSON source. Corpus integrity is tested.
- Imported cases found and fixed omitted seconds and leap-second validation.
- Every SHOULD/SHOULD NOT/RECOMMENDED paragraph has an explicit implementation or
  caller policy in `docs/advisory-policies.json`, checked against the inventory.
- Reviewed mixed paragraphs are decomposed in `docs/conformance-obligations.json`;
  focused paired-format cases cover reference URI uniqueness and enum member names.
- Stale matrix notes about completed follow-ups are reconciled without promoting
  broad suite links to complete clause evidence.

## Remaining work, in order

1. Complete the reviewed clause decomposition for the remaining mixed paragraphs.
2. Add independent positive/negative XML and JSON fixtures per static obligation,
   including XSD/JSON-schema branches and context-sensitive ABNF path adaptations.
3. Fix any behavior gaps those fixtures reveal, then promote only the obligations
   directly proven by their focused evidence. The partial matrix is the remaining
   work inventory; this checkpoint does not establish full conformance.
