# Spec 006 — `recommend --from "<background>" --to "<goal>"`

**Status:** Draft
**Priority:** P1
**Depends on:** 000, 003
**Owner:** Noah Gift

---

## 1. Problem

A visitor who wants to move from "senior Go backend engineer" to "ML platform engineer"
has no pathway through the 79 courses. The catalog is a flat list. There is no
prerequisite graph, no role-to-skill mapping, and no way for the site to recommend a
sequence. The static pane shows specialization counts; it does not help a person plan.

## 2. Non-goals

- Personal account / progress tracking. Stateless recommendations only.
- Predicting completion probability from demographics. No PII, no profiling.
- Coursera-account integration. All inputs are typed at the command line.
- Fine-grained effort estimation. Surface total hours only, not week-by-week plans.

## 3. User story

As a Go backend engineer who wants to ship an LLM product in six months, I type
`recommend --from "go backend, 5y" --to "llm platform engineer"` and see a ranked 4-step
course sequence (~40 hours), each step annotated with "why" (shared prereqs, role
overlap), with alternatives per step, and the total sequence backed by a contract I can
falsify.

## 4. Design

### 4.1. Surface

```
$ recommend --from "<background>" --to "<goal>"
$ recommend --from "<bg>" --to "<g>" --budget-hours 30
$ recommend --from "<bg>" --to "<g>" --depth deep|broad
$ recommend --from "<bg>" --to "<g>" --explain
$ recommend --from "<bg>" --to "<g>" --json
```

Example output:
```
$ recommend --from "go backend, 5y" --to "llm platform engineer"
sequence (4 courses, ~42 hrs total):
  1. python-bash-sql-data-engineering         (Duke)   8 hrs
       → bridges: Go → Python idioms, scripting fluency
  2. mlops-machine-learning-duke              (Duke)  12 hrs
       → foundation for "platform engineer" role vocabulary
  3. llmops-specialization/operationalizing-llms (PAL)  10 hrs
       → ↑ direct match on "llm" + "platform"
  4. llmops-specialization/llm-fine-tuning    (PAL)   12 hrs
       → consolidates the goal
alternatives per step printed with --explain.
```

### 4.2. Data contract

**Input sources:**

- `fixtures.json` — catalog + metadata
- `public/recommend/graph.json` — prerequisite DAG over courses
- `public/recommend/roles.json` — role → skill vectors
- `public/search/index.apr` (reused from spec 003) — course embeddings

**Graph schema:**

```json
{
  "nodes": [
    { "id": "mlops-machine-learning-duke", "hours": 12, "level": "intermediate",
      "skills": ["mlops", "python", "deployment"], "spec_slug": "mlops-duke" }
  ],
  "edges": [
    { "from": "python-bash-sql-data-engineering", "to": "mlops-machine-learning-duke",
      "relation": "recommends", "strength": 0.8 }
  ]
}
```

**Role schema:**

```json
{
  "llm platform engineer": {
    "skills": { "llmops": 1.0, "python": 0.8, "containers": 0.7, "inference": 0.9 }
  },
  "data scientist": { ... }
}
```

