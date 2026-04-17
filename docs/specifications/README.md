# noahgift.com — WOS-First Provable Runtime

**Specification version:** 0.1.0
**Status:** Draft — awaiting approval
**Updated:** 2026-04-17
**Owner:** Noah Gift
**Cap:** every file in this directory is ≤ 500 lines. Split when a file approaches the cap.

---

## Abstract

noahgift.com today is a static HTML page that lists 10 Coursera specializations, 79 courses,
and a bio. The terminal aesthetic (tmux-style panes, tokyo-night theme) is cosmetic: the
panes are inert HTML, the WOS WASM shell in pane 3 is decorative, and the 15 machine-verifiable
"contracts" live in a JSON file that a visitor cannot easily verify without leaving the page.

This specification proposes a **WASM/Rust-first redesign** where the shell is the primary
interface and every factual claim on the site is *executable and falsifiable in-browser*.
The site stops being a document and becomes a runtime: a visitor can type `prove SPEC_COUNT`
and watch the site re-derive that number from the upstream source of truth (`fixtures.lua`)
using code that runs locally via WebAssembly.

The redesign is organized as ten commands, each specified in its own sub-spec. Every command
exposes new contracts (falsifiable claims) and leans on existing tooling from the
`course-studio`, `rmedia`, and `aprender` repositories. No new ML training is required for
the P0 slice.

---

## Design principles

These principles are load-bearing — every sub-spec must satisfy all of them or explicitly
justify a deviation.

### P1. The shell is primary, not decorative
WOS (the WASM terminal) is the top-level interface. Static panes exist only to bootstrap
discoverability for visitors who don't know what commands to type. Every static claim has
a `$ <command>` affordance that reproduces it.

### P2. Every number is executable
No hard-coded facts on the site. Every integer, percentage, or enumeration is produced by
a command whose source is auditable and whose output can be re-derived by a visitor.

### P3. Every claim is falsifiable
Every contract ships a `falsified_if` expression — a shell snippet that returns 0 when the
claim still holds and non-zero when it has broken. Visitors can run the falsifier
in-browser and see the outcome.

### P4. Source of truth is `course-studio/config/fixtures.lua`
The site mirrors the catalog in `fixtures.lua`. Drift between the site and the upstream
fixture file is a bug — detected by cryptographic hash comparison, not by human review.

### P5. Offline-first, WASM-first
No server round-trips for claim verification. Every falsifier, scorer, search index, and
inference path runs in the visitor's browser after a single cacheable download. rmedia
and aprender ship as `.wasm` modules.

### P6. Ship without ML where possible
The P0 slice uses deterministic algorithms (SHA-256, jq, tf-idf, shortest-path). ML
(embedding search, whisper, generative Q&A) is P1+. Each ML feature must carry a
deterministic fallback for users on slow networks.

### P7. Dogfood the marketing-asset pipeline
The site uses `rmedia coursera-score` to audit itself. The site's own grade is published
as a contract. If the grade drops below A, a banner appears. The tool that grades course
assets grades the storefront.

### P8. No new abstractions without a failing test
Every new abstraction (pipe plumbing, WASM command loader, RAG index format) begins with a
test that fails. Contracts precede implementation.

---

## Specification table of contents

All sub-specs live beside this README. Each follows the same template (see
`000-premise-and-principles.md` §6).

