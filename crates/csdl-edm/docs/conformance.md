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

Service identity is retained in `csdl::Edmx::entity_container` and resolves to
`edm::DocumentModel::service_container`. JSON preserves an absent selector;
XML infers identity only when exactly one container is declared. JSON selectors
must use a namespace-qualified name, not an alias. Serialization does not choose
a container based on declaration order.

Use `validator::validate_document_as` with `DocumentKind::ServiceMetadata` to
require service identity and exactly one declared container. `DocumentKind::Model`
permits vocabulary and reusable model documents without a service container.
XML has no explicit selector, so selecting among multiple containers cannot be
represented as conforming XML service metadata.

Referenced documents can be resolved with `Resolver::resolve_with_loader`.
Pass the root document's absolute URI and a `DocumentLoader` implementation
(or a closure). `DocumentRegistry` supplies an in-memory implementation. The
library performs no network requests. References are cached by resolved URI;
relative references honor document/reference XML Base. Cyclic references reuse
loaded documents, while semantic inheritance cycles remain invalid.

Each document's aliases and included namespaces form a separate scope.
Selected schemas and their dependencies share the same resolved EDM node
identity. `DocumentModel::schemas` contains the root's declarations;
`referenced_schemas` retains imported nodes and `all_schemas()` visits both.
Annotation inclusion filters apply term namespace, qualifier, and target
namespace before attachments are resolved. Unincluded namespaces cannot be
used as structural type references or bind annotation terms merely because a
different document caused their definitions to be loaded.

Annotation expressions retain dynamic operators, nested annotations, and
Cast/IsOf facets. The resolver binds their types, function candidates, enum
members, labels, and paths. Recursive validation checks records, operands,
literal facets, conditional branches, and expression result types. Invalid or
unavailable expression bindings produce annotation diagnostics; they do not
prevent tolerant consumption. Client-side function evaluation remains outside
the library's scope.

For guarded serialization use `write_as_with_profile(..., ValidationProfile::Strict)`
or `try_to_json_value()`. These check grammar and declared-version restrictions
before writing any bytes. They reject XML Base loss in JSON and an XML conversion
that cannot preserve an explicit service-container selection. Resolve XML Base
into reference URIs before converting such a document. Semantic validation is
separate: resolve the complete graph with your loader and validate that graph
before publishing it. Legacy `write_as` and `to_json_value` retain their existing
unchecked behavior for compatibility.

Strict XML authoring requires explicit collection-property nullability in 4.01,
rejects deprecated `MaxLength="max"` in 4.01, and checks 4.0 restrictions on
vocabulary types, expression operators/paths, floating scale, singleton
nullability, and Unicode facet placement. Tolerant parsing still consumes these
constructs. Imported schemas retain their own source version; changing the root
document version affects local schemas only.

These profiles do not constitute a full conformance claim. The
[normative audit and matrix](conformance-audit.md) account for 485 requirement
paragraphs and identify remaining implementation and evidence gaps. The
[delivery record](../TODO/csdl-coverage-gaps.md#7-full-conformance-follow-up--dependency-order)
and [ordered follow-ups](../TODO/README.md) distinguish completed work from
requirements still needed for full validation.
