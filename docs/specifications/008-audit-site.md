# Spec 008 — `audit site`

**Status:** Draft
**Priority:** P0
**Depends on:** 000, 001, 002, 004, 007
**Owner:** Noah Gift

---

## 1. Problem

The site has lots of claims — 15+ contracts, 20+ URLs, replay hashes, a scoring gold
target. Today nothing checks them rollup-style. A visitor runs `prove SPEC_COUNT` and
gets one result; nobody runs everything at once. A regression where (say) two contracts
quietly fail after a fixture edit would go unnoticed until a human clicks through.

## 2. Non-goals

- Replacing CI. `audit site` runs in-browser; CI has its own pipeline.
- Bespoke alerting. Failure mode is a banner on the site, not a webhook.
- Fixing broken contracts. Detection only.

## 3. User story

As a visitor landing on noahgift.com, I want to see a compact "health line" in the
status bar that tells me the site is currently graded A+ and all 15 contracts verify. If
I type `audit site` I get the rolled-up report, and if anything fails the page shows a
red banner automatically.

## 4. Design

### 4.1. Surface

```
$ audit site                   # run everything, print report
$ audit site --json            # structured rollup
$ audit site --quick           # skip URL verification (fast, ≤100 ms)
$ audit site --since <time>    # only re-run contracts touched since given ISO time
```

Exit codes:
- `0` — composite grade ≥ A and all contracts pass
- `1` — composite grade < A OR any contract fails
- `2` — usage error
- `3` — audit itself errored (network, WASM load)

Example:
```
$ audit site
─── noahgift.com self-audit ──────────────────────────────────
contracts:     15 / 15   ✓
urls:          31 / 31   ✓  (31 checked, 0 skipped)
replay hashes: 10 / 10   ✓
self-score:    A+ (96.8 / 100)
fixtures sync: ✓ in sync with upstream (commit 4a2b9e3)
──────────────────────────────────────────────────────────────
composite grade: A+
duration:        1.8 s
```

On failure:
```
$ audit site
─── noahgift.com self-audit ──────────────────────────────────
contracts:     14 / 15   ✗  (1 failed: SPEC_COUNT expected 10, got 11)
urls:          31 / 31   ✓
replay hashes: 10 / 10   ✓
self-score:    A  (92.4 / 100)  (-4.4 for failed contract)
fixtures sync: ✗ drift (upstream sha 9f81c0...; see `diff fixtures`)
──────────────────────────────────────────────────────────────
composite grade: A
duration:        2.1 s

1 critical defect detected. Red banner injected into DOM.
```

### 4.2. Data contract

**Rollup JSON:**

```json
{
  "grade": "A+",
  "composite": 96.8,
  "checks": {
    "contracts":      { "passed": 15, "total": 15, "failed_ids": [] },
    "urls":           { "passed": 31, "total": 31, "failed": [], "skipped": 0 },
    "replay_hashes":  { "passed": 10, "total": 10, "failed_ids": [] },
    "self_score":     { "grade": "A+", "composite": 96.8, "dimensions": { ... } },
    "fixtures_sync":  { "in_sync": true, "local_hash": "d3a4f8...", "upstream_hash": "d3a4f8..." }
  },
  "duration_ms": 1842,
  "timestamp":   "2026-04-17T19:30:00Z"
}
```

**Banner injection (on failure):**

```html
<aside id="audit-banner" role="alert">
  <strong>site drift detected</strong>
  <span>1 of 15 contracts failed. <a href="#audit">details</a>.</span>
  <button onclick="hideBanner()">dismiss</button>
</aside>
```

### 4.3. Implementation sketch

**New crate:** `../../../rmedia/crates/rmedia-wos-audit/`

**Composition (Promise.all-style):**

```rust
pub async fn audit_site() -> AuditReport {
    let (contracts, urls, replay, score, sync) = futures::join!(
        audit_contracts(),     // calls prove::--all
        audit_urls(),          // client-side URL probes (see §4.4)
        audit_replay(),        // calls replay::--verify for each in index
        audit_self_score(),    // calls score on self-outline.md
        audit_fixtures_sync(), // calls diff fixtures
    );
    let composite = composite_grade(&contracts, &urls, &replay, &score, &sync);
    if composite.grade < "A" { inject_banner(&composite); }
    AuditReport { composite, contracts, urls, replay, score, sync, duration_ms, timestamp }
}
```

