# Spec 012 — Site Audit Contract (`site-contract`)

**Status:** Implemented (partial) — SITE_001, SITE_002, SITE_003, SITE_004, SITE_005, SITE_009, SITE_010 wired into CodeBuild; SITE_006, SITE_007 wired into `make precommit`; SITE_008 deferred pending apr-cli on crates.io (tracked in PMAT-009)
**Priority:** P0
**Depends on:** 000, 001, 002, 008, 011
**Owner:** Noah Gift

---

## Abstract

Spec 008 defined an in-browser rollup (`audit site`). This spec defines the
**build-time / CI-time** counterpart: a set of falsifiable contracts that must
hold for the deployed `rmedia-site/` artifacts before a CodeBuild deploy is
allowed to sync to S3. The contracts prevent regressions of two real P0 bugs
found in the 2026-04-18 audit session:

1. **Bio Duke link pointed to `scholars.duke.edu/person/Noah.Gift`** — Duke
   retired the Scholars profile, returning 404.
2. **Social "Google Scholar" row** linked to `scholar.google.com/citations?user=...`
   — reachable from Noah's browser but CAPTCHA-blocked for most first-time
   visitors, producing a 403 via server verification and a silent trust-damaging
   failure for site users.

Both bugs shipped to production because (a) the URL verifier existed but was
not gated in CI, and (b) no contract forbade Google Scholar as a link target.
This spec makes both failures falsifiable in one shell command.

---

## Table of Contents

