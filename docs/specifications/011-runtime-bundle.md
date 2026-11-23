# Spec 011 — Runtime Bundle (`rmedia-wos-kit` + `rmedia wos`)

**Status:** Draft
**Priority:** P0
**Depends on:** 000, 001, 002, 003, 004, 005, 006, 007, 008, 009, 010
**Owner:** Noah Gift

**Canonical bundle spec.** This is the master spec for the shippable runtime that
composes every sub-command defined in specs 000–010 into a single Rust library
(`rmedia-wos-kit`) and a single CLI entrypoint (`rmedia wos`). Component detail
lives in `sub/*.md` and in the upstream command specs 000–010.

---

## Abstract

Specs 000–010 define ten in-browser WASM commands. Each command ships as its own
Cargo crate with its own trait implementation, fixtures, and falsifiers. A visitor
to noahgift.com sees one unified terminal; an implementer maintaining the Rust
side needs a symmetric "one surface" — a bundle that wires every command together
without circular dependencies, and a host-side CLI that exercises the exact same
registry that WASM serves.

This spec defines two artifacts:

1. **`rmedia-wos-kit`** — a top-level Cargo crate that re-exports the eight WOS
   crates (`rmedia-wos`, `-prove`, `-score`, `-ask`, `-audit`, `-grep`,
   `-recommend`, `-transcribe`) and provides a `RegistryBuilder` for composing
   them into a single `Registry` without dependency cycles.
2. **`rmedia wos <subcmd>`** — a new subcommand on the existing `rmedia` CLI that
   builds a `Registry` via the kit, reads stdin, and dispatches to a single
   command, a `--pipe` pipeline (spec 010), or `--list`.

The bundle is the glue between the ten command specs and the terminal users type
into. It introduces no new semantic behavior — every exit code, contract, and
refusal is defined upstream. Its job is deterministic composition and exit-code
propagation so host-side smoke tests can falsify the full matrix from a shell.

---

## Table of Contents

