# Sub-spec 012-a — Site Contracts (detail)

**Parent:** [012-site-audit-contract.md](../012-site-audit-contract.md)
**Status:** Draft
**Cap:** ≤ 500 lines

Detail for the eight contracts defined in spec 012 §4. Each contract has:

- **Claim** — what must be true
- **Input** — files/tools inspected
- **Falsifier** — exact shell command + expected exit code
- **Edge cases** — known false-positive / false-negative patterns
- **Remediation** — where the fix lands in source

---

## SITE_001 — All link targets resolve (hard gate)

**Claim:** Every `<a href="...">` in the panes returns HTTP 200 when HEAD-probed.

**Input:**
- `rmedia-site/public/index.html`
- `rmedia-site/content/pane-{0,1,2}.html`

**Falsifier:**
```bash
rmedia site verify-urls
# Expected: "verify_urls: N/N ok, 0 failed"
# Exit 0 on success, 1 on any failed URL
```

**Edge cases:**
- Some servers (Duke Scholars) return 200 + a 404 HTML body. SITE_001 catches
  the protocol-level bugs; SITE_003 catches the semantic bugs.
- Corporate SSO redirects return 302 → 200 on a login page. Treat as pass.
- Rate-limited hosts (GitHub API) may return 429. `--retry 3` mitigates.

**Remediation:** Fix the URL in `scripts/gen_panes.lua`, then regenerate
panes, then rebuild.

---

## SITE_002 — No Google Scholar links (hard gate)

**Claim:** `scholar.google.com` does not appear as a link target in any pane.

**Input:** `rmedia-site/public/index.html`

**Falsifier:**
```bash
grep -c 'scholar\.google\.com' rmedia-site/public/index.html
# Expected: 0
# Contract fails if count ≥ 1
```

**Rationale:** Google Scholar serves reCAPTCHA to non-whitelisted browsers.
A visitor clicking the link sees a CAPTCHA challenge, not Noah's publication
list. This is trust-damaging and invisible to server-side URL verifiers.

**Edge cases:**
- `scholar.google.com` appearing in `<code>` or `<pre>` blocks as example
  text → acceptable. Falsifier checks only `<a href>` attrs. Refine by
  piping through `htmlq 'a[href]' --attribute href` before grep.
- Subdomains like `scholar.googleapis.com` — not affected, the regex is
  anchored to `scholar.google.com`.

**Remediation:** Replace Google Scholar links in `gen_panes.lua` with the
Coursera instructor profile (`coursera.org/instructor/noahgift`) or a
static publications page.

---

## SITE_003 — No `scholars.duke.edu` links (hard gate)

**Claim:** `scholars.duke.edu` does not appear as a link target in any pane.

**Input:** `rmedia-site/public/index.html`

**Falsifier:**
```bash
grep -c 'scholars\.duke\.edu' rmedia-site/public/index.html
# Expected: 0
# Contract fails if count ≥ 1
```

**Rationale:** Duke retired `scholars.duke.edu/person/Noah.Gift` without
a redirect. The URL returns 200 with a 404 HTML body, defeating HTTP-level
URL verification.

**Edge cases:**
- `duke.edu` (root domain) and `fuqua.duke.edu` (business school) remain
  valid and are not blocked.
- Old blog posts linking `scholars.duke.edu` internally — not affected (this
  contract only inspects `public/index.html`, not archived posts).

**Remediation:** Use `coursera.org/partners/duke` (see SITE_004).

---

## SITE_004 — Duke link uses Coursera partner URL (hard gate)

**Claim:** At least one `<a href>` in `public/index.html` points to
`https://www.coursera.org/partners/duke`.

**Input:** `rmedia-site/public/index.html`

**Falsifier:**
```bash
grep -c 'coursera\.org/partners/duke"' rmedia-site/public/index.html
# Expected: ≥ 1
# Contract fails if count == 0
```

**Rationale:** The Bio pane references Noah's Duke affiliation. After
retiring the Scholars profile, the Coursera partner page is the canonical
Duke destination with stable URL guarantees from Coursera.

**Edge cases:**
- **The wrong form `/partners/duke-university`** — Coursera uses the short
  slug `duke`, not the long one. A previous session introduced the long
  form across 27 config files; this contract catches a regression.
- Whitespace / quote variations: the falsifier matches `"` as terminator to
  avoid matching `duke-university` which ends with `-` before the close quote.

**Remediation:** `gen_panes.lua` line 100 — set `href =
"https://www.coursera.org/partners/duke"`.

---

## SITE_005 — Coursera instructor link exists (soft warn)

**Claim:** At least one `<a href>` points to
`https://www.coursera.org/instructor/noahgift`.

**Input:** `rmedia-site/public/index.html`