Role list is closed (~25 roles) in v0.1. Visitors who type free-form backgrounds fall
back to embedding similarity (spec 003's index).

### 4.3. Implementation sketch

**New crate:** `../../../rmedia/crates/rmedia-wos-recommend/`

**Algorithm:**

```rust
pub fn recommend(from: &str, to: &str, budget: Option<u32>) -> Sequence {
    // 1. Resolve from/to to role vectors.
    let from_vec = resolve_role(from)?;
    let to_vec   = resolve_role(to)?;

    // 2. Find courses whose skill vectors best span the gap.
    let gap = to_vec.sub(&from_vec);            // dense skill-delta
    let scored = GRAPH.nodes.iter()
        .map(|n| (n, cosine(&n.skills_vec, &gap)))
        .collect::<Vec<_>>();

    // 3. Greedy-cover the gap using top-scoring courses, respecting prereq edges.
    let mut picked = Vec::new();
    let mut remaining = gap.clone();
    while !remaining.below_threshold(0.1) && total_hours(&picked) < budget.unwrap_or(60) {
        let next = pick_next(&scored, &picked)?;   // respects prereqs
        remaining = remaining.sub(&next.skills_vec.weighted(1.0));
        picked.push(next);
    }

    // 4. Topological sort within the prereq DAG.
    toposort(picked)
}
```

**Explainability:** every selected course emits an `{ overlap_score, gap_closed_delta,
prereq_satisfied_by }` trace. The `--explain` flag prints these.

**Graph authoring:** the prereq DAG is hand-curated in a TOML file (`config/recommend.toml`
in course-studio), projected to JSON at CI time. Human-in-the-loop; automation would be
ML-sourced and noisy.

## 5. Contracts produced

```json
{
  "id": "RECOMMEND_GRAPH_NODE_COUNT",
  "claim": "Recommendation graph covers all 79 courses",
  "value": 79,
  "source": "public/recommend/graph.json",
  "falsified_if": { "kind": "jq_count", "target": ".nodes | length", "op": "==", "value": 79 }
}
```

```json
{
  "id": "RECOMMEND_GRAPH_IS_DAG",
  "claim": "Prerequisite graph contains no cycles",
  "value": true,
  "source": "CI job `recommend-graph-check`",
  "falsified_if": { "kind": "sha256", "path": "recommend/dag-proof.log", "hash": "<bound>" }
}
```

```json
{
  "id": "RECOMMEND_ROLE_COUNT",
  "claim": "Recommender supports ≥ 20 target roles",
  "value": 25,
  "source": "public/recommend/roles.json",
  "falsified_if": { "kind": "jq_count", "target": "keys | length", "op": ">=", "value": 20 }
}
```

## 6. Falsification recipe

```bash
# Node count matches catalog size
test "$(curl -s https://noahgift.com/recommend/graph.json | jq '.nodes | length')" = "79"

# DAG check — toposort must succeed
curl -s https://noahgift.com/recommend/graph.json \
  | python3 -c 'import json,sys; from graphlib import TopologicalSorter, CycleError;
    g=json.load(sys.stdin); ts=TopologicalSorter({n["id"]:[] for n in g["nodes"]});
    [ts.add(e["to"],e["from"]) for e in g["edges"]];
    try: ts.prepare(); print("DAG ✓")
    except CycleError as e: print("CYCLE ✗", e); sys.exit(1)'

# Role coverage
test "$(curl -s https://noahgift.com/recommend/roles.json | jq 'keys | length')" -ge 20
```

## 7. Performance budget

| Metric                                  | Target       |
| --------------------------------------- | ------------ |
| Graph JSON payload (gzipped)            | ≤ 40 KB      |
| Role vectors payload (gzipped)          | ≤ 8 KB       |
| Cold `recommend` (load + run)           | ≤ 400 ms     |
| Warm `recommend` (typical 4-step path)  | ≤ 30 ms      |
| Worst-case exhaustive search (budget 60 hrs) | ≤ 150 ms |

## 8. Failure modes and fallback

| Failure                           | Behavior                                              |
| --------------------------------- | ----------------------------------------------------- |
| Unknown role in `--to`            | Embedding-similarity match against role list; warn    |
| Gap too small (from ≈ to)         | Exit 0 with "already there — suggest refresher" list  |
| Gap too large (no path)           | Exit 1 with "gap spans >80 hrs — consider staged goal"|
| `recommend.toml` has cycle (CI)   | Build fails, no deploy                                |
| Graph fetch fails                 | Exit 3; suggest `diff fixtures`                       |

## 9. Open questions

- **Q1.** Who curates `config/recommend.toml`? Noah in v0.1. Consider automated
  extraction from course key_concepts later.
- **Q2.** How do we handle role synonyms? ("backend dev" ≈ "backend engineer" ≈ "server
  engineer"). Normalize via a small alias table + embedding fallback.
- **Q3.** Do we publish the resulting sequence as its own contract (pinned for a given
  `from+to` pair)? That would let a visitor verify "this is the recommendation the site
  actually gives, not a cherry-picked output." Lean yes, via a hash of
  `(from, to, sequence)` tuples tested in CI.

## 10. References

- Catalog: `../../../course-studio/config/fixtures.lua`
- Shares embeddings with: `./003-semantic-grep.md`
- Prereq graph TOML (to be authored): `../../../course-studio/config/recommend.toml`
- Master spec principles: `./README.md` §"Design principles"
