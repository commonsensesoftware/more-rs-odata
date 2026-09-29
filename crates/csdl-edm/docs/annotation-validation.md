# Annotation resolution and authoring diagnostics

The resolver preserves inline, sibling-target, and external annotation groups
in each semantic model's `annotations` field. Targets are typed paths over the
same EDM nodes used by routing and other validation. Operation targets can
select all overloads or a qualified parameter-type signature. Expression paths
are resolved against their annotated context; validators do not parse paths.

Use `validator::validate_annotations(&document)` to inspect annotation authoring
diagnostics, or `validate_model_annotations(&model)` for a single model.
Diagnostics include unavailable terms, unresolved targets, duplicate
term/qualifier applications, invalid qualifiers, intended applicability,
missing base-term applications, incompatible value shapes, and invalid paths.
Record and PropertyValue expression annotations also retain semantic targets.

These diagnostics are separate from `validate_document`. CSDL 4.01 section 14
requires consumers to tolerate unknown or invalid annotations. References do
not trigger network requests: annotations from an unavailable external
vocabulary retain their qualified term name and report `UnknownTerm` to the
explicit annotation diagnostic API.

The authored expression remains available alongside resolved paths. This API
does not execute dynamic expressions or evaluate service data. Value-shape
checks do not replace a runtime expression evaluator.

An omitted XML expression uses the term's default (or an empty collection or
default-valued record). Explicit Boolean true remains explicit. JSON requires
an explicit annotation value, so serialization expands locally available term
defaults. If the term is unavailable, the unknown value is represented as null.