**Falsifier:**
```bash
grep -c 'coursera\.org/instructor/noahgift' rmedia-site/public/index.html
# Expected: ≥ 1
# Soft warn if count == 0 (does not block deploy)
```

**Rationale:** The instructor profile aggregates all of Noah's Coursera
courses with stable URL. It's the primary CTA for prospective learners; its
removal would be a regression but not necessarily a deploy blocker (e.g.,
during a planned migration to a new primary CTA).

**Edge cases:** Username changes (e.g., `noah-gift` vs `noahgift`) would
trip this. Update the contract string alongside the CTA move.

**Remediation:** `gen_panes.lua` (pane-2 Social section) — preserve the
Coursera instructor link.

---

## SITE_006 — Deterministic build (hard gate)

**Claim:** Running `rmedia site build` twice with the same inputs produces
bit-identical `public/index.html`.

**Input:** `rmedia-site/content/pane-*.html`, `rmedia-site/templates/index.tmpl`

**Falsifier:**
```bash
sha_a=$(sha256sum rmedia-site/public/index.html | awk '{print $1}')
rmedia site build > /dev/null
sha_b=$(sha256sum rmedia-site/public/index.html | awk '{print $1}')
test "$sha_a" = "$sha_b"
# Exit 0 iff identical
```

**Rationale:** Non-deterministic builds (e.g., build timestamp embedded in a
comment) defeat cache invalidation and make it impossible to audit which
content shipped. The site build should be a pure function of its inputs.

**Edge cases:**
- Embedded timestamp in `<!-- built {timestamp} -->` comment — must be
  removed or pinned to a reproducible source (e.g., `git log -1
  --format=%ct`).
- File ordering in directory globs — Lua `io.dir` ordering is filesystem-
  dependent. Sort before iterating.

**Remediation:** Remove non-deterministic regions from
`templates/index.tmpl` and the build pipeline. Open a follow-up ticket if
reproducibility isn't yet achievable.

---

## SITE_007 — Generator is source of truth (soft warn)

**Claim:** `scripts/gen_panes.lua` produces the pane HTML currently in
`content/pane-*.html`. No hand-edits linger.

**Input:**
- `rmedia-site/scripts/gen_panes.lua`
- `rmedia-site/content/pane-{0,1,2}.html`

**Falsifier:**
```bash
rmedia site build --regen-panes  # regenerates content/pane-*.html
git diff --exit-code rmedia-site/content/
# Exit 0 iff working tree clean after regen
```

**Rationale:** A hand-edit to a pane file is lost on the next
`gen_panes.lua` run. The 2026-04-18 audit found that the Duke URL fix had
been made via `sed` against the pane HTML without updating the generator,
and the pane was regenerated by a later build — re-introducing the bug.

**Edge cases:**
- Trailing-newline differences from editors — run `dos2unix` before diff.
- Contributors may batch manual + generator edits in one commit. Accept a
  single-commit window where `git diff` is dirty, but CI must see a clean
  tree in merged state.

**Remediation:** Always fix URL issues in `gen_panes.lua` first; regenerate
panes; commit both.

---

## SITE_008 — `apr probar comply` backstop (hard gate)

**Claim:** `apr probar comply public/ --checks C002,C003,C004,C005,C007,C008,C009,C010`
returns exit 0.

**Input:** `rmedia-site/public/` directory

**Falsifier:**
```bash
apr probar comply rmedia-site/public/ \
  --checks C002,C003,C004,C005,C007,C008,C009,C010 \
  --fail-fast \
  --format text
# Exit 0 iff all 8 checks pass
```

**Rationale:** Independent of site-specific contracts, the probador C001–C010
compliance suite catches regressions in WASM-adjacent concerns (custom
elements, cache, panic paths) that SITE_001–SITE_007 don't cover.

**C001 + C006 rationale (why excluded from `--checks`):**
- **C001 "Code execution verified":** requires a runnable WASM module. The
  static site has no WASM today; once spec 011 (`rmedia wos` WASM shell)
  ships, C001 enters the hard gate.
- **C006 "COOP/COEP headers":** only matters when serving WASM with
  `SharedArrayBuffer`. Static HTML can't emit these headers meaningfully.

**Strict tightening rule:** any future spec that widens `--checks` (adds
more check IDs) represents a tightening of the contract. Narrowing
(removing a check ID) requires a new spec revision and explicit rationale.

**Edge cases:**
- New probador check IDs (C011+) are not auto-included — this spec must be
  revised to add them to `--checks`.
- `apr` not on `PATH` in CI — gate with `which apr || { echo "install
  apr-cli"; exit 10; }`.