| #  | Section                                             | Sub-spec                                           |
|----|-----------------------------------------------------|----------------------------------------------------|
| 1  | [Problem](#1-problem)                               | inline                                             |
| 2  | [Non-goals](#2-non-goals)                           | inline                                             |
| 3  | [User story](#3-user-story)                         | inline                                             |
| 4  | [Contract surface](#4-contract-surface)             | [sub/site-contracts.md](./sub/site-contracts.md)   |
| 5  | [Generator as source of truth](#5-generator-as-source-of-truth) | inline                                 |
| 6  | [`apr probar comply` integration](#6-apr-probar-comply-integration) | inline                             |
| 7  | [Falsification recipe](#7-falsification-recipe)     | inline                                             |
| 8  | [Failure modes](#8-failure-modes)                   | inline                                             |
| 9  | [Open questions](#9-open-questions)                 | inline                                             |
| 10 | [References](#10-references)                        | inline                                             |

---

## 1. Problem

noahgift.com ships as static HTML assembled from three panes
(`rmedia-site/content/pane-0.html`, `pane-1.html`, `pane-2.html`). The
assembler is `rmedia site build`, which:

1. Reads the three pane HTML fragments
2. Substitutes them into `rmedia-site/templates/index.tmpl`
3. Runs a URL verifier (`verify_urls`) that HEAD-checks every link
4. Emits `rmedia-site/public/index.html` + static assets

`verify_urls` reports `N/N ok, 0 failed` on success. On the 2026-04-18 audit,
it reported `109/109 ok` — **yet the site still had two broken user
experiences** because:

- The Duke link (`scholars.duke.edu/person/Noah.Gift`) returned a 404 HTML
  page with status 200 OK (Duke's 404 handler doesn't return 404)
- The Google Scholar link returned 200 for server-to-server checks but a
  reCAPTCHA page for anonymous browsers

Status-code-only verification is insufficient. We need **content-shape
contracts**: per-link rules that forbid known-bad destinations
independent of HTTP status.

## 2. Non-goals

- Replacing the generic URL verifier (`verify_urls` remains step 1)
- Crawling off-site — contracts only inspect the built `public/` tree
- Anti-bot evasion — if a URL is CAPTCHA-blocked we refuse to link it, not
  route around it
- Generic accessibility audits (lighthouse, axe) — separate spec

## 3. User story

As the site maintainer, before I push a commit that changes pane HTML, I want
to run one command that:

1. Builds the site (`rmedia site build`)
2. Checks the 8 site-specific contracts below against `public/`
3. Runs `apr probar comply public/` as the browser-side compliance backstop
4. Returns exit 0 only if every contract and every applicable probar check
   passes

If a contract fails, the output tells me **which file** and **which line** to
fix at the generator source (`scripts/gen_panes.lua`), not at the built
`public/*.html` (which is a build artifact, not a source).

## 4. Contract surface

Ten contracts, each a falsifiable claim over `rmedia-site/public/`:

| ID            | Claim                                                                             | Falsifier                                                        |
|---------------|-----------------------------------------------------------------------------------|------------------------------------------------------------------|
| `SITE_001`    | Every `<a href>` in panes resolves via `verify_urls` with status 200              | `rmedia site verify-urls` exit 0                                 |
| `SITE_002`    | No `<a href>` in panes points to `scholar.google.com` (CAPTCHA-blocked)           | `grep -c 'scholar\.google\.com' public/index.html` equals 0      |
| `SITE_003`    | No `<a href>` in panes points to `scholars.duke.edu` (retired profile host)       | `grep -c 'scholars\.duke\.edu' public/index.html` equals 0       |
| `SITE_004`    | Duke link uses canonical Coursera partner URL `/partners/duke`                     | `grep -cE 'coursera\.org/partners/duke[">]' public/index.html` ≥ 1 (minify-safe)   |
| `SITE_005`    | Coursera instructor profile link exists                                            | `grep -c 'coursera\.org/instructor/noahgift' public/index.html` ≥ 1 |
| `SITE_006`    | Deterministic build: same inputs produce bit-identical `public/index.html`        | Two sequential `rmedia site build` produce identical sha256      |
| `SITE_007`    | Pane HTML is always regenerated from `scripts/gen_panes.lua`, never hand-edited   | `rmedia site build --regen-panes` leaves working tree clean      |
| `SITE_008`    | `apr probar comply` passes all checks except C001 + C006 (N/A on static HTML)     | `apr probar comply public/ --skip-checks C001,C006` exit 0       |
| `SITE_009`    | Bio pane distinguishes **current** from **former** faculty affiliations           | `grep -c 'Faculty at.*UC Berkeley' public/index.html` equals 0 (pre-fix shape) |
| `SITE_010`    | `<meta name="description">` comes from site.toml / scene.prs, not template literal | `grep -c '90+ Coursera' public/index.html` equals 0 (old hardcoded shape) |

Contract detail (inputs, outputs, edge cases) lives in
[sub/site-contracts.md](./sub/site-contracts.md).

### Contract classification

- **Hard gate (block deploy):** SITE_001, SITE_002, SITE_003, SITE_004, SITE_006, SITE_008, SITE_009, SITE_010
- **Soft warn (log but allow):** SITE_005, SITE_007

SITE_005 is soft because the instructor profile URL pattern might change;
SITE_007 is soft because small manual edits to `pane-*.html` may precede a
batched regeneration.

## 5. Generator as source of truth

The panes under `rmedia-site/content/pane-{0,1,2}.html` are **build artifacts**
derived from `rmedia-site/scripts/gen_panes.lua`. The audit session found that
a hand-edit to a pane file (e.g., fixing a URL) is reversible on the next
`gen_panes.lua` run and must be made at the generator first.

### Rule

All URL fixes **MUST** land in `gen_panes.lua` before the pane HTML is
regenerated. Contract SITE_007 enforces this by:

1. Running `gen_panes.lua` to regenerate `content/pane-{0,1,2}.html`
2. Running `git diff --exit-code content/` in the worktree
3. Exit 0 iff the working tree is clean post-regeneration

This prevents a future contributor from fixing a bug by editing the pane HTML
in `public/` or `content/` while leaving `gen_panes.lua` stale.

### Anti-pattern (what bit us 2026-04-18)

A previous session fixed the Duke URL by editing `content/pane-0.html`
directly. The next `rmedia site build` ran `gen_panes.lua`, which rewrote the
pane with the stale Scholars URL, re-introducing the bug. SITE_007 catches
this immediately in CI.

## 6. `apr probar comply` integration

The new unified `apr probar comply` binary (aprender GH-876) wraps probador's
C001–C010 WASM/browser compliance checks. For a static-HTML site without
WASM, two checks are structurally not applicable:

| Check | Status on static HTML                                            |
|-------|------------------------------------------------------------------|
| C001  | N/A — "code execution verified" requires a WASM module to run    |
| C006  | N/A — COOP/COEP headers only matter when serving WASM            |

The remaining eight checks (C002 console-errors, C003 custom-elements,
C004 threading, C005 low-memory, C007 replay-hash, C008 cache, C009 WASM-size,
C010 panic-paths) all pass against noahgift.com's `public/` tree because none
of them fire on static HTML inputs.

### Invocation

```bash
apr probar comply rmedia-site/public/ \
  --skip-checks C001,C006 \
  --fail-fast \
  --format text
```

Exit 0 iff all remaining eight checks pass. `--skip-checks` is a denylist
(aprender GH-876, 2026-04-18) — it runs all checks except the two N/A ones,
so when a new check (C011+) ships, SITE_008 picks it up automatically
(tightening-by-default). Compare to `--checks` which is an allowlist: it
requires this spec to be revised every time probador adds a new check, so
would silently loosen coverage over time.

### Future: WASM shell

Spec 011 (`rmedia wos`) bundles a real WASM shell into noahgift.com. Once
that ships, the N/A list shrinks to zero and this contract upgrades to
`--checks C001,C002,...,C010` (all ten, no filter). SITE_008 is written so
that adding more checks to the `--checks` list is a strict tightening, never a
loosening — any new check that fails will flip the gate red.

## 7. Falsification recipe

Five shell tests that prove each hard-gate contract. Run from the
`rmedia-site/` working directory after `rmedia site build`.

```bash
#!/bin/bash
set -euo pipefail

# --- Test 1: SITE_001 — generic URL verification ---
rmedia site verify-urls
echo "PASS SITE_001"

# --- Test 2: SITE_002 — no Google Scholar CAPTCHA-blocked links ---
if grep -q 'scholar\.google\.com' public/index.html; then
  echo "FAIL SITE_002: Google Scholar link present (CAPTCHA-blocked)" >&2
  exit 2
fi
echo "PASS SITE_002"

# --- Test 3: SITE_003 + SITE_004 — Duke link correctness ---
if grep -q 'scholars\.duke\.edu' public/index.html; then
  echo "FAIL SITE_003: retired scholars.duke.edu link present" >&2
  exit 3
fi
if ! grep -qE 'coursera\.org/partners/duke[">]' public/index.html; then
  echo "FAIL SITE_004: canonical Duke Coursera link missing" >&2
  exit 4
fi
echo "PASS SITE_003"
echo "PASS SITE_004"

# --- Test 4: SITE_006 — deterministic build ---
sha_a=$(sha256sum public/index.html | awk '{print $1}')
rmedia site build > /dev/null
sha_b=$(sha256sum public/index.html | awk '{print $1}')
if [ "$sha_a" != "$sha_b" ]; then
  echo "FAIL SITE_006: non-deterministic build (sha differs)" >&2
  exit 6
fi
echo "PASS SITE_006"

# --- Test 5: SITE_008 — probar comply backstop ---
apr probar comply public/ --skip-checks C001,C006 --fail-fast
echo "PASS SITE_008"

# --- Test 6: SITE_009 — no stale faculty claim ---
# The pre-fix bio said "Faculty at Duke, UC Berkeley, UC Davis, and Northwestern"
# but UC Berkeley is "Former Lecturer" since Summer 2019, and Northwestern SPS
# doesn't list Noah on the linked program page. Require the bio to split
# "Faculty at X and Y" from "Former lecturer at Z and W".
if grep -qE 'Faculty at [^.]*UC Berkeley' public/index.html; then
  echo "FAIL SITE_009: bio claims current UC Berkeley faculty (Noah is Former Lecturer)" >&2
  exit 9
fi
echo "PASS SITE_009"

# --- Test 7: SITE_010 — meta description not hardcoded in template ---
# Pre-fix, rmedia's terminal_template.html had a literal
# "Faculty at Duke, UC Berkeley, UC Davis, and Northwestern. 90+ Coursera courses."
# that contradicted site.toml's "79 courses across 10 specializations".
if grep -q '90+ Coursera' public/index.html; then
  echo "FAIL SITE_010: meta description is template literal (should come from site.toml)" >&2
  exit 10
fi
echo "PASS SITE_010"

echo "ALL HARD-GATE CONTRACTS PASS"
```

Exit codes map to contract IDs (2→SITE_002, 3→SITE_003, 4→SITE_004,
6→SITE_006) so CI can report `failed contract: SITE_00N` by exit code alone.

## 8. Failure modes

| Failure              | Symptom                                            | Recovery                                                     |
|----------------------|----------------------------------------------------|--------------------------------------------------------------|
| `apr` not installed  | `command not found: apr`                           | `cargo install apr-cli` (GH-876 published)                   |
| `verify_urls` timeout | HTTP check exceeds 5s                              | `rmedia site verify-urls --offline` skips HEAD probes        |
| Generator drift       | SITE_007 fails after `gen_panes.lua` regeneration | Re-run `gen_panes.lua`, commit regenerated panes             |
| Coursera partner URL changes | SITE_004 hard-fails on valid URL             | Update `gen_panes.lua` + spec + bump SITE_004 version        |
| False positive on new link | SITE_002/SITE_003 block a legitimate URL     | Add allow-list in `gen_panes.lua` with justification comment |

## 9. Open questions

- **Should SITE_005 (Coursera instructor link) be hard-gated?** Currently
  soft; if it's removed, the site loses a primary CTA. Lean toward hardening
  once SITE_001–SITE_004 have stabilized.
- **Scope of "CAPTCHA-blocked" block-list:** should we enumerate known
  CAPTCHA-protected hosts (scholar.google.com, linkedin.com/sales/...) or
  keep contracts URL-specific?

### Resolved

- **~~SITE_006 determinism guarantees~~** — Resolved 2026-04-18. The
  non-determinism source was `os.date("!%Y-%m-%dT%H:%M:%SZ")` in
  `rmedia-site/scripts/gen_panes.lua`, which wrote a wall-clock
  `generated_at` field into `fixtures.json` + `contracts.json`. Fix: dropped
  the `generated_at` field entirely and bumped schema to 0.2. Output is now
  fully content-addressed via `fixtures_hash`. Two sequential runs produce
  bit-identical output.
- **~~CodeBuild integration~~** — Resolved 2026-04-18. The recipe runs in
  `buildspec.yml`'s `build` phase, before the `s3 sync` in `post_build`.
  `rmedia-site/scripts/contracts.sh` + `verify_urls.sh` gate the deploy.
  SITE_006 + SITE_007 run locally via `make precommit` because they need
  Lua + the upstream `course-studio` tree and can't run in the CodeBuild
  environment, which only deploys from pre-built `rmedia-site/public/`.

## 10. References

- **Spec 008** — `audit site` (in-browser rollup; runtime counterpart)
- **Spec 011** — Runtime bundle (`rmedia-wos-kit` + `rmedia wos`)
- **aprender GH-876 / PR #877** — `apr probar comply` unified CLI
- **probador compliance checks** — `handlers/comply.rs` C001–C010
  (`crates/aprender-test-cli/src/handlers/comply.rs`)
- **noahgift-website** — `rmedia-site/scripts/gen_panes.lua` (source of
  truth for pane HTML)
- **2026-04-18 audit session** — found scholars.duke.edu 404 + Google Scholar
  CAPTCHA block; fixes landed in pane-0.html + pane-2.html + gen_panes.lua
