# Reference metadata and scope completeness

Audit follow-up 2; depends on source provenance.

- Preserve/serialize/resolve annotations on references and included schemas.
- Expose Core.SchemaVersion to caller loader policy without automatic network use.
- Check namespace visibility on all structural and expression paths, cast types,
  enum values, function/label references, and external annotation targets.
- Test that loading a hidden dependency never makes it visible to the root.
- Test IncludeAnnotations with nested expressions and annotations-on-annotations.

Key paragraphs: XML/JSON 4.1-2, 14.2.2-1, 14.4.1.1-3 through -5.