**Auto-run on page load:**

```html
<script type="module">
  import init, { audit_site } from "/audit.js";
  await init();
  const report = await audit_site();
  document.getElementById("status-bar").textContent =
    `${report.checks.contracts.passed}/${report.checks.contracts.total} contracts · grade ${report.grade}`;
</script>
```

### 4.4. URL verification in-browser

The current `verify_urls.sh` uses `curl` at build time. In-browser we have CORS limits.
Strategy:

- Same-origin URLs → `fetch(url, {method:'HEAD'})` with fallback to GET.
- External URLs (Coursera, arXiv) → emit a `prefetch` ping. If `opaque` response ≠ net
  error, treat as OK. Cannot distinguish 200 from 404 cross-origin; accept the
  limitation and rely on the build-time `url-verification.txt` for strong guarantees.
- At minimum, verify `url-verification.txt` is fresh (< 24 h old) and signed by CI.

## 5. Contracts produced

```json
{
  "id": "AUDIT_RUNS_ON_PAGE_LOAD",
  "claim": "audit site runs automatically within 3 s of page load",
  "value": true,
  "source": "index.html inline script",
  "falsified_if": { "kind": "sha256", "path": "audit.js", "hash": "<bound>" }
}
```

```json
{
  "id": "AUDIT_COMPOSITE_GRADE",
  "claim": "Current site composite grade ≥ A on self-audit",
  "value": "A",
  "source": "audit_site() at page load",
  "falsified_if": { "kind": "jq_count", "target": ".grade_is_a_or_better", "op": "==", "value": 1 }
}
```

```json
{
  "id": "AUDIT_DURATION_MS",
  "claim": "Full audit completes in under 3 seconds on a mid-range laptop",
  "value": "<=3000",
  "source": "tests/perf/audit-bench.log",
  "falsified_if": { "kind": "jq_count", "target": ".p95_duration_ms", "op": "<=", "value": 3000 }
}
```

## 6. Falsification recipe

```bash
# Audit composite grade on live site — CI gate
grade=$(node tests/e2e/audit.js https://noahgift.com | jq -r .grade)
case "$grade" in
  "A+"|"A"|"A-") echo "✓ grade $grade" ;;
  *) echo "✗ grade $grade below A"; exit 1 ;;
esac

# Audit latency
p95=$(node tests/perf/audit-bench.js | jq -r .p95_duration_ms)
test "$p95" -le 3000

# Auto-run presence
curl -s https://noahgift.com/ | grep -q "audit_site()"
```

## 7. Performance budget

| Metric                                   | Target       |
| ---------------------------------------- | ------------ |
| Audit WASM module + dependencies (gzip)  | ≤ 500 KB     |
| Full audit cold                          | ≤ 3 s        |
| Quick audit (--quick, no URL probes)     | ≤ 100 ms     |
| Page-load → audit-started delay          | ≤ 500 ms     |
| Banner DOM injection on failure          | ≤ 50 ms      |

## 8. Failure modes and fallback

| Failure                             | Behavior                                       |
| ----------------------------------- | ---------------------------------------------- |
| One sub-audit fails                 | Composite grade penalized; report continues    |
| All sub-audits fail (network)       | Show "audit could not run" with retry button   |
| WASM load timeout                   | Silent; CI still guarantees via build-time run |
| Banner injection blocked (CSP)      | Fall back to red text in status bar            |

## 9. Open questions

- **Q1.** Should banner appear for *any* failure or only for composite < A? Lean: for
  any critical defect, banner. For soft warnings, only status-bar color change.
- **Q2.** Is auto-run on page load surprising? It might surprise a visitor with 500 KB
  of additional download. Put behind a flag? Lean: auto-run the --quick variant on load
  (100 ms, no URLs), expose `audit site` to run the full version.
- **Q3.** Publish audit history? Weekly snapshots to `/audit/history.json`? Nice-to-have,
  defer to v0.2.

## 10. References

- Dogfooding principle: `./README.md` P7
- URL verification: `../../../noahgift-website/rmedia-site/scripts/verify_urls.sh`
- Composition target: `./010-pipe-composition.md`
- Self-score dependency: `./004-score-paste.md`
- Fixture sync dependency: `./002-diff-fixtures.md`
