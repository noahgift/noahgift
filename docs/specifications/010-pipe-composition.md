# Spec 010 — `pipe` — command composition

**Status:** Draft
**Priority:** P0
**Depends on:** 000, 001
**Owner:** Noah Gift

---

## 1. Problem

Every command in specs 001-009 is useful in isolation, but the *killer* interaction of a
terminal is composition. A visitor who can type

```
grep course "llm" | score --as-spec | prove
```

— "find courses about LLMs, score them as a specialization proposal, verify the scoring
passes" — gets an interactive proof of something no static page could ever say. Without
pipes, we have ten independent tools; with pipes, we have a *runtime*.

## 2. Non-goals

- Full POSIX shell semantics. No job control, no environment variables, no backticks.
- Arbitrary WASM plugins. Only commands registered by `rmedia-wos-*` crates.
- Unbounded stream sizes. Pipe buffers capped at 4 MB; exceeding cap errors cleanly.
- Subshells, `$(...)`. Single-line pipelines only.

## 3. User story

As a visitor exploring the catalog, I type
`grep course "llm" -n 5 --json | score --as-spec --detail | prove PROVE_SCHEMA_VERSION`
and watch WOS chain three WASM commands through stdin/stdout — returning in ≤ 600 ms
with a composite grade at the end. No page navigation, no network round-trip after
initial load.

## 4. Design

### 4.1. Surface

```
cmd1 | cmd2 | cmd3            # stdout of N → stdin of N+1
cmd1 | cmd2 | tee fname | cmd3  # fan-out (v0.2 — not P0)
cmd1 ; cmd2                   # sequential, no pipe (v0.2)
cmd1 && cmd2                  # conditional on exit 0 (v0.2)
```

P0 scope: **single-line, linear pipes only**. No `tee`, `;`, `&&`, `||`. No redirection.

### 4.2. Command interface

Every WOS command implements:

```rust
pub trait WosCommand: Send + Sync {
    fn name(&self) -> &'static str;
    fn usage(&self) -> &'static str;

    /// Run with a single stdin buffer and a shared context.
    /// Returns exit code, stdout bytes, stderr bytes.
    fn run(&self, stdin: &[u8], argv: &[String], ctx: &WosContext) -> CommandOutput;
}

pub struct CommandOutput {
    pub exit:   u8,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
```

The `WosContext` carries cached fetches (`contracts.json`, `fixtures.json`) and a
perf clock. It's shared across a pipeline so the first `prove` in a chain warms up the
cache for downstream commands.

### 4.3. Pipeline executor

```rust
pub fn exec_pipeline(line: &str, ctx: &WosContext) -> PipelineResult {
    let stages = parse_pipeline(line)?;          // Vec<(name, argv)>
    let mut buf = Vec::<u8>::new();              // seed stdin is empty

    for (i, (name, argv)) in stages.iter().enumerate() {
        let cmd = ctx.registry.get(name).ok_or(err_unknown(name))?;
        let out = cmd.run(&buf, argv, ctx);

        if out.exit != 0 && i + 1 < stages.len() {
            // bail: do not feed failure into next stage
            return PipelineResult::aborted(i, out);
        }
        stream_stderr_to_wos(&out.stderr);        // always show stderr live
        buf = out.stdout;
    }

    PipelineResult::ok(buf)
}
```

**Back-pressure:** WASM synchronous execution means commands run sequentially — no true
streaming. For v0.1 this is fine (buffers are small); streaming is deferred.

**Size cap:**

```rust
const MAX_PIPE_BUFFER: usize = 4 * 1024 * 1024;    // 4 MB

if out.stdout.len() > MAX_PIPE_BUFFER {
    return Err(err_pipe_overflow(out.stdout.len()));
}
```

### 4.4. Parser

Recursive-descent, zero dependencies:

```rust
#[derive(Debug)]
pub struct Stage {
    pub name: String,
    pub argv: Vec<String>,
}

pub fn parse_pipeline(line: &str) -> Result<Vec<Stage>, ParseError> {
    let mut stages = Vec::new();
    for raw in split_top_level(line, '|') {
        let mut iter = shlex::split(raw).ok_or(ParseError::QuoteMismatch)?.into_iter();
        let name = iter.next().ok_or(ParseError::EmptyStage)?;
        stages.push(Stage { name, argv: iter.collect() });
    }
    if stages.is_empty() { return Err(ParseError::Empty); }
    Ok(stages)
}
```

`shlex` handles single- and double-quoted args. Escape `\|` to use a literal pipe.

### 4.5. Canonical pipelines

