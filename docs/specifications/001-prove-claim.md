# Spec 001 — `prove <claim-id>`

**Status:** Draft
**Priority:** P0
**Depends on:** 000
**Owner:** Noah Gift

---

## 1. Problem

`contracts.json` lists 15 claims today with `falsified_if` shell snippets that nobody runs.
The site tells a visitor "you can verify this" but provides no execution surface. As a
result, the contracts are aspirational, not load-bearing — and the site currently has no
way to detect when a claim breaks.

## 2. Non-goals

- Arbitrary shell execution. `prove` runs only the specific `falsified_if` shapes listed
  in §4.2. No generic POSIX subset. No `bash -c` passthrough.
- Verification of claims that require private data (e.g., enrollment numbers). Only
  claims derivable from public `fixtures.json` are in scope.
- Network access to arbitrary hosts. Only same-origin `GET /fixtures.json` is allowed.

## 3. User story

As a visitor curious about the "79 total Coursera courses" claim, I type
`prove TOTAL_COURSERA` and see the site fetch `fixtures.json`, recompute the number via
code running in my own browser, and display `✓ VERIFIED: 79 === 79` within 100 ms.

## 4. Design

### 4.1. Surface

```
$ prove <claim-id>                 # run one falsifier
$ prove --list                     # list all claim IDs with status
$ prove --all                      # run every falsifier, stream results
$ prove --show <claim-id>          # print the falsifier expression without running
$ prove --json <claim-id>          # emit structured result for piping
```

Exit codes:
- `0` — all requested claims verified (falsifier returned 0)
- `1` — at least one claim failed
- `2` — unknown claim ID or usage error
- `3` — runtime error (fixtures.json unreachable, WASM panic, etc.)

Example output:
```
$ prove TOTAL_COURSERA
loading contracts.json ...................... ok (3.9 KB)
loading fixtures.json ....................... ok (14.2 KB)
evaluating falsified_if ..................... ok (2 ms)
  claim:  Noah teaches 79 courses on Coursera
  value:  79
  source: course-studio/config/fixtures.lua
  expr:   len(specs[*].courses) + len(guided) + len(standalone) == 79
  result: ✓ VERIFIED  (79 === 79)
```

### 4.2. Data contract

**Input:** a claim ID from `contracts.json`.

**Contracts schema (JSON Schema, abridged):**

```json
{
  "$id": "https://noahgift.com/schemas/contract.json",
  "type": "object",
  "required": ["id", "claim", "value", "source", "falsified_if"],
  "properties": {
    "id":             { "type": "string", "pattern": "^[A-Z][A-Z0-9_]+$" },
    "claim":          { "type": "string" },
    "value":          { "type": ["number", "string", "boolean", "array"] },
    "source":         { "type": "string", "description": "file path or URL" },
    "falsified_if":   { "$ref": "#/definitions/falsifier" }
  },
  "definitions": {
    "falsifier": {
      "oneOf": [
        { "type": "object",
          "required": ["kind", "target"],
          "properties": {
            "kind":   { "const": "jq_count" },
            "target": { "type": "string", "description": "jq path expression" },
            "op":     { "enum": ["==", "!=", ">=", ">", "<=", "<"] },
            "value":  { "type": "number" }
          }
        },
        { "type": "object",
          "required": ["kind", "sum", "op", "value"],
          "properties": {
            "kind":  { "const": "jq_sum" },
            "sum":   { "type": "array", "items": { "type": "string" } },
            "op":    { "enum": ["==", "!=", ">=", ">", "<=", "<"] },
            "value": { "type": "number" }
          }
        },
        { "type": "object",
          "required": ["kind", "path", "hash"],
          "properties": {
            "kind": { "const": "sha256" },
            "path": { "type": "string" },
            "hash": { "type": "string", "pattern": "^[0-9a-f]{64}$" }
          }
        }
      ]
    }
  }
}
```

This is a strict subset. No arbitrary jq, no arbitrary shell — just a small set of
opcodes that the WASM evaluator understands.

### 4.3. Implementation sketch

**New crate:** `../../../rmedia/crates/rmedia-wos-prove/` (exposes a `prove` command
object to WOS).

**Dependencies:**
- `sha2` for SHA-256 opcode
- `serde_json` for parsing `contracts.json` + `fixtures.json`
- `reqwest` with `wasm-bindgen` feature for fetches
- Custom mini-evaluator for `jq_count` / `jq_sum` opcodes (no full jq required)

**Flow:**
1. On first `prove` invocation, lazily fetch `/contracts.json` and `/fixtures.json`.
   Cache in a `once_cell::sync::OnceCell` for the session.
2. Look up the claim by ID.
3. Dispatch on `falsified_if.kind`:
   - `jq_count`: walk `fixtures.json` along the target path, count leaves, apply op.
   - `jq_sum`: sum counts from multiple paths, apply op.
   - `sha256`: hash the contents at `path`, compare hex.
4. Emit a structured result: `{ id, passed: bool, expected, actual, duration_ms }`.

**Example evaluator (Rust, abbreviated):**

