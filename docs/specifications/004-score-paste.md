# Spec 004 — `score <paste>`

**Status:** Draft
**Priority:** P0
**Depends on:** 000, 001
**Owner:** Noah Gift

---

## 1. Problem

noahgift.com advertises `rmedia coursera-score` as a 7-dimension weighted scorer used
to grade every course artifact before it ships on Coursera. Visitors cannot try it.
The tool is stranded in a Rust binary on Noah's laptop.

If the bio says "I built a tool that grades courses A-F with 11 critical defects" — the
site should let a visitor paste a markdown outline and get their own A-F grade, using the
same code, running locally, with no upload of their content to any server.

## 2. Non-goals

- Grading arbitrary prose. Scope is course-marketing artifacts: outlines, about pages,
  key-terms files, reflections, roleplays.
- Server-side grading. Pasted content must never leave the browser.
- Parity with the full `rmedia coursera-score` CLI on first release. P0 scope is the
  three most-used artifact types: outline, about, key-terms.

## 3. User story

As a Coursera instructor drafting a new course outline, I paste my draft markdown into
the WOS pane, type `score`, and within 500 ms see "Composite: B+ (85/100)", a list of
critical defects, per-dimension scores, and the exact fix recommendations — identical to
what `rmedia coursera-score` CLI would emit.

## 4. Design

### 4.1. Surface

```
$ score                              # read stdin (paste then Ctrl-D, or pipe)
$ score --as outline                 # hint the artifact type
$ score --as about
$ score --as key-terms
$ score --detail                     # per-dimension breakdown + defect list
$ score --json                       # structured output for `| prove` chains
$ score --example outline            # print example artifact that scores A+
```

Exit codes:
- `0` — composite grade ≥ C (passing)
- `1` — composite grade below C
- `2` — usage error / unknown artifact type
- `3` — scorer panic (should never happen)

Example:
```
$ cat my-outline.md | score --as outline --detail
Composite:        85.0  (B+)
  Content Quality:  90.0  ⓘ arXiv citations present, Bloom verbs used
  Structure:        88.0  ⓘ 7 modules × ~5 lessons, good pacing
  Grounding:        95.0  ⓘ 100% of terms traceable to transcripts
  Coverage:         75.0  ⚠ module 4 has only 2 lessons
  SVG Quality:       —    (not applicable for outline)
  Falsifiability:   80.0
  Citation Diversity: 82.0 ⚠ arXiv 2303.08774 appears in 4 lessons (cap 3)
critical defects:  0
warnings:          2
fix-loop:
  1. Expand module 4 with 3 more lessons to reach the ≥4-lesson floor.
  2. Rotate 1 of the citations of 2303.08774 out to another arXiv paper.
```

### 4.2. Data contract

**Input:** any markdown text. Inferred artifact type from leading headers if `--as` not
supplied.

**Output (JSON):**

```json
{
  "composite": 85.0,
  "grade": "B+",
  "dimensions": {
    "content_quality":    { "score": 90.0, "weight": 0.25, "notes": [...] },
    "structure":          { "score": 88.0, "weight": 0.15, "notes": [...] },
    "grounding":          { "score": 95.0, "weight": 0.20, "notes": [...] },
    "coverage":           { "score": 75.0, "weight": 0.15, "notes": [...] },
    "falsifiability":     { "score": 80.0, "weight": 0.10, "notes": [...] },
    "citation_diversity": { "score": 82.0, "weight": 0.15, "notes": [...] }
  },
  "critical_defects": [],
  "warnings": [
    { "code": "COVERAGE_UNDERFLOOR", "module": 4, "detail": "..." },
    { "code": "CITATION_OVERUSE",    "paper": "2303.08774", "count": 4 }
  ],
  "fix_loop": [ ... ordered remediations ... ],
  "artifact_type": "outline",
  "duration_ms": 87
}
```

### 4.3. Implementation sketch

**Source:** `../../../rmedia/crates/rmedia-scoring/` (existing — must be WASM-compiled).

**Plan:**
1. Audit `rmedia-scoring` for non-WASM-friendly dependencies (filesystem I/O, HTTP,
   spawn). Gate each behind a feature flag so the WASM build excludes them.
2. The arXiv URL validation dimension normally does HTTP. In WASM, default to
   `skip_arxiv_validation = true` with a `--validate-arxiv` flag that uses `fetch()`
   for each arXiv URL (subject to CORS — may not be usable in practice).
3. Expose a `score_paste(markdown: &str, artifact_type: Option<&str>) -> ScoreResult`
   function from a thin WASM shim (`rmedia-wos-score`).

**WASM shim:**