| #  | Section                                         | Sub-spec                                             |
|----|-------------------------------------------------|------------------------------------------------------|
| 1  | [Problem](#1-problem)                           | [000-premise-and-principles.md](./000-premise-and-principles.md) |
| 2  | [Non-goals](#2-non-goals)                       | inline                                               |
| 3  | [User story](#3-user-story)                     | inline                                               |
| 4  | [Bundle crate layout](#4-bundle-crate-layout)   | [sub/bundle-crate.md](./sub/bundle-crate.md)         |
| 5  | [`RegistryBuilder` API](#5-registrybuilder-api) | [sub/registry-builder.md](./sub/registry-builder.md) |
| 6  | [Audit snapshot pattern](#6-audit-snapshot-pattern) | [sub/audit-snapshot.md](./sub/audit-snapshot.md) |
| 7  | [`rmedia wos` CLI surface](#7-rmedia-wos-cli-surface) | [sub/cli-wos-subcommand.md](./sub/cli-wos-subcommand.md) |
| 8  | [Command components](#8-command-components)     | [001](./001-prove-claim.md) … [009](./009-ask-transcripts.md) |
| 9  | [Pipeline composition](#9-pipeline-composition) | [010-pipe-composition.md](./010-pipe-composition.md) |
| 10 | [Contracts produced](#10-contracts-produced)    | inline                                               |
| 11 | [Falsification recipe](#11-falsification-recipe) | inline                                              |
| 12 | [Performance budget](#12-performance-budget)    | inline                                               |
| 13 | [Failure modes](#13-failure-modes)              | inline                                               |
| 14 | [Open questions](#14-open-questions)            | inline                                               |
| 15 | [References](#15-references)                    | inline                                               |

---

## 1. Problem

The ten WOS specs describe commands in isolation. Before this spec, composing
them required downstream code (the noahgift.com site, `rmedia` CLI, or an ad hoc
host tool) to depend on every sub-crate individually, construct indexes by hand,
and hope registration order and audit recursion did the right thing.

Three concrete consequences:

- **Circular-dep risk.** `rmedia-wos` (registry + dispatch) is a transitive
  dependency of `-ask`, `-grep`, `-recommend`, `-audit`, `-transcribe`. Putting a
  `with_defaults()` factory inside `rmedia-wos` would force it to depend on its
  own dependents.
- **Audit recursion.** `AuditCommand` (spec 008) holds an `Arc<Registry>` and
  dispatches back into it. If registered into the same mutable `Registry` it
  dispatches against, a naive implementation recurses.
- **Host/browser asymmetry.** The browser runs WASM through the WOS kernel. The
  host has no terminal — you cannot smoke-test `prove | jq` from a shell without
  building your own glue. CI ends up re-implementing what the site already does.

This spec eliminates all three by adding one bundle crate and one CLI subcommand.

## 2. Non-goals

- **New command semantics.** Every contract, exit code, and refusal is defined in
  specs 001–010. The bundle only composes and propagates.
- **WASM build.** The kit compiles for `wasm32` but this spec does not define the
  browser loader. See spec 000 §"Rollout phases" for the WASM artifact pipeline.
- **Non-WOS commands.** `jq`, shell built-ins, and anything outside the
  `WosCommand` trait are out of scope. Spec 010 defines `jq` as a registered
  first-class citizen.
- **Global mutable registries.** The kit builds one `Registry` per `build()` call.
  No `static mut`, no `OnceCell`, no singleton.

## 3. User story

*As a contributor maintaining the noahgift.com repo*, I add one dependency —
`rmedia-wos-kit` — and call `RegistryBuilder::new().stateless().with_prove(...).build()`
to get a registry identical to what the WASM site serves.

*As a CI pipeline*, I invoke `rmedia wos --contracts contracts.json --fixtures
fixtures.json prove --all --json | jq '.[] | select(.passed == false)'` in a
bash script and assert the exit code. The same command string would work in the
in-browser WOS terminal.

*As a release engineer*, I smoke the full command matrix via `rmedia wos --audit
audit --json --quick` and gate the release on composite ≥ A.

## 4. Bundle crate layout

**Sub-spec**: [sub/bundle-crate.md](./sub/bundle-crate.md)

`rmedia-wos-kit` is a leaf crate one level below the application layer. It
depends on all eight WOS crates and re-exports their public API so downstream
consumers import from `rmedia_wos_kit::` alone.

```
rmedia-cli
     │
     └── rmedia-wos-kit ──┬── rmedia-wos            (trait + registry + exec)
                          ├── rmedia-wos-prove      (spec 001)
                          ├── rmedia-wos-score      (spec 004)
                          ├── rmedia-wos-ask        (spec 009)
                          ├── rmedia-wos-grep       (spec 003)
                          ├── rmedia-wos-recommend  (spec 006)
                          ├── rmedia-wos-audit      (spec 008)
                          └── rmedia-wos-transcribe (spec 005)
```

The kit itself contains no command logic. Re-exports, a builder, and seven unit
tests that verify composition. Browser code can depend on the kit directly; the
host `rmedia-cli` depends on it once via `src/wos_cmd.rs`.

## 5. `RegistryBuilder` API

**Sub-spec**: [sub/registry-builder.md](./sub/registry-builder.md)

The kit exposes a chained-method builder because each command takes a different
construction payload (indexes, graphs, Arcs). A single `with_defaults()` factory
cannot type-check every combination. The builder is additive, cheap to clone,
and its `build()` method is infallible.

```rust
let reg = RegistryBuilder::new()
    .stateless()                                  // cat, jq, score
    .with_prove(contracts, fixtures_json, bytes)  // spec 001
    .with_ask(ask_index)                          // spec 009
    .with_grep(grep_index)                        // spec 003
    .with_recommend(graph, roles)                 // spec 006
    .with_transcribe(Arc::new(backend))           // spec 005
    .with_audit()                                 // spec 008
    .build();
```

Omitted `with_*` calls produce a registry that simply does not register that
command — there is no null or stub.

## 6. Audit snapshot pattern

**Sub-spec**: [sub/audit-snapshot.md](./sub/audit-snapshot.md)

`AuditCommand` (spec 008) rolls up every other command. To avoid recursion, the
builder snapshots the pre-audit registry into an `Arc<Registry>` *before*
registering audit itself:

```rust
if self.audit {
    let snapshot = Arc::new(reg.clone());
    reg.register(AuditCommand::new(snapshot));
}
```

Audit dispatches against the snapshot, which does not contain audit. The outer
registry does contain audit, so `rmedia wos audit` works from the shell, but a
hypothetical pipeline `audit | audit` dispatches the inner audit against a
registry that is one audit shallower — still non-recursive.

## 7. `rmedia wos` CLI surface

**Sub-spec**: [sub/cli-wos-subcommand.md](./sub/cli-wos-subcommand.md)

New subcommand on the `rmedia` binary. Reads stdin once and dispatches one of
three paths:

```
rmedia wos --list                     # list registered commands
rmedia wos --pipe "cmd1 | cmd2"       # pipeline (spec 010)
rmedia wos <name> [args...]           # single command
```

Opt-in data loading via flag or environment variable pair:

| Flag                    | Env var                      | Enables           |
|-------------------------|------------------------------|-------------------|
| `--contracts + --fixtures` | `RMEDIA_WOS_CONTRACTS + _FIXTURES` | `prove`    |
| `--ask-chunks`          | `RMEDIA_WOS_ASK_CHUNKS`      | `ask`             |
| `--grep-docs`           | `RMEDIA_WOS_GREP_DOCS`       | `grep`            |
| `--graph + --roles`     | `RMEDIA_WOS_GRAPH + _ROLES`  | `recommend`       |
| `--replay-manifest + --replay-assets-dir` | `RMEDIA_WOS_REPLAY_*` | `replay` |
| `--fake-transcribe`     | —                            | `transcribe` (deterministic) |
| `--audit`               | —                            | `audit` (requires at least one data-bearing cmd) |

Paired flags are validated eagerly — supplying one without its partner exits 2
with a clear message. Unknown commands return the name and exit 1. Exit codes
from underlying commands propagate to the process.

## 8. Command components

Each component below is governed by its upstream spec. The bundle's job is
registration; the component's job is semantics.

| Spec | Command      | Kit wiring                                         |
|------|--------------|-----------------------------------------------------|
| 001  | `prove`      | `.with_prove(contracts, fixtures_json, bytes)`      |
| 002  | `diff`       | stateless, registered by `.stateless()`             |
| 003  | `grep`       | `.with_grep(grep_index)`                            |
| 004  | `score`      | stateless, registered by `.stateless()`             |
| 005  | `transcribe` | `.with_transcribe(Arc<dyn WhisperBackend>)`         |
| 006  | `recommend`  | `.with_recommend(graph, roles)`                     |
| 007  | `replay`     | `.with_replay(ReplayCommand)`                       |
| 008  | `audit`      | `.with_audit()` — MUST be called last (snapshot)    |
| 009  | `ask`        | `.with_ask(ask_index)`                              |
| 010  | `pipe`       | `exec_pipeline_with_stdin()` (always available)     |

Additionally registered by `.stateless()`: `cat`, `jq`. These are infra, not
spec'd individually, and are pre-conditions for the canonical cookbook pipelines
in spec 010 §4.5.

## 9. Pipeline composition

**Sub-spec**: [010-pipe-composition.md](./010-pipe-composition.md)

`exec_pipeline_with_stdin(line, stdin, &registry)` from `rmedia-wos` powers
`--pipe`. The 4 MB inter-stage buffer cap, shlex parsing, and exit-bail
semantics are defined in spec 010 and re-exported from the kit.

The CLI wires `--pipe` directly to the same function used in-browser. If the
last stage writes to stderr, the CLI streams that to the process's stderr; on
non-zero exit the process exits with the pipeline's composite exit code.

## 10. Contracts produced

```json
{
  "id": "RUNTIME_BUNDLE_CRATES",
  "claim": "rmedia-wos-kit re-exports at least 8 WOS crates",
  "value": 8,
  "source": "rmedia/crates/rmedia-wos-kit/Cargo.toml",
  "falsified_if": { "kind": "jq_count", "target": ".dependencies | map(select(startswith(\"rmedia-wos\"))) | length", "op": ">=", "value": 8 }
}
```

```json
{
  "id": "CLI_WOS_LIST_CONTAINS_TRIO",
  "claim": "`rmedia wos --list` prints cat, jq, and score unconditionally",
  "value": ["cat", "jq", "score"],
  "source": "rmedia/crates/rmedia-cli/tests/wos_cli_t.rs::wos_list_shows_stateless_trio",
  "falsified_if": { "kind": "shell", "cmd": "rmedia wos --list | grep -Ec '^(cat|jq|score)$'", "op": "==", "value": 3 }
}
```

```json
{
  "id": "CLI_WOS_EXIT_PROPAGATES",
  "claim": "A non-zero exit from an underlying WOS command propagates to the CLI process exit code",
  "value": true,
  "source": "rmedia/crates/rmedia-cli/src/wos_cmd.rs::run",
  "falsified_if": { "kind": "shell", "cmd": "rmedia wos galaxy-brain; echo $?", "op": "!=", "value": 0 }
}
```

```json
{
  "id": "CLI_WOS_PAIRED_FLAGS_VALIDATED",
  "claim": "Supplying --contracts without --fixtures (or vice versa) exits with code 2",
  "value": 2,
  "source": "rmedia/crates/rmedia-cli/src/wos_cmd.rs::build_registry",
  "falsified_if": { "kind": "shell", "cmd": "rmedia wos --contracts /tmp/c.json --list; echo $?", "op": "==", "value": 2 }
}
```

```json
{
  "id": "AUDIT_SNAPSHOT_NON_RECURSIVE",
  "claim": "AuditCommand dispatches against a pre-audit snapshot (no self-recursion)",
  "value": true,
  "source": "rmedia/crates/rmedia-wos-kit/src/lib.rs::RegistryBuilder::build",
  "falsified_if": { "kind": "sha256", "path": "rmedia-wos-kit/src/lib.rs", "hash": "<bound>" }
}
```

## 11. Falsification recipe

```bash
# Exit-code propagation
rmedia wos galaxy-brain; test "$?" -ne 0 || { echo "✗ unknown command did not fail"; exit 1; }

# Paired-flag validation
rmedia wos --contracts /tmp/nope.json --list 2>&1 | grep -q "must be supplied together" \
  || { echo "✗ paired-flag validation missing"; exit 1; }

# Stateless trio always registered
count=$(rmedia wos --list | grep -Ec '^(cat|jq|score)$')
test "$count" -eq 3 || { echo "✗ stateless trio missing"; exit 1; }

# Prove pipeline end-to-end (host parity with browser)
out=$(rmedia wos \
  --contracts crates/rmedia-wos-prove/tests/fixtures/contracts.json \
  --fixtures  crates/rmedia-wos-prove/tests/fixtures/fixtures.json \
  --pipe "prove --all --json | jq '[.[].passed] | all'")
test "$out" = "true" || { echo "✗ prove pipeline regressed"; exit 1; }

# Audit snapshot round-trip
rmedia wos \
  --contracts crates/rmedia-wos-prove/tests/fixtures/contracts.json \
  --fixtures  crates/rmedia-wos-prove/tests/fixtures/fixtures.json \
  --audit audit --json --quick | jq -e '.grade' >/dev/null \
  || { echo "✗ audit rollup regressed"; exit 1; }
```

All five commands must exit 0.

## 12. Performance budget

| Metric                                       | Target       |
|----------------------------------------------|--------------|
| `RegistryBuilder::build()` — empty registry  | ≤ 1 ms       |
| `RegistryBuilder::build()` — full registry   | ≤ 10 ms      |
| `rmedia wos --list` cold (binary startup)    | ≤ 50 ms      |
| Single-command dispatch overhead (vs direct) | ≤ 5 ms       |
| `--pipe` 3-stage overhead (vs direct)        | ≤ 15 ms      |
| Bundle crate size (release, host)            | ≤ 400 KB     |
| Unit + integration tests for kit + CLI       | ≤ 5 s        |

Budgets enforced by `rmedia-wos-kit/tests/cookbook.rs` and
`rmedia-cli/tests/wos_cli_t.rs`.

## 13. Failure modes

| Failure                                   | Behavior                                       |
|-------------------------------------------|------------------------------------------------|
| Unknown subcommand name                   | Exit 1; stderr names the command + suggests `--list` |
| Paired flag mismatch                      | Exit 2; stderr names both flags                |
| Data file unreadable or invalid JSON      | Exit 1; stderr includes path + parse error     |
| `--audit` without any data-bearing cmd    | `--list` prints only audit; audit itself exits with "no sub-contracts" |
| Pipe buffer overflow (spec 010 §4.3)      | Exit 3; propagated from `exec_pipeline_with_stdin` |
| Empty stdin + command that requires stdin | Command's own exit code (usually 1) propagates |
| Whisper backend panics                    | Caught by `catch_unwind` in `-transcribe`; exit 1 |

Nothing in the bundle layer produces a panic on its own. All errors route
through `rmedia_types::RmediaError::Other(String)` and exit cleanly.

## 14. Open questions

- **Q1.** Should the kit expose a `stateless_registry()` fn so the browser can
  call zero-config for the cookbook panel? Currently yes — it's a one-line
  convenience that saves the WASM loader a builder invocation. Lean keep.
- **Q2.** Should the CLI emit a machine-readable `--list --json` for CI
  pipelines? Current `--list` is human-readable one-per-line. Low cost; defer
  until a consumer asks.
- **Q3.** Where do fixtures live canonically? Today the CLI's integration tests
  use `../rmedia-wos-prove/tests/fixtures/` via `env!("CARGO_MANIFEST_DIR")`.
  For an end user running `rmedia wos prove`, they must supply their own. Should
  the CLI ship a default `--contracts=site` that downloads from noahgift.com?
  Defer to v0.2 — it conflates host and browser responsibilities.
- **Q4.** Does the kit need an async surface? Every WOS command is synchronous
  by trait definition. Host concurrency is the caller's problem. Hold sync.

## 15. References

- Bundle crate: `../../../rmedia/crates/rmedia-wos-kit/`
- CLI source: `../../../rmedia/crates/rmedia-cli/src/wos_cmd.rs`
- CLI tests: `../../../rmedia/crates/rmedia-cli/tests/wos_cli_t.rs`
- Kit tests: `../../../rmedia/crates/rmedia-wos-kit/tests/cookbook.rs`
- Upstream PR: `rmedia#80 — feat/wos-prove-evaluator`
- Component specs: [000](./000-premise-and-principles.md) through
  [010](./010-pipe-composition.md)
- Style reference: `../../../provable-contracts/docs/specifications/pv-spec.md`

---

*End of bundle spec. See component sub-specs for per-command design and
`sub/*.md` for bundle-specific details.*