| ID  | Command                              | P0 | Sub-spec                                      |
| --- | ------------------------------------ | -- | --------------------------------------------- |
| 000 | *Premise + principles + template*    | —  | [000-premise-and-principles.md](./000-premise-and-principles.md) |
| 001 | `prove <claim-id>`                   | ✅ | [001-prove-claim.md](./001-prove-claim.md)    |
| 002 | `diff fixtures`                      | ✅ | [002-diff-fixtures.md](./002-diff-fixtures.md) |
| 003 | `grep course "<query>"`              | 🟡 | [003-semantic-grep.md](./003-semantic-grep.md) |
| 004 | `score <paste>`                      | ✅ | [004-score-paste.md](./004-score-paste.md)    |
| 005 | `transcribe <clip>`                  | 🔴 | [005-transcribe-clip.md](./005-transcribe-clip.md) |
| 006 | `recommend --from "<bg>" --to "<g>"` | 🟡 | [006-recommend-path.md](./006-recommend-path.md) |
| 007 | `replay <lesson> --verify`           | ✅ | [007-replay-verify.md](./007-replay-verify.md) |
| 008 | `audit site`                         | ✅ | [008-audit-site.md](./008-audit-site.md)      |
| 009 | `ask "<question>"`                   | 🔴 | [009-ask-transcripts.md](./009-ask-transcripts.md) |
| 010 | `pipe` — command composition         | ✅ | [010-pipe-composition.md](./010-pipe-composition.md) |

**Priority legend:** ✅ P0 (ship first, deterministic) · 🟡 P1 (ML fallback acceptable) · 🔴 P2 (heavy WASM, gated on perf).

---

## Dependency graph

Sub-specs have strict build ordering. Higher numbers depend on lower.

```
         000-premise
             │
     ┌───────┴───────┐
     │               │
  002-diff       001-prove
     │               │
     │               ├── 007-replay (hashes)
     │               ├── 008-audit  (rolls up all claims)
     │               └── 010-pipe   (composes all commands)
     │
     └── 004-score   (reuses jq primitive)
         │
         └── 003-grep (tf-idf fallback shares text pipeline)
             │
             └── 006-recommend (shares embeddings)
                 │
                 └── 009-ask    (RAG over same embeddings)
                     │
                     └── 005-transcribe (requires whisper.apr WASM, longest tail)
```

**Critical path:** 000 → 001 → 002 → 010 → 008 gives a fully working provable runtime
without any ML. Ship that first.

---

## Rollout phases

### Phase 0 — Foundation (week 1)
- Spec approval (this document + sub-specs 000-010)
- Bootstrap `rmedia-wos` crate (new crate in `../rmedia` for WASM command shims)
- `fixtures.json` emission added to CI in `course-studio`
- SHA-256 hash contract bound to `fixtures.json`

### Phase 1 — Deterministic core (weeks 2-3)
- Sub-specs **001 prove**, **002 diff**, **010 pipe**, **008 audit** land
- Every claim in current `contracts.json` has a working in-browser falsifier
- `audit site` reports composite grade on every page load

### Phase 2 — Scored content (week 4)
- Sub-spec **004 score** lands
- Sub-spec **007 replay** lands (needs CI sample-render job in `course-studio`)
- Visitors can paste outlines and get A-F grades

### Phase 3 — Search and recommend (weeks 5-6)
- Sub-spec **003 grep** (tf-idf first, embeddings behind flag)
- Sub-spec **006 recommend** (graph-only first, embeddings optional)
- aprender WASM build matures

### Phase 4 — Heavy ML (weeks 7+)
- Sub-spec **009 ask** (extractive RAG v1, generative v2)
- Sub-spec **005 transcribe** (whisper tiny, 39 MB download gate)
- Perf budget enforced via Playwright contracts

---

## Success metrics

Tracked as contracts in `contracts.json` and checked by `audit site` on every load.

| Metric                                        | Target         | Current   |
| --------------------------------------------- | -------------- | --------- |
| Falsifier pass rate on page load              | 100 %          | 0 %       |
| Composite coursera-score grade of the site    | A+             | unscored  |
| P50 time-to-first-command-echo                | < 50 ms        | —         |
| P95 WASM payload (gzipped)                    | < 30 MB        | 2.3 MB    |
| Claims re-derivable from public sources       | 100 %          | 0 %       |
| Site-fixtures hash drift                      | 0 (always)     | untracked |
| Command failure mode                          | WOS stderr line + red banner | — |

---

## Constraints

