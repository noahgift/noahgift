# Sub-spec — `rmedia-wos-kit` bundle crate

**Parent:** [../011-runtime-bundle.md](../011-runtime-bundle.md) §4
**Source:** `../../../../rmedia/crates/rmedia-wos-kit/`

---

## 1. Purpose

Collapse eight independent WOS crates into one import surface so downstream
consumers (the noahgift.com WASM site, `rmedia-cli`, third-party tools) depend
on a single crate and get the full command matrix. No command logic lives here.

## 2. Manifest

```toml
[package]
name = "rmedia-wos-kit"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Bundle crate that wires all WOS crates into a single Registry factory (specs 000-010)"

[dependencies]
rmedia-wos            = { path = "../rmedia-wos" }
rmedia-wos-prove      = { path = "../rmedia-wos-prove" }
rmedia-wos-score      = { path = "../rmedia-wos-score" }
rmedia-wos-ask        = { path = "../rmedia-wos-ask" }
rmedia-wos-audit      = { path = "../rmedia-wos-audit" }
rmedia-wos-grep       = { path = "../rmedia-wos-grep" }
rmedia-wos-recommend  = { path = "../rmedia-wos-recommend" }
rmedia-wos-transcribe = { path = "../rmedia-wos-transcribe" }
serde      = { workspace = true, features = ["derive"] }
serde_json = "1"

[dev-dependencies]
sha2 = "0.10"
hex  = "0.4"
```

Eight `rmedia-wos*` deps. No optional features. `serde` + `serde_json` for the
shared fixture types re-exported from `rmedia-wos-prove`. `sha2` + `hex` are
test-only for claim-hash verification in `tests/cookbook.rs`.

## 3. Re-exports

```rust
pub use rmedia_wos::{
    exec_pipeline_with_stdin, CommandOutput, PipelineResult, Registry, WosCommand,
};
pub use rmedia_wos_prove::{Contracts, prove_all, ProveError, ProveResult};
pub use rmedia_wos_score::Score;
pub use rmedia_wos_ask::{AskIndex, Chunk, ChunkParams};
pub use rmedia_wos_audit::{audit_site, AuditCommand, AuditReport};
pub use rmedia_wos_grep::{DocEntry, DocSource, GrepIndex};
pub use rmedia_wos_recommend::{Graph, Role, Roles};
pub use rmedia_wos_transcribe::{
    DeterministicFakeBackend, ReplayCommand, ReplayManifest, WhisperBackend,
};
```

Downstream consumers never import from individual `rmedia-wos-*` crates.

## 4. Dependency ordering

```
       rmedia-wos           (trait + registry + pipe exec)
            ▲
   ┌────────┼────────┬────────┬──────────┬──────────┬────────────┐
   │        │        │        │          │          │            │
prove    score     ask      grep     recommend    audit      transcribe
   ▲        ▲        ▲        ▲          ▲          ▲            ▲
   └────────┴────────┴────────┴──────────┴──────────┴────────────┘
                                   │
                            rmedia-wos-kit
                                   │
                              rmedia-cli
```

`rmedia-wos` is the kernel: trait, registry, pipe executor, shared types.
Everything else depends on it. The kit depends on all of them. The CLI depends
only on the kit.

## 5. What the kit does NOT do

- **No global state.** No `OnceCell`, no `lazy_static`, no `static mut`. Every
  caller builds its own `Registry`.
- **No I/O.** The kit does not read files, talk to networks, or spawn threads.
  Data loaders live in `rmedia-cli/src/wos_cmd.rs`.
- **No feature flags.** Every command is unconditionally compiled. Consumers
  opt out at registration time by not calling the corresponding `with_*`.
- **No panics.** Every public fn returns infallibly or via `Result`.

## 6. Tests

`crates/rmedia-wos-kit/tests/cookbook.rs` — 8 cross-crate integration tests.

| Test                              | Asserts                                         |
|-----------------------------------|-------------------------------------------------|
| `full_kit_registers_every_command` | All 10 commands present after full builder       |
| `stateless_trio_always_registered` | cat, jq, score present with `.stateless()`       |
| `ask_pipe_jq_roundtrip`            | `ask "q" --json \| jq .` end-to-end              |
| `grep_pipe_jq_roundtrip`           | `grep "q" \| jq .` end-to-end                    |
| `recommend_pipe_jq_roundtrip`      | `recommend --from --to \| jq .` end-to-end       |
| `transcribe_pipes_into_ask`        | deterministic fake backend → `ask` consumption   |
| `audit_quick_rollup`               | `--audit audit --quick` exits 0 with grade       |
| `audit_site_fn_matches_command`    | `audit_site()` fn output == `audit` command JSON |

`src/lib.rs` — 7 unit tests covering builder defaults, audit snapshot, empty
registry, transcribe Arc wiring, and pipeline composition.

## 7. Size

Release profile, host target: ~380 KB rlib. Budget ≤ 400 KB. WASM target not
measured at this layer — gzipped WASM budget (≤ 30 MB total) is tracked in
spec 000 §C1.
