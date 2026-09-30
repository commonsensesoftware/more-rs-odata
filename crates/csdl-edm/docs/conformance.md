# Static CSDL conformance

Use `parser::from_xml_reader_with_profile` or
`parser::from_json_reader_with_profile` with `conformance::ValidationProfile::Strict`
for authoring checks before resolution. These checks retain source information
that disappears during normalization: required XML Type attributes, document
wrappers, JSON member duplication, and XML namespace identity.

The profile-aware XML reader resolves namespaces before interpreting local
names. Foreign and future extension subtrees are ignored, so they cannot inject
known declarations. Strict mode additionally checks root/version, declaration
grammar, cardinalities, names, reference/include uniqueness, and aliases.
Unicode identifiers follow the categories and code-point limits in CSDL 15.
Known attributes used on the wrong declaration are rejected; unknown attributes
remain tolerated for extensibility.

After resolution, call `validator::validate_document_with_profile`. Both
profiles return structural errors and annotation diagnostics. The report's
`is_valid()` includes annotation diagnostics in strict mode and excludes them
in tolerant mode. An unavailable vocabulary is therefore visible to authors
without preventing tolerant model consumption.

Existing parser and validator entrypoints remain compatible. For complete
authoring diagnostics, use both strict source parsing and semantic validation;
validating only the normalized EDM cannot recover omitted or discarded source
constructs. The profile APIs are additive, and parsing remains available without
the optional resolver/validator features.

These profiles do not yet constitute a full conformance claim. Service identity,
external document resolution, complete expressions, recursive expression type
checking, and versioned serialization are tracked in
[the ordered conformance tasks](../TODO/csdl-coverage-gaps.md#7-full-conformance-follow-up--dependency-order).
