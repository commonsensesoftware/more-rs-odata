# Crate features

The default features preserve the complete public API: parsing, serialization,
the semantic EDM graph, resolution, validation, graph export, and path expansion.
Existing dependency declarations need no changes.

| Features | Available modules |
| --- | --- |
| No features | `csdl`, XML/JSON readers, `parser`, `expr`, `serialization`, `error` |
| `edm` | Adds `edm`, `graph`, and `path_expansion` |
| `resolver` | Adds `resolver`; enables `edm` |
| `validator` | Adds `validator`; enables `edm` |
| Default | `resolver` and `validator`, including `edm` |

For parsing and serialization alone:

```toml
csdl-edm = { path = "../csdl-edm", default-features = false }
```

To resolve without compiling the validator, add `features = ["resolver"]`.
The validator can also be enabled independently for callers constructing EDM
models directly. Feature selection does not change XML/JSON representation,
syntactic model fields, or serialization defaults.

The format-comparison examples work without default features.
`parse_resolve_validate`, `edm_graph_demo`, and `url_expansion` require both
`resolver` and `validator`; Cargo skips them when those features are disabled.
All examples have tests enabled. Semantic integration tests are gated by the
features they use; parse/serialize tests still run without default features.

Check the supported configurations from the workspace root:

```text
cargo test -p csdl-edm --all-targets
cargo test -p csdl-edm --no-default-features --all-targets
cargo check -p csdl-edm --no-default-features --features edm --all-targets
cargo check -p csdl-edm --no-default-features --features resolver --all-targets
cargo check -p csdl-edm --no-default-features --features validator --all-targets
```

Gating remains within this crate. No public modules have moved to a new crate,
and default-enabled APIs remain the compatibility baseline.