```rust
#[wasm_bindgen]
pub struct WasmScorer {
    config: ScoringConfig,
}

#[wasm_bindgen]
impl WasmScorer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { config: ScoringConfig::default_for_web() }
    }
    pub fn score(&self, markdown: &str, artifact_type: Option<String>) -> JsValue {
        let kind = artifact_type
            .as_deref()
            .map(ArtifactKind::parse)
            .unwrap_or_else(|| ArtifactKind::infer(markdown));
        let result = rmedia_scoring::score_text(markdown, kind, &self.config);
        serde_wasm_bindgen::to_value(&result).unwrap()
    }
}
```

**WOS command handler:**

```rust
struct ScoreHandler { scorer: WasmScorer }

impl CommandHandler for ScoreHandler {
    fn run(&self, stdin: &[u8], argv: &[String]) -> CommandOutput {
        let md = std::str::from_utf8(stdin).unwrap_or("");
        if md.is_empty() { return usage_error(argv); }
        let artifact_type = parse_flag(argv, "--as");
        let detail = argv.contains(&"--detail".into());
        let json = argv.contains(&"--json".into());
        let result = self.scorer.score(md, artifact_type);
        // render per flags …
    }
}
```

## 5. Contracts produced

```json
{
  "id": "SCORE_WASM_PARITY",
  "claim": "In-browser scorer agrees with rmedia CLI on the gold corpus (±1.0 point)",
  "value": true,
  "source": "rmedia-scoring/tests/gold_corpus/",
  "falsified_if": {
    "kind": "sha256",
    "path": "score/parity-report.json",
    "hash": "<CI-bound>"
  }
}
```

```json
{
  "id": "SCORE_SELF_GRADE",
  "claim": "noahgift.com outline scores A+ on its own scorer",
  "value": "A+",
  "source": "https://noahgift.com/content.sh.out",
  "falsified_if": {
    "kind": "jq_count",
    "target": ".self_grade_ok",
    "op": "==",
    "value": 1
  }
}
```

Note: SCORE_SELF_GRADE is the dogfooding contract required by master spec P7. The site's
own outline (derived from its content, reformatted as a course-outline artifact) is
scored and must be A+.

## 6. Falsification recipe

```bash
# Parity: run the CLI and the WASM on the same input, compare composites.
echo '...gold outline...' > /tmp/gold.md

# CLI
cli_composite=$(rmedia coursera-score -c /tmp/cfg.lua /tmp/gold.md --json | jq .composite)

# WASM via headless Chrome (Playwright) — see tests/parity/score.spec.ts
wasm_composite=$(node tests/parity/score.js /tmp/gold.md | jq .composite)

diff=$(echo "$cli_composite - $wasm_composite" | bc -l)
awk -v d="$diff" 'BEGIN { exit !(d*d <= 1.0) }'
```

```bash
# Self-grade: the site's own artifact
curl -s https://noahgift.com/self-outline.md > /tmp/self.md
node tests/parity/score.js /tmp/self.md --json | jq -e '.grade == "A+"'
```

## 7. Performance budget

| Metric                            | Target         |
| --------------------------------- | -------------- |
| WASM module size (gzipped)        | ≤ 400 KB       |
| Cold score call (load + run)      | ≤ 800 ms       |
| Warm score call (typical outline) | ≤ 150 ms       |
| Warm score call (large key-terms) | ≤ 300 ms       |

## 8. Failure modes and fallback

| Failure                             | Behavior                                        |
| ----------------------------------- | ----------------------------------------------- |
| Unknown artifact type               | Auto-infer, warn in stderr                      |
| Markdown parser panic               | Catch; exit 3; emit "paste issue" with location |
| arXiv validation unreachable        | Soft-skip; emit warning; continue with 0 points on that sub-check |
| scorer mismatch with CLI > 1.0 pt   | CI fails; SCORE_WASM_PARITY contract falsified  |

## 9. Open questions

- **Q1.** Do we ship the full scorer (all 7 dimensions + 11 critical defects) or a
  reduced web variant? Full is ideal for parity; reduced is smaller. Lean full; monitor
  payload in perf budget.
- **Q2.** Is the `--validate-arxiv` flag worth implementing given CORS on
  `export.arxiv.org`? Likely not — defer to v0.2.
- **Q3.** Should `score` with no `--as` flag attempt to infer from structure or require
  explicit kind? Lean infer with a stderr note "(inferred as outline from headers)".

## 10. References

- Current scorer: `../../../rmedia/crates/rmedia-scoring/`
- Scoring docs: `../../../rmedia/docs/src/coursera-marketing.md`
- Master spec P7 (dogfood): `./README.md`
- Spec 008 (audit site uses this): `./008-audit-site.md`
