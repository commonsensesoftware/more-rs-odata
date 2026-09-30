# Complete expression assignability

Audit follow-up 4; depends on typed expression-path result information.

- Complete canonical function signatures/overloads, numeric promotion and all
  temporal/spatial operand combinations; add caller-supplied client-function types.
- Check base-term record inheritance per member rather than exempting a whole record.
- Validate numeric JSON enum values and flag combinations, and retain literal
  source kinds where XML and JSON permit different spellings.
- Complete typed open-record values, label scope/uniqueness/cycles, cast facets,
  and all vocabulary path subtype assignments.
- Represent URL-reference results as unknown until caller-provided type/evaluation
  information exists; do not infer the resource body type from the URL string.
- Add paired positive/negative cases for every remaining expression clause.

Key paragraphs: XML/JSON 14.3, 14.4.4, 14.4.5, 14.4.9, 14.4.12, 14.4.13.