```rust
pub struct ProveResult {
    pub id: String,
    pub passed: bool,
    pub expected: serde_json::Value,
    pub actual: serde_json::Value,
    pub duration_ms: u32,
}

pub fn prove(
    claim_id: &str,
    contracts: &serde_json::Value,
    fixtures: &serde_json::Value,
) -> Result<ProveResult, ProveError> {
    let claim = find_claim(contracts, claim_id)?;
    let expected = claim["value"].clone();
    let start = wasm_timer::Instant::now();
    let actual = match claim["falsified_if"]["kind"].as_str() {
        Some("jq_count") => eval_count(&claim["falsified_if"], fixtures)?,
        Some("jq_sum")   => eval_sum(&claim["falsified_if"], fixtures)?,
        Some("sha256")   => eval_sha256(&claim["falsified_if"], fixtures)?,
        other => return Err(ProveError::UnknownKind(other.unwrap_or("").into())),
    };
    let passed = values_equal_or_compare(&expected, &actual, &claim["falsified_if"]["op"]);
    Ok(ProveResult {
        id: claim_id.to_string(),
        passed,
        expected,
        actual,
        duration_ms: start.elapsed().as_millis() as u32,
    })
}
```

**WOS registration:**

```rust
// in rmedia-wos-prove/src/lib.rs
#[wasm_bindgen]
pub fn register_prove(wos: &mut WosRegistry) {
    wos.register(Command {
        name: "prove",
        usage: "prove <claim-id> | --list | --all | --show <id> | --json <id>",
        handler: Box::new(ProveHandler::new()),
    });
}
```

## 5. Contracts produced

Adding this command introduces three new meta-contracts to `contracts.json`:

```json
{
  "id": "PROVE_COMMAND_AVAILABLE",
  "claim": "`prove` is registered as a WOS command",
  "value": true,
  "source": "rmedia/crates/rmedia-wos-prove/src/lib.rs",
  "falsified_if": {
    "kind": "sha256",
    "path": "wos_bg.wasm",
    "hash": "<auto-bound at build time>"
  }
}
```

```json
{
  "id": "PROVE_SCHEMA_VERSION",
  "claim": "contracts.json conforms to schema v0.1",
  "value": "0.1",
  "source": "docs/specifications/001-prove-claim.md §4.2",
  "falsified_if": {
    "kind": "jq_count",
    "target": ".$schema",
    "op": "==",
    "value": 1
  }
}
```

```json
{
  "id": "PROVE_ALL_GREEN_ON_LOAD",
  "claim": "Every contract verifies on page load in CI",
  "value": true,
  "source": "CI job `contracts-smoke`",
  "falsified_if": {
    "kind": "sha256",
    "path": "ci-smoke.log",
    "hash": "<known-good>"
  }
}
```

## 6. Falsification recipe

From WOS or any browser console:

```
$ prove --all
...
15/15 verified in 48 ms.
```

From a developer machine (pre-WASM):

```bash
curl -s https://noahgift.com/contracts.json > /tmp/c.json
curl -s https://noahgift.com/fixtures.json  > /tmp/f.json
for id in $(jq -r '.claims[].id' /tmp/c.json); do
  expr=$(jq -r --arg id "$id" '.claims[] | select(.id==$id) | .falsified_if.target' /tmp/c.json)
  # ... evaluator: same semantics as WASM path ...
done
```

The CI smoke test runs this exact loop against the published site before merging any
change to `rmedia-site/`.

## 7. Performance budget

| Metric                             | Target        |
| ---------------------------------- | ------------- |
| First `prove` invocation cold      | ≤ 200 ms      |
| Subsequent `prove` (warm cache)    | ≤ 10 ms       |
| `prove --all` across 15 claims     | ≤ 150 ms      |
| Evaluator WASM size (gzipped)      | ≤ 80 KB       |

Measured via Playwright contract: `tests/perf/prove.spec.ts`.

## 8. Failure modes and fallback

| Failure                           | Behavior                                           |
| --------------------------------- | -------------------------------------------------- |
| `fixtures.json` fetch fails       | Show stderr banner; exit 3; suggest `diff fixtures` |
| Unknown claim ID                  | Exit 2 with `did-you-mean` suggestion              |
| `falsified_if.kind` unrecognized  | Exit 3; log to WOS console; do NOT crash shell     |
| SHA mismatch on `sha256` opcode   | Exit 1; show expected vs actual first 8 hex chars  |
| WASM not loaded yet               | Queue command, retry after `wos:ready` event       |

## 9. Open questions

- **Q1.** Should `prove --json` output be stable-sorted or preserve contract order?
  Leaning: stable-sorted by ID for deterministic diffs.
- **Q2.** Do we want a `prove --watch` mode that re-runs on every keystroke in another
  pane? Nice-to-have, not P0.
- **Q3.** Should the evaluator support nested `and`/`or` over opcodes, or keep every
  claim a single opcode? Keep single for v0.1.

## 10. References

- Current `contracts.json`: `https://noahgift.com/contracts.json`
- Current generator: `../../../noahgift-website/rmedia-site/scripts/gen_panes.lua`
- Master spec: `./README.md`
- Premise: `./000-premise-and-principles.md`
- Composition target: `./010-pipe-composition.md`