The site advertises these as example commands in the WOS "try one of these" panel:

```
prove --all --json                    | jq '.[] | select(.passed == false)'
grep course "rust" -n 3 --json        | score --as outline
diff fixtures --json                  | prove FIXTURES_IN_SYNC
ask "simd cosine" --json              | jq '.passages[0].citation'
audit site --json                     | jq '{grade, composite}'
```

These go in a `public/cookbook.json` shown in pane 0.

### 4.6. `jq` as a first-class WASM citizen

The cookbook uses `jq`. Ship a small WASM jq (`jqjs` or `jq-web`) as a registered WOS
command so visitors can actually run these chains. Not a cheat — jq is the right tool
for JSON filtering and we already assume it in the falsifier grammar.

Budget: jq WASM is ~400 KB gzipped.

## 5. Contracts produced

```json
{
  "id": "PIPE_MAX_STAGES",
  "claim": "Pipeline executor supports ≥ 6 stages in a single line",
  "value": 6,
  "source": "rmedia-wos-pipe/tests/many_stages.rs",
  "falsified_if": { "kind": "sha256", "path": "pipe/stages-proof.log", "hash": "<bound>" }
}
```

```json
{
  "id": "PIPE_BUFFER_CAP_MB",
  "claim": "Inter-stage buffer cap is 4 MB",
  "value": 4,
  "source": "rmedia-wos-pipe/src/lib.rs:MAX_PIPE_BUFFER",
  "falsified_if": { "kind": "jq_count", "target": ".cap_mb", "op": "==", "value": 4 }
}
```

```json
{
  "id": "PIPE_COOKBOOK_COUNT",
  "claim": "Cookbook ships at least 5 canonical pipelines",
  "value": 5,
  "source": "public/cookbook.json",
  "falsified_if": { "kind": "jq_count", "target": ".pipelines | length", "op": ">=", "value": 5 }
}
```

## 6. Falsification recipe

```bash
# Smoke the cookbook
for pipeline in $(curl -s https://noahgift.com/cookbook.json | jq -r '.pipelines[]'); do
  # Run via headless Playwright — exec each pipeline, expect exit 0
  node tests/e2e/pipeline-smoke.js "$pipeline" || exit 1
done

# Max stages
node tests/e2e/pipeline-smoke.js \
  "prove --all --json | jq '.[0:10]' | jq 'length' | jq '. + 1' | jq '. * 2' | jq '.'"

# Overflow
node tests/e2e/pipeline-smoke.js \
  "grep course 'x' -n 1000 --json | score --as outline" \
  && echo "expected overflow" && exit 1 || echo "✓ overflow caught"
```

## 7. Performance budget

| Metric                                 | Target       |
| -------------------------------------- | ------------ |
| Parser (10-stage line)                 | ≤ 1 ms       |
| 3-stage pipeline (prove | jq | jq)     | ≤ 50 ms      |
| 6-stage pipeline                       | ≤ 200 ms     |
| jq WASM size (gzipped)                 | ≤ 450 KB     |
| Pipe executor overhead per stage       | ≤ 2 ms       |

## 8. Failure modes and fallback

| Failure                            | Behavior                                         |
| ---------------------------------- | ------------------------------------------------ |
| Unknown command                    | Fail with `command not found: X`; exit 2         |
| Pipe buffer overflow (> 4 MB)      | Abort pipeline; exit 3; hint to filter upstream  |
| Stage exits non-zero               | Do not feed into next stage; surface exit code   |
| Parser error (quote mismatch etc.) | Exit 2; highlight the problem column in stderr   |
| Circular registration (cmd → cmd)  | Prevented at registration; panics at boot (dev)  |

## 9. Open questions

- **Q1.** Does `| tee <name>` write to a real download or to a virtual FS path visible
  in WOS? Lean virtual FS; download is a separate `save` command.
- **Q2.** Should stderr be merged, interleaved, or buffered until exit? Lean stream
  stderr live, buffer stdout until the stage exits.
- **Q3.** Are pipelines themselves hashable as claims? e.g.,
  "this exact cookbook pipeline produces exit 0 against live site". Could be a contract
  shape. Defer to v0.2.
- **Q4.** Do we expose `--trace` that shows per-stage timing? Yes — it's a cheap
  debugging aid and visible-timing is on-brand. Ship with v0.1.

## 10. References

- Every other sub-spec — pipe composes all of them
- WOS source: `../../../rmedia/crates/rmedia-wos/` (to be created)
- Cookbook publishing: `public/cookbook.json` (new CI output)
- Master spec dependency graph: `./README.md` §"Dependency graph"
