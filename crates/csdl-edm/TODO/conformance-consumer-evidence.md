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

The focused evidence suites now independently author XML and JSON for reference/
include shapes, schema declarations, structured inheritance, scalar/complex/related
keys, facet bounds, navigation nullability/partners, enums and type definitions.
These cases exposed and fixed reserved schema namespaces and symbolic-scale casing
at the strict source boundary, plus declaring-type member name collisions in semantic validation. The matrix now records 219 covered and 207 partial
paragraphs; advisory and outside-static-scope dispositions remain unchanged.

Structured member redeclaration, partner path/type clauses, containment partner
cardinality and versioned complex collection containment now have focused evidence.
The containment checks also now accept collection-valued non-recursive partners
consistently across XML and JSON; their omitted Nullable facet is not scalar nullability.
Containment binding endpoints and inheritance-chain parent uniqueness also have
independent positive/negative XML and JSON evidence.
Constraint paths and required navigation declarations now have focused source evidence. Operation evidence covers overload identity, parameter names, return-type scope and collection nullability, required operation declarations, and entity-set paths. Collection return consistency and required parameter source shapes now have independent XML/JSON evidence. Parameter type scope now has scalar and collection cases for primitive, entity, complex, enum and type-definition types. Operation multiplicity now has direct resolved-graph assertions and invalid JSON encoding cases. These exposed and fixed strict JSON acceptance of XML-style Collection(...) type strings. Container evidence now covers entity-set/singleton type scope and naming collisions between sets and singletons. Container name evidence also covers operation-import collisions. Inheritance evidence covers narrowing, kind changes and inherited import redeclarations. Container declaration shapes and unbound operation reference checks now have paired-format evidence. Function-import overload exposure now has direct graph assertions. Import entity-set target cases cover scalar returns, local/cross-container sets, missing targets and singleton rejection. Collection-return targets and required import declarations now have paired-format evidence. Import name uniqueness and versioned singleton nullability now have independent fixtures, including the JSON default and invalid value shapes. Binding source paths now have independent set/singleton cases for complex properties, derived casts and containment traversal. Binding local target scope, required declaration shapes and JSON same-container target spelling now have independent evidence. Containment target traversal now has scalar/collection intermediate-segment cases; duplicate binding paths and distinct cast paths have independent fixtures. Target-root scope now has independent evidence. Mixed binding-selection paragraphs are decomposed into covered static uniqueness and runtime consumer responsibilities; their paragraph-level status remains partial. Annotation constant evidence now covers Date, Guid, DateTimeOffset, TimeOfDay and Duration with explicit annotation diagnostics and temporal precision facets. Boolean, Int64 and floating constants now have independent cases, which fixed JSON numeric compatibility with floating terms and XML acceptance of noncanonical nonfinite spellings. Binary constant cases now enforce base64url terminal bits and matching padding; decimal cases cover large values and invalid forms under variable scale. Floating-scale decimal and string cases are now covered; string fixtures fixed XML entity-reference decoding in constant text and rejection of invalid XML control characters. Enum constants now have member/type resolution and flags-combination evidence. Logical operator arity/types and conditional arity, Boolean conditions and direct-collection exceptions now have independent fixtures. Equality and ordered comparisons now have arity, numeric promotion and operand compatibility cases. Has/In and arithmetic expressions are next, followed by remaining dynamic expressions and constant representation branches, followed by remaining grammar branches and unresolved mixed-clause reviews. Existing broad suites remain supporting evidence, not a substitute for
the independent clause fixtures required below.

1. Complete the reviewed clause decomposition for the remaining mixed paragraphs.
2. Add independent positive/negative XML and JSON fixtures per static obligation,
   including XSD/JSON-schema branches and context-sensitive ABNF path adaptations.
3. Fix any behavior gaps those fixtures reveal, then promote only the obligations
   directly proven by their focused evidence. The partial matrix is the remaining
   work inventory; this checkpoint does not establish full conformance.
