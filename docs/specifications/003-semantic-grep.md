# Spec 003 — `grep course "<query>"`

**Status:** Draft
**Priority:** P1 (deterministic fallback P0)
**Depends on:** 000, 002
**Owner:** Noah Gift

---

## 1. Problem

A visitor who wants "a Rust course that teaches SIMD" has no way to discover it on
noahgift.com today. The static pane lists 10 specializations; finding the matching course
requires reading course names on Coursera or guessing. The site advertises aprender and
trueno (Rust ML + SIMD) as bio items but does not use them to help visitors search.

## 2. Non-goals

- Full-text search over course transcripts. (That's spec 009 `ask`.)
- Cross-site search (no crawling Coursera). Only fixtures + local course descriptions.
- Query understanding via a frontier LLM. Keep inference local.

## 3. User story

As a Rust engineer curious whether Noah teaches anything in my stack, I type
`grep course "rust simd gpu"` and within 500 ms see a ranked list of three courses from
the Rust Programming specialization with cosine similarity scores and one-line snippets,
plus an `--explain` flag I can use to inspect the ranking.

## 4. Design

### 4.1. Surface

```
$ grep course "<query>"              # semantic search, top 10 by default
$ grep course "<query>" -n 5         # top N
$ grep course "<query>" --mode tfidf # force deterministic fallback
$ grep course "<query>" --explain    # show ranking internals
$ grep course "<query>" --spec <slug># scope to a specific specialization
$ grep course "<query>" --json       # structured output
```

Exit codes:
- `0` — ≥1 result returned above relevance threshold
- `1` — no results above threshold (query is too narrow or off-topic)
- `2` — usage error
- `3` — index unavailable (fallback to tf-idf should prevent this in practice)

Example output:
```
$ grep course "rust simd gpu"
1.  0.78  rust-simd-accelerated-ml        [Rust Programming spec]
        "Hand-tuned SIMD kernels for machine learning inference …"
2.  0.71  advanced-fine-tuning-in-rust    [Hugging Face AI Development]
        "Fine-tune transformers with Rust + aprender. GPU optional …"
3.  0.64  rust-for-large-scale-data       [Rust Programming]
        "Parallel dataframe processing with Polars and trueno …"
```

### 4.2. Data contract

**Index artifact** (shipped with site, loaded on first `grep` invocation):

```
/search/index.apr       # aprender-format tensor (embeddings)
/search/docs.json       # { id, title, spec_slug, url, snippet }[]  -- keyed 1:1 with index rows
/search/tfidf.json      # fallback: { doc_id, term_weights }[]
/search/vocab.json      # fallback: { term → idx, idf: [] }
```

**Embedding choice:**
- aprender-native small sentence encoder (384-dim)
- OR MiniLM-L6-v2 exported to aprender `.apr` format (also 384-dim, ~22 MB quantized int8)

Decision criterion: whichever is stable and trueno-SIMD-accelerated in WASM. Default to
tf-idf for v0.1 to unblock P0 spec chain; upgrade to embeddings in v0.2.

### 4.3. Implementation sketch

**New crate:** `../../../rmedia/crates/rmedia-wos-grep/`

**Dependencies:**
- `aprender` with `wasm` feature
- `trueno` for SIMD cosine similarity (P1+)
- `unicode-segmentation` for tokenization
- `serde_json`

**Flow (tf-idf baseline):**

```rust
pub struct GrepIndex {
    vocab:   Vec<String>,           // term at idx
    idf:     Vec<f32>,              // len == vocab.len()
    docs:    Vec<DocEntry>,         // per-course tf-idf sparse vector
}

pub struct DocEntry {
    pub id:         String,
    pub title:      String,
    pub spec_slug:  String,
    pub url:        String,
    pub snippet:    String,
    pub vec:        Vec<(u32, f32)>, // sparse (term_idx, weight)
}

pub fn grep(
    query: &str,
    index: &GrepIndex,
    n: usize,
    spec_filter: Option<&str>,
) -> Vec<GrepHit> {
    let qvec  = tfidf_vectorize(query, &index.vocab, &index.idf);
    let mut  scored = index.docs
        .iter()
        .filter(|d| spec_filter.map_or(true, |s| d.spec_slug == s))
        .map(|d| (d, cosine_sparse(&qvec, &d.vec)))
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.into_iter().take(n).map(|(d, s)| d.into_hit(s)).collect()
}
```

**Flow (embedding upgrade path):**

```rust
// aprender path used when /search/index.apr loads
pub fn grep_semantic(
    query: &str,
    model: &aprender::models::SentenceEncoder,
    index: &Tensor2D<f32>,   // [N_docs, 384]
    docs:  &[DocEntry],
    n: usize,
) -> Vec<GrepHit> {
    let qemb: Tensor1D<f32> = model.encode(query);                // [384]
    let sims: Tensor1D<f32> = trueno::simd_matmul(index, &qemb);  // [N_docs]
    let topk = sims.argsort_desc().take(n);
    topk.into_iter()
        .map(|idx| GrepHit::new(&docs[idx], sims[idx]))
        .collect()
}
```

**Index build** (one-time, at site build):

`course-studio/scripts/build_search_index.lua` + rmedia subcommand
`rmedia search-index build --fixtures config/fixtures.lua --out public/search/`
→ emits tf-idf (immediate) + embeddings (once aprender encoder ready).

## 5. Contracts produced

```json
{
  "id": "SEARCH_INDEX_DOC_COUNT",
  "claim": "Search index covers all 79 courses",
  "value": 79,
  "source": "public/search/docs.json",
  "falsified_if": {
    "kind": "jq_count",
    "target": ".docs | length",
    "op": "==",
    "value": 79
  }
}
```

```json
{
  "id": "SEARCH_INDEX_SIZE_MB",
  "claim": "tf-idf index is ≤ 2 MB, embedding index is ≤ 25 MB",
  "value": { "tfidf_mb": 1.1, "embed_mb": 22.4 },
  "source": "Content-Length of /search/*.json and /search/index.apr",
  "falsified_if": {
    "kind": "jq_count",
    "target": ".size_budget_ok",
    "op": "==",
    "value": 1
  }
}
```

## 6. Falsification recipe

```bash
# Doc count
test "$(curl -s https://noahgift.com/search/docs.json | jq '.docs | length')" = "79"

# Index size
tfidf_size=$(curl -sI https://noahgift.com/search/tfidf.json | awk '/Content-Length/{print $2}')
embed_size=$(curl -sI https://noahgift.com/search/index.apr  | awk '/Content-Length/{print $2}')
[ "$tfidf_size" -le 2097152 ] && [ "$embed_size" -le 26214400 ]

# Smoke: query for a known course
curl -s 'https://noahgift.com/search?q=rust%20simd' \
  | jq -e '.hits[0].id == "advanced-fine-tuning-in-rust"'
```

The third test assumes a `/search` endpoint, but in the WOS model there is no endpoint —
it runs client-side. The CI smoke test uses a Playwright script that injects the same
`grep` call and asserts the top hit.

## 7. Performance budget

| Metric                                  | Target          |
| --------------------------------------- | --------------- |
| First `grep` tf-idf (cold index load)   | ≤ 200 ms        |
| Warm tf-idf query                       | ≤ 15 ms         |
| First `grep` embedding (cold load 22MB) | ≤ 1500 ms       |
| Warm embedding query (79 docs × 384d)   | ≤ 10 ms w/ SIMD |
| Snippet render                          | ≤ 5 ms          |

## 8. Failure modes and fallback

| Failure                            | Behavior                                          |
| ---------------------------------- | ------------------------------------------------- |
| Embedding index fails to load      | Automatic fallback to tf-idf, banner "fallback mode" |
| Query has 0 tokens after normalize | Exit 2 with usage hint                            |
| No results above threshold 0.3     | Exit 1; suggest `--explain` for ranking detail    |
| aprender panics on model load      | Same as "index fails to load" — fallback to tf-idf |

## 9. Open questions

- **Q1.** Which sentence encoder? Options: (a) aprender-native trained from scratch on
  MS-MARCO; (b) MiniLM-L6-v2 via ONNX→aprender converter; (c) bge-small-en-v1.5 similar.
  Lean (b) — well-understood baseline.
- **Q2.** Do we index course titles only, or titles + descriptions + key_concepts?
  Richer index → better recall but larger payload. Lean titles + descriptions + top 5 key_concepts.
- **Q3.** Should `--explain` show the top contributing terms (sparse tf-idf) and top
  contributing dimensions (dense embeddings)? Yes, both — interpretability as a feature.

## 10. References

- Source of truth: `../../../course-studio/config/fixtures.lua`
- aprender: `../../../aprender/`
- trueno: `../../../trueno/`
- Spec 006 (shares embeddings): `./006-recommend-path.md`
- Spec 009 (shares RAG): `./009-ask-transcripts.md`
