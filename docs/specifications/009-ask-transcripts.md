# Spec 009 — `ask "<question>"`

**Status:** Draft
**Priority:** P2 (RAG v1 extractive; generative v2)
**Depends on:** 000, 003
**Owner:** Noah Gift

---

## 1. Problem

Course transcripts are the richest source of truth about what Noah actually teaches —
tens of thousands of sentences pinned to SRT timestamps. A visitor asking "does Noah
explain why SIMD beats scalar for cosine similarity?" cannot find the answer without
watching all 79 courses. The SRTs exist, they just aren't queryable.

## 2. Non-goals

- Open-domain Q&A. Only questions answerable from course transcripts.
- Generating answers from model memory. Extractive retrieval only in v0.1. Generation
  v0.2 is gated on a citation contract (below).
- Voice / speech interface. Text-only.
- Answering biographical questions. "Where does Noah live" is not this tool.

## 3. User story

As a prospective student, I type `ask "why does SIMD beat scalar for cosine similarity"`
and within 1 second see one or more passages extracted from course transcripts, each
cited as `[C1 Rust Programming / Lesson 2.3.1 @ 02:14-02:47]`, with a "watch" button
that links directly to the lesson at that timestamp. If top-k similarity is below
threshold, the tool refuses to answer.

## 4. Design

### 4.1. Surface

```
$ ask "<question>"                    # extractive, top 3 passages
$ ask "<question>" -n 5               # top N
$ ask "<question>" --threshold 0.55   # tighten minimum similarity
$ ask "<question>" --json             # structured output
$ ask "<question>" --spec <slug>      # scope to one specialization
$ ask "<question>" --mode generate    # v0.2 only, requires small LLM loaded
```

Exit codes:
- `0` — ≥1 passage above threshold
- `1` — no passage above threshold ("refuse to answer")
- `2` — usage error
- `3` — index unavailable

Example:
```
$ ask "why does SIMD beat scalar for cosine similarity"
passage 1  (similarity 0.78)  [C3 Rust Programming / Lesson 2.3.1 @ 02:14-02:47]
  "SIMD lets us compute dot products eight 32-bit lanes at a time, which on a
   modern x86 or ARM core reduces the loop body from ~10 cycles per dimension to
   about 1.5 cycles. For 384-dim embeddings that's the difference between
   300 microseconds and 40 microseconds per pair — a 7× speedup we get for free
   if we use trueno instead of naive Rust."
  $ open https://coursera.org/learn/.../lesson-2-3-1?t=134

passage 2  (similarity 0.72)  ...
passage 3  (similarity 0.67)  ...
```

On refusal:
```
$ ask "what's Noah's favorite ice cream"
no passage scored above threshold 0.5.
top candidate was 0.19 ("…"). refusing to answer (see --threshold).
```

### 4.2. Data contract

**Index artifact:**

```
public/ask/chunks.json         # { id, text, spec, course, lesson, start_ms, end_ms, course_url }[]
public/ask/index.apr           # [N_chunks, 384] embedding tensor
public/ask/stats.json          # { n_chunks, n_courses, n_lessons, dim, built_at, rmedia_version }
```

**Chunking rules** (deterministic, run by `rmedia ask-index build`):

