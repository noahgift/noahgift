# Spec 000 — Premise, principles, and sub-spec template

**Status:** Approved
**Priority:** P0 (foundational)
**Depends on:** none
**Owner:** Noah Gift

---

## 1. Problem

The current noahgift.com terminal site (deployed 2026-04-17) is a visually-faithful
imitation of a tmux session — but *only* the imitation. Nothing that a terminal promises
(composability, executability, inspection, provenance) is actually delivered. Concretely:

- Panes 0-2 are inert `<div>`s styled to look like tmux columns.
- The 15 "machine-verifiable contracts" live in a JSON file that visitors are told to
  `curl` manually. No execution happens in the page.
- WOS (the WASM shell in pane 3) has a canned command list and no access to the rest of
  the site's data.
- The rich tooling in `../rmedia` (renderer, scorer, outline extractor, falsifier) and
  `../aprender` (tensor ML with trueno SIMD) is advertised on the site but not used on
  the site.

A visitor has no better way to verify *"Noah teaches 79 courses on Coursera"* than a
senator has to verify a tax return. The number is ink.

## 2. Premise

**A website for a researcher who publishes provable contracts should be a runtime, not a
document.** Every factual claim must be executable; every execution must leave a public
audit trail; every tool mentioned in the bio must power some visible behavior on the page.

The terminal aesthetic is not a costume — it is the honest interface to a runtime that
really does run code. WOS + rmedia + aprender, compiled to WASM, make this buildable in
a browser tab with no server.

## 3. Non-goals

This spec and the ones that follow do **not**:

- Migrate the old Hugo Academic articles back online. That content is archived and
  out of scope.
- Build a CMS, blog, or RSS feed. The site is a catalog + verifier.
- Replace Coursera or host video content. Coursera remains the canonical delivery
  surface; noahgift.com is a discovery + verification layer over it.
- Provide a login or user-specific features. All interaction is stateless and anonymous.
- Add tracking, analytics, or any third-party JS. Everything is first-party WASM.

## 4. Design principles

The eight principles listed in the master spec (README §"Design principles") bind every
sub-spec. Restated here for convenience:

| # | Principle | Test |
|---|-----------|------|
| P1 | Shell is primary | Every static claim has a `$ <cmd>` affordance |
| P2 | Every number is executable | No hard-coded ints/percents in HTML |
| P3 | Every claim is falsifiable | `falsified_if` is present and runs in-browser |
| P4 | Source of truth is `fixtures.lua` | All derivations hash-chain back to it |
| P5 | Offline-first, WASM-first | Zero server calls for claim verification |
| P6 | Ship without ML where possible | Deterministic baseline before any model |
| P7 | Dogfood marketing-asset pipeline | Site self-scores with `coursera-score` |
| P8 | No new abstractions without a failing test | Contract precedes code |

## 5. Critique of the existing design

### 5.1. Contracts are JSON, not runtime
Current `contracts.json` lists claims but has no execution path. The falsifier is a
`jq` expression in a string field — not invoked, not checked on load, not checked in CI.
If `fixtures.lua` drops to 9 specializations tomorrow, the site will continue to say 10
until a human notices. **This violates P3.**

### 5.2. Panes 0-2 are static HTML
`content/pane-{0,1,2}.html` are generated once by `scripts/gen_panes.lua` at build time
and frozen into the published bundle. No command echoes their content, and the terminal
in pane 3 cannot read from them. **This violates P1 and P2.**

### 5.3. rmedia and aprender are bio decorations
The bio says "built rmedia" and "built aprender" — yet neither runs in the browser. The
tools that earn the bio placement should power the runtime. **This violates P7.**

### 5.4. WOS has no extension API used
`wos_bg.wasm` ships but the command set is canned. There is no registration path for
noahgift.com-specific commands like `prove`, `grep`, `score`. **This blocks every
sub-spec.**

## 6. Sub-spec template

Every file in `00N-*.md` follows this skeleton. Do not deviate without a note.

```markdown
# Spec NNN — <Command or concept>

**Status:** Draft | Review | Approved | Implemented | Superseded
**Priority:** P0 | P1 | P2
**Depends on:** <comma-separated spec IDs or "none">
**Owner:** <name>

## 1. Problem
## 2. Non-goals
## 3. User story
## 4. Design
### 4.1. Surface (command syntax, flags, output)
### 4.2. Data contract (inputs, outputs, schemas)
### 4.3. Implementation sketch (code hooks, crates, APIs)
## 5. Contracts produced
## 6. Falsification recipe
## 7. Performance budget
## 8. Failure modes and fallback
## 9. Open questions
## 10. References
```

### 6.1. Section rules

- **§1 Problem** states the gap in *one paragraph*. No background history.
- **§2 Non-goals** must list at least two explicit exclusions.
- **§3 User story** is one sentence in the canonical form:
  *"As a <role>, I type `<command>` so that <outcome>."*
- **§4 Design** is the load-bearing section. Code fences are welcome. Stay under 200 lines.
- **§5 Contracts produced** lists new rows added to `contracts.json` with full
  `{id, claim, value, source, falsified_if}` shapes.
- **§6 Falsification recipe** is a shell snippet a reader can paste into WOS (or any
  Unix shell) to check the contracts. Must exit 0 when the spec's contracts all hold.
- **§7 Performance budget** gives hard numbers (ms, MB, KB). No vague "fast".
- **§8 Failure modes** lists deterministic fallbacks per P6.
- **§9 Open questions** gets pruned as decisions are made. Empty list is valid at
  `Status: Approved`.
- **§10 References** cites files by absolute path or `repo/path/to/file.ext:line`.

### 6.2. Length cap

Each sub-spec is capped at **500 lines** including headers, code fences, and blank lines.
If a spec needs more, split it into `NNN-a-*.md` and `NNN-b-*.md` and link from the
original.

### 6.3. Approval gate

A sub-spec may not move from `Draft` to `Approved` until:

1. All open questions (§9) resolved or explicitly deferred
2. Contracts (§5) all have working `falsified_if` snippets
3. Performance budget (§7) has a method of measurement identified
4. At least one reviewer other than the owner has commented

## 7. Glossary extensions

Extending the master glossary for this spec:

**Provable runtime** — a web surface where every public claim can be re-derived by
running code the visitor can inspect, in the visitor's browser, with no server call.

**Dogfooding contract** — a contract whose target is the site itself (e.g., "this page
scores A+ on coursera-score"). The site competes against its own tool.

**Deterministic fallback** — a code path that produces a usable answer without invoking
any ML model. Required for every ML-enabled feature.

**WOS command** — a function with the signature `fn(stdin: &[u8], argv: Vec<String>) ->
(exit_code: u8, stdout: Vec<u8>, stderr: Vec<u8>)`. Exposed via the `rmedia-wos`
registration API (design TBD — see master spec §"Open architectural questions").

## 8. Versioning

This document is the **binding template** for specs 001-010. Changes to §6 template
structure require bumping the master spec version and re-approving sub-specs that
deviate from the new template.

Template version bumps:
- `0.1.0` — initial template (this document)

## 9. References

- Master spec: `./README.md`
- Existing site: `https://noahgift.com/` (as of 2026-04-17)
- Existing contracts: `https://noahgift.com/contracts.json`
- Source of truth: `../../../course-studio/config/fixtures.lua`
- Canonical WOS example: `../../../rmedia/site/noahgift/`
