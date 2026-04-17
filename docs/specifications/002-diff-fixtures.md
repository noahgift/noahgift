# Spec 002 — `diff fixtures`

**Status:** Draft
**Priority:** P0
**Depends on:** 000, 001
**Owner:** Noah Gift

---

## 1. Problem

noahgift.com derives its catalog from `course-studio/config/fixtures.lua`, but today that
derivation happens at build time and leaves no cryptographic record. If `fixtures.lua`
updates on course-studio (e.g., a new specialization) and noahgift.com is not rebuilt,
the site silently drifts from the source of truth. A visitor has no way to tell.

## 2. Non-goals

- Writing back to `fixtures.lua`. This is read-only verification.
- Synchronous rebuild-on-drift. Detection, not self-healing.
- Partial-tree diffing of JSON arrays with structural edits. A hash mismatch is the
  signal — the remediation is "rebuild the site".

## 3. User story

As a visitor who saw a tweet about a new specialization, I type `diff fixtures` and the
site tells me whether the catalog it's currently serving matches the latest committed
`fixtures.lua` on `main` of course-studio — within 300 ms.

## 4. Design

### 4.1. Surface

```
$ diff fixtures                   # compare local fixtures.json to upstream hash
$ diff fixtures --verbose         # also show per-section drift (spec count, guided count)
$ diff fixtures --upstream <url>  # override upstream source (for testing)
$ diff fixtures --json            # structured output for piping
```

Exit codes:
- `0` — local matches upstream hash
- `1` — drift detected
- `2` — upstream unreachable (treated as indeterminate, not a claim failure)

Example output:
```
$ diff fixtures
local  fixtures.json: d3a4f8...b22c (2026-04-17T17:31Z)
upstream fixtures.lua: d3a4f8...b22c (main @ 4a2b9e3)
  ✓ MATCH — site is in sync with source of truth
```

On drift:
```
$ diff fixtures
local  fixtures.json: d3a4f8...b22c (2026-04-17T17:31Z)
upstream fixtures.lua: 9f81c0...ae11 (main @ 7c8d112 — 2026-04-18T09:02Z)
  ✗ DRIFT — 17h behind upstream
  probable change: specializations count 10 → 11  (new: "Cybersecurity Ethics")
  remediation: rebuild noahgift-website via `make publish`
```

### 4.2. Data contract

**Local artifact** — the site ships `fixtures.json` (CI-emitted JSON projection of
`fixtures.lua`) and `fixtures.hash`:

```
fixtures.json           # full JSON projection
fixtures.hash           # { "sha256": "…", "source_commit": "4a2b9e3", "built_at": "2026-04-17T17:31Z" }
```

**Upstream source.** Two options, decision in master spec §"Open architectural questions":

**Option A — S3-hosted, course-studio CI pushes:**
- `https://fixtures.noahgift.com/latest.hash` (small JSON, ~200 B)
- Pushed to S3 by course-studio CI on every merge to main

**Option B — GitHub raw content:**
- `https://raw.githubusercontent.com/paiml/course-studio/main/config/fixtures.lua`
- Computes hash client-side; Lua-to-JSON normalization required

**Recommended:** Option A. Reasons: (1) latency — S3 regional cache < 50 ms vs GitHub
raw CDN variable; (2) no CORS concerns; (3) hashing happens once in CI, not per
visitor. Trade-off: requires setting up the S3 bucket + CI job, but that's one-time.

### 4.3. Implementation sketch

**New crate:** shares `rmedia-wos-prove` (spec 001) — reuses the fetch + SHA-256 plumbing.

**Flow:**
1. Fetch `/fixtures.hash` (local) via same-origin GET.
2. Fetch `https://fixtures.noahgift.com/latest.hash` (upstream) via CORS-enabled GET.
3. Compare `sha256` fields.
4. On mismatch and `--verbose`, fetch `/fixtures.json` and `https://fixtures.noahgift.com/latest.json`,
   compute per-section counts, list differences.

**Hash binding in CI (course-studio):**

```yaml
# .github/workflows/fixtures-publish.yml
name: Publish fixtures
on:
  push:
    branches: [main]
    paths: [config/fixtures.lua]
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Lua → JSON
        run: lua5.1 scripts/fixtures_to_json.lua > latest.json
      - name: Hash
        run: jq -n --arg h "$(sha256sum latest.json | awk '{print $1}')" \
                   --arg c "$GITHUB_SHA" \
                   --arg t "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
                   '{sha256:$h, source_commit:$c, built_at:$t}' > latest.hash
      - name: Upload
        run: |
          aws s3 cp latest.hash s3://fixtures.noahgift.com/latest.hash
          aws s3 cp latest.json s3://fixtures.noahgift.com/latest.json
```