### C1. Payload budget
Total first-load WASM cannot exceed 30 MB gzipped. Heavy models (whisper, embeddings) load
on-demand via `loadModule <name>`.

### C2. Zero server dependencies
All provable infrastructure runs in the browser. CloudFront + S3 is the only backend.

### C3. No broken claims at any time
The CI build fails if any falsifier in `contracts.json` returns a failing outcome when run
against the current `fixtures.json`. The site cannot publish a false statement.

### C4. Graceful degradation
Every ML-backed command has a deterministic fallback. If aprender fails to load,
`grep course` falls back to tf-idf; `recommend` falls back to prerequisite-graph-only;
`ask` falls back to extractive return of top-k SRT chunks.

### C5. Browser compatibility
The site must work in the latest two versions of Chrome, Safari, Firefox, and mobile
Safari. SIMD is required; WebGPU is optional (only #005 and #009 at v2 need it).

---

## Open architectural questions

These must be resolved during approval review.

1. **Where does `rmedia-wos` live?** New crate in `../rmedia/crates/`? Sibling repo? Vendored?
   Impacts CI time and the dependency chain between course-studio and rmedia.
2. **How does `fixtures.json` get to the browser?**
   (a) Built as part of course-studio CI, pushed to noahgift.com S3; or
   (b) Fetched from course-studio raw GitHub with CORS.
   Option (a) gives latency + reliability; (b) gives freshness.
3. **Is aprender WASM-ready today?** Need to verify aprender + trueno compile to
   `wasm32-unknown-unknown` with SIMD. If not, that's a prerequisite epic.
4. **Contracts.json schema versioning.** Current schema is ad-hoc. Pre-P0, lock a versioned
   schema (JSON Schema file committed alongside).
5. **WOS extension API.** How do we register new commands? Does WOS expose a plugin loader,
   or do we fork WOS for this use case?

---

## Approval checklist

Before implementation begins:

- [ ] Specs 000-010 reviewed and marked `Status: Approved`
- [ ] All open architectural questions (this document §above) resolved
- [ ] `fixtures.json` emission agreed with course-studio maintainer (also Noah)
- [ ] `rmedia-wos` crate scaffold approved in rmedia repo
- [ ] Perf budget (C1) confirmed realistic against current aprender WASM size
- [ ] CI gates defined: contract falsifier smoke test, payload size regression

When all items are checked, change this document's `Status:` to `Approved` and cut a git
tag `spec-v0.1.0`.

---

## References

- `../../../course-studio/config/fixtures.lua` — current source of truth
- `../../../course-studio/CLAUDE.md` — course-studio conventions
- `../../../rmedia/` — Rust media toolchain and scorer
- `../../../aprender/` — Rust ML framework (tensor ops via trueno)
- `../../../noahgift-website/rmedia-site/` — current deployed terminal site
- Sibling repo `../../../rmedia/site/noahgift/` — canonical WOS terminal layout

---

## Glossary

**WOS** — *WASM Operating System*. The terminal-in-a-tab shell that ships in `wos.js` /
`wos_bg.wasm`. Runs commands, manages history, keeps a virtual filesystem.

**Contract** — a machine-verifiable claim. Shape: `{ id, claim, value, source, falsified_if }`.
Every contract is a row in `contracts.json`.

**Falsifier** — the shell expression in a contract's `falsified_if` field. Exits 0 when
the contract still holds; non-zero when broken.

**fixtures.lua** — the canonical catalog of specializations, guided projects, standalone
courses, instructors, and partners. Lives in `course-studio/config/`.

**fixtures.json** — a CI-emitted JSON snapshot of `fixtures.lua`, checked into the build
output at `/fixtures.json`. Consumable from WASM.

**Source of truth** — `fixtures.lua`. Everything else is a derivation.

**P0/P1/P2** — priority tiers. P0 = must ship in phase 1. P2 = stretch, may not ship.

---

*End of master spec. See sub-specs for per-command design.*