- Window: 1-4 SRT cues, up to 60 s or 400 chars, whichever hits first
- Stride: 50 % overlap
- Speaker boundaries always split (course-studio's SRTs are single-speaker but safety)
- Blank cues dropped

Expected scale (from CLAUDE.md: 79 courses × ~20-30 lessons × ~50 SRT cues): ~50-80k
chunks → at 384-dim float16, ~60-100 MB. **Too large for a single download.**

### 4.3. Implementation sketch

**Tiered loading to keep payload in budget:**

1. **Spec-scoped indices** — one index per specialization, ~5-10 MB each. Default:
   load only the currently-scoped one (user's current spec, via `ask --spec`).
2. **Global search** — user opts in to the full ~80 MB download. Gate behind explicit
   flag or UI confirmation.

**Ranking:**

```rust
pub struct AskResult {
    pub passages: Vec<Passage>,
    pub top_similarity: f32,
    pub refused: bool,
}

pub fn ask(
    query: &str,
    index: &Tensor2D<f32>,
    chunks: &[Chunk],
    model: &SentenceEncoder,
    n: usize,
    threshold: f32,
) -> AskResult {
    let qemb = model.encode(query);
    let sims = trueno::simd_matmul(index, &qemb);
    let topk = sims.argsort_desc().take(n);
    let best = topk.first().map(|i| sims[*i]).unwrap_or(0.0);

    if best < threshold {
        return AskResult { passages: vec![], top_similarity: best, refused: true };
    }
    let passages = topk.into_iter()
        .map(|i| Passage { chunk: chunks[i].clone(), similarity: sims[i] })
        .collect();
    AskResult { passages, top_similarity: best, refused: false }
}
```

**Timestamp-linked course URLs:**

Each `Chunk` carries `course_url` + `start_ms`. Render as Coursera deep-links where the
player format supports it, else fall back to the lesson landing page.

### 4.4. Generation gate (v0.2)

`ask --mode generate` is off by default. To enable, the synthesizer must:

1. Condition strictly on retrieved passages. No model-memory synthesis.
2. Output a citation for every factual claim. One claim → one or more `[Cx Lesson x.y.z
   @ mm:ss]` references.
3. Pass a citation-coverage contract: `n_cited / n_factual_claims ≥ 0.9` on a gold set.

If the gate fails, `ask` silently stays extractive.

## 5. Contracts produced

```json
{
  "id": "ASK_INDEX_CHUNK_COUNT",
  "claim": "Global ask-index has ≥ 40,000 SRT chunks across all 79 courses",
  "value": 54221,
  "source": "public/ask/stats.json",
  "falsified_if": { "kind": "jq_count", "target": ".n_chunks", "op": ">=", "value": 40000 }
}
```

```json
{
  "id": "ASK_NO_HALLUCINATION",
  "claim": "ask refuses to answer when top similarity < threshold",
  "value": true,
  "source": "tests/e2e/ask-refuse-adversarial.spec.ts",
  "falsified_if": { "kind": "sha256", "path": "ask/refuse-proof.log", "hash": "<bound>" }
}
```

```json
{
  "id": "ASK_PER_SPEC_INDEX_SIZE_MB",
  "claim": "Per-spec ask-index is ≤ 12 MB",
  "value": 9.8,
  "source": "HEAD /ask/<spec>.apr",
  "falsified_if": { "kind": "jq_count", "target": ".max_spec_mb", "op": "<=", "value": 12 }
}
```

## 6. Falsification recipe

```bash
# Index size per spec
for spec in $(curl -s https://noahgift.com/ask/index.json | jq -r '.specs[]'); do
  size=$(curl -sI "https://noahgift.com/ask/${spec}.apr" | awk '/Content-Length/{print $2}')
  [ "$size" -le 12582912 ] || { echo "✗ $spec too large ($size)"; exit 1; }
done

# Adversarial refusal: ask something not in course content
out=$(node tests/e2e/ask.js "what's the airspeed velocity of an unladen swallow" --json)
echo "$out" | jq -e '.refused == true'

# Chunk count
curl -s https://noahgift.com/ask/stats.json | jq -e '.n_chunks >= 40000'
```

## 7. Performance budget

| Metric                                         | Target        |
| ---------------------------------------------- | ------------- |
| Per-spec index size (gzipped)                  | ≤ 12 MB       |
| Global index size (gzipped)                    | ≤ 100 MB      |
| Cold ask (per-spec, index cached)              | ≤ 250 ms      |
| Warm ask (per-spec)                            | ≤ 50 ms       |
| Cold ask (global, first download)              | ≤ 15 s        |
| Chunk-text retrieval latency                   | ≤ 10 ms       |

## 8. Failure modes and fallback

| Failure                            | Behavior                                          |
| ---------------------------------- | ------------------------------------------------- |
| Index fails to load                | Show actionable error; suggest `--spec` scope     |
| All similarities below threshold   | Refuse with top candidate reported                |
| Model load fails                   | Fall back to tf-idf search over chunks (spec 003 style) |
| Timestamp unlinkable on Coursera   | Show course landing page URL, warn               |

## 9. Open questions

- **Q1.** Where are SRTs stored today for indexing? course-studio output directories
  per-course. CI needs to aggregate from all 79. A meta-artifact
  `course-studio/out/all-srts.tar.gz` may make this tractable.
- **Q2.** Which sentence encoder? Shared with spec 003. Lock the choice together.
- **Q3.** Threshold default? Empirically 0.5 feels reasonable for MiniLM. Pin via
  adversarial test set; any change must rerun the test.
- **Q4.** For generation v0.2: which tiny LLM? Llama-3.2-1B quantized is ~700 MB. Too
  large. Gemma-2B quantized ~1.1 GB. Also too large. Realistic v0.2 may be
  "server-backed but cited" (HuggingFace endpoint) if local generation stays infeasible.
  Mark this in the next version of the spec.

## 10. References

- SRT corpus: `course-studio/coursera-assets/rendered/<course>/*/*.srt`
- aprender: `../../../aprender/`
- Shared embeddings: `./003-semantic-grep.md`, `./006-recommend-path.md`
- Deterministic-fallback principle: `./README.md` P6