**New Lua helper:** `course-studio/scripts/fixtures_to_json.lua`

```lua
#!/usr/bin/env lua5.1
-- Emits fixtures.json from fixtures.lua. Deterministic output.
local F = dofile("config/fixtures.lua")
local dkjson = require("dkjson")
-- ... walk F.catalog, emit sorted, stable JSON ...
print(dkjson.encode(projection, { indent = false, keyorder = KEYORDER }))
```

Stability: keys in a known order, arrays sorted by `slug`. Any normalization changes
bump a schema version.

## 5. Contracts produced

```json
{
  "id": "FIXTURES_IN_SYNC",
  "claim": "Site fixtures.json matches upstream course-studio fixtures.lua",
  "value": true,
  "source": "fixtures.noahgift.com/latest.hash",
  "falsified_if": {
    "kind": "sha256",
    "path": "fixtures.json",
    "hash": "<upstream-bound>"
  }
}
```

```json
{
  "id": "FIXTURES_SCHEMA_VERSION",
  "claim": "fixtures.json conforms to projection schema v0.1",
  "value": "0.1",
  "source": "course-studio/scripts/fixtures_to_json.lua",
  "falsified_if": {
    "kind": "jq_count",
    "target": ".$schema_version",
    "op": "==",
    "value": 1
  }
}
```

```json
{
  "id": "FIXTURES_BUILD_AGE_HOURS",
  "claim": "fixtures.json was built within the last 168 hours (one week)",
  "value": "<=168",
  "source": "fixtures.hash.built_at",
  "falsified_if": {
    "kind": "jq_count",
    "target": ".built_at_age_hours",
    "op": "<=",
    "value": 168
  }
}
```

## 6. Falsification recipe

```bash
# Falsify FIXTURES_IN_SYNC — expect exit 0 (still in sync) or 1 (drift)
local_hash=$(curl -s https://noahgift.com/fixtures.hash | jq -r .sha256)
upstream_hash=$(curl -s https://fixtures.noahgift.com/latest.hash | jq -r .sha256)
test "$local_hash" = "$upstream_hash" && echo "✓ in sync" || echo "✗ drift"

# Falsify FIXTURES_BUILD_AGE_HOURS — expect exit 0
built_at=$(curl -s https://noahgift.com/fixtures.hash | jq -r .built_at)
age_h=$(( ( $(date -u +%s) - $(date -u -d "$built_at" +%s) ) / 3600 ))
test "$age_h" -le 168 && echo "✓ fresh" || echo "✗ stale"
```

## 7. Performance budget

| Metric                            | Target    |
| --------------------------------- | --------- |
| Cold `diff fixtures` (2 fetches)  | ≤ 300 ms  |
| Warm `diff fixtures`              | ≤ 20 ms   |
| Upstream `.hash` payload          | ≤ 300 B   |
| Upstream `.json` payload (--verbose) | ≤ 20 KB |

## 8. Failure modes and fallback

| Failure                              | Behavior                                      |
| ------------------------------------ | --------------------------------------------- |
| Upstream `.hash` fetch fails         | Exit 2 (indeterminate), hint to retry         |
| CORS denied on upstream fetch        | Fall back to `https://raw.githubusercontent.com/...` with a warning |
| Local `fixtures.json` missing (old build) | Exit 3; hint to rebuild                 |
| Upstream and local agree but local file tampered post-deploy | `sha256sum fixtures.json` vs `fixtures.hash.sha256` catches it |

## 9. Open questions

- **Q1.** Does `fixtures.noahgift.com` get its own CloudFront distribution, or share
  `noahgift.com`'s? Own is cleaner; shared is cheaper. Lean shared.
- **Q2.** Does the drift banner (persistent, auto-shown when detected) belong in this
  spec or in spec 008 (`audit site`)? Lean 008 (centralized banner logic).
- **Q3.** Should `fixtures.json` be checked into `course-studio` git alongside
  `fixtures.lua`, or stay as pure CI artifact? Lean pure artifact — avoids the "two
  sources of truth" problem.

## 10. References

- Source of truth: `../../../course-studio/config/fixtures.lua`
- Current generator: `../../../noahgift-website/rmedia-site/scripts/gen_panes.lua`
- Spec 001 (evaluator reuse): `./001-prove-claim.md`
- Master: `./README.md` §"Open architectural questions" Q2