**Remediation:** Per-check runbook:
- C002 console-errors → audit `<script>` blocks for unhandled rejections
- C003 custom-elements → check `customElements.define` calls in HTML
- C004 threading → verify no `SharedArrayBuffer` usage without COOP/COEP
- C005 low-memory → verify no large synchronous allocations on page load
- C007 replay-hash → verify `replay.json` hash matches on reload
- C008 cache → verify `Cache-Control` headers on static assets
- C009 WASM-size → verify no `.wasm` files exceed 5MB (N/A for static site)
- C010 panic-paths → verify no `panic!` / `unreachable!` surfaces in WASM

Most of these are trivially satisfied by a static HTML site; the contract
exists to catch regressions if JS/WASM is progressively added.

---

## SITE_009 — Bio distinguishes current from former faculty (hard gate)

**Claim:** The Bio pane separates **current** faculty affiliations (Duke,
UC Davis — verified in 2026-04 via Coursera partner page + UC Davis GSM
faculty directory) from **former** lecturer roles (UC Berkeley iSchool —
"Former Lecturer" per iSchool people page, last Summer 2019; Northwestern
SPS — not listed on current program faculty pages).

**Input:** `rmedia-site/public/index.html`

**Falsifier:**
```bash
if grep -qE 'Faculty at [^.]*UC Berkeley' public/index.html; then
  exit 9
fi
if grep -qE 'Faculty at [^.]*Northwestern' public/index.html; then
  exit 9
fi
# Correct shape: "Faculty at Duke and UC Davis. Former lecturer at UC Berkeley iSchool and Northwestern SPS."
```

**Rationale:** Claiming current faculty appointments that aren't current is a
factual defect. The upstream pages (Berkeley iSchool, Northwestern SPS) are
the ground truth; when their content changes, this contract must be
re-verified. WebFetch evidence is cached in the commit that landed the fix.

**Edge cases:**
- If UC Berkeley lists Noah as current lecturer again, update both
  `gen_panes.lua:139-144` and this contract's falsifier.
- If Northwestern SPS lists Noah on a faculty page, cite that page and
  update the bio.

**Remediation:** `gen_panes.lua:139-144` — keep the "Faculty at ... Former
lecturer at ..." split. Never regress to "Faculty at ..., UC Berkeley, ...,
Northwestern".

---

## SITE_010 — Meta description sourced from site.toml, not template (hard gate)

**Claim:** The `<meta name="description">` content reflects
`rmedia-site/site.toml:description` (or `scene.prs metadata.description`),
not a literal hardcoded in `rmedia/crates/rmedia-cli/src/site/terminal_template.html`.

**Input:** `rmedia-site/public/index.html`

**Falsifier:**
```bash
if grep -q '90+ Coursera' public/index.html; then
  exit 10
fi
# Correct: "79 Coursera courses across 10 specializations"
# (or whatever count matches the current fixtures.lua catalog)
```

**Rationale:** The 2026-04-18 audit found the meta description hardcoded as
"Faculty at Duke, UC Berkeley, UC Davis, and Northwestern. 90+ Coursera
courses." — contradicting `site.toml` which correctly declared 79 courses
across 10 specializations. Shipping a hardcoded inaccurate meta description
damages SEO and trust.

**Fix (landed 2026-04-18):**
1. `terminal_template.html:9`: `<meta name="description" content="{{ description }}">`
2. `site/build.rs:build_terminal_context`: insert `description` from
   `scene.metadata.description` with fallback to `config.description`.
3. `site.toml` + `scene.prs`: set canonical description.

**Edge cases:**
- When the course count changes (new Coursera course), update
  `site.toml:description` + `scene.prs metadata.description`. This
  contract asserts the string is plumbed through, not the specific count.
- Minification strips whitespace around `content=` but preserves the
  substring; grep is robust to this.

**Remediation:** Remove any literal description from the Tera template;
always pipe through `{{ description }}`.

---

## Summary table

| ID        | Gate  | Command (abbreviated)                                                    |
|-----------|-------|--------------------------------------------------------------------------|
| SITE_001  | Hard  | `rmedia site verify-urls`                                                |
| SITE_002  | Hard  | `grep -c 'scholar\.google\.com' public/index.html` == 0                  |
| SITE_003  | Hard  | `grep -c 'scholars\.duke\.edu' public/index.html` == 0                   |
| SITE_004  | Hard  | `grep -cE 'coursera\.org/partners/duke[">]' public/index.html` ≥ 1           |
| SITE_005  | Soft  | `grep -c 'coursera\.org/instructor/noahgift' public/index.html` ≥ 1      |
| SITE_006  | Hard  | Two builds produce identical sha256                                      |
| SITE_007  | Soft  | `git diff --exit-code content/` after regen                              |
| SITE_008  | Hard  | `apr probar comply public/ --skip-checks C001,C006`                      |
| SITE_009  | Hard  | `grep -qE 'Faculty at [^.]*UC Berkeley\|Northwestern' public/index.html` (false) |
| SITE_010  | Hard  | `grep -q '90+ Coursera' public/index.html` (false)                       |
