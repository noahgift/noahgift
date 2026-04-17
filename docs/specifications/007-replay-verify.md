# Spec 007 — `replay <lesson> --verify`

**Status:** Draft
**Priority:** P0
**Depends on:** 000, 001
**Owner:** Noah Gift

---

## 1. Problem

The CLAUDE.md in course-studio asserts a **SRT-lock protocol**: a given `.srt` file
produces a deterministic `.mp4` render. Same SRT → same video, byte-for-byte (modulo
encoder non-determinism, which course-studio already pins). The site has no way to show
this to a visitor. A reader either takes the claim on faith or `git clone`s the repo and
runs a multi-minute pipeline.

## 2. Non-goals

- Re-rendering full lessons in-browser. The replay is a 20-30 second sample; rendering
  is pre-computed and cryptographically bound.
- Audio-frame-level comparison. Container-level SHA-256 of the mp4 is the contract;
  bitstream drift within constant-bitrate H.264 would be a separate project.
- Full 79-course replay catalog on day one. P0 ships 5-10 representative samples.

## 3. User story

As a visitor skeptical of "rendering is deterministic", I type `replay 1.2.3 --verify`
and see: (a) a 25-second video clip play inline, (b) a pair of SHA-256 hashes — one for
the SRT that drove the render, one for the MP4 that came out, (c) my browser recomputes
both and reports `✓ MATCH` for each, (d) a line citing the CI job that bound them.

## 4. Design

### 4.1. Surface

```
$ replay <lesson-id>                # play the sample inline, no verification
$ replay <lesson-id> --verify       # play + verify hashes against contract
$ replay --list                     # list available samples
$ replay <lesson-id> --hash         # print only the bound hashes
$ replay <lesson-id> --json
```

Lesson ID format: `<course-id>.<module>.<lesson>` (e.g., `c3-etl.1.2.3`).

Example:
```
$ replay c3-etl.1.2.3 --verify
loading /replay/c3-etl.1.2.3.mp4 ................ ok (4.8 MB)
loading /replay/c3-etl.1.2.3.srt ................ ok (1.2 KB)
hashing src srt ............................... sha256: a1b2c3...ef01
hashing out mp4 ............................... sha256: f0e1d2...ba34
contract bound (ci run 2026-04-15T03:22Z):
  srt_hash: a1b2c3...ef01   ✓ MATCH
  mp4_hash: f0e1d2...ba34   ✓ MATCH
  pipeline: rmedia@0.3.121
```

### 4.2. Data contract

**Assets per replayable lesson:**

```
public/replay/<lesson-id>.srt        # the source SRT
public/replay/<lesson-id>.mp4        # 20-30 s rendered sample
public/replay/<lesson-id>.replay.json  # { srt_hash, mp4_hash, rmedia_version, ci_run_id, rendered_at }
```

**Contract entries (per lesson):**

```json
{
  "id": "REPLAY_C3_ETL_1_2_3_HASHES",
  "claim": "c3-etl lesson 1.2.3 sample renders deterministically from its SRT",
  "value": { "srt_hash": "a1b2c3...ef01", "mp4_hash": "f0e1d2...ba34" },
  "source": "course-studio CI job `replay-sample-render`",
  "falsified_if": [
    { "kind": "sha256", "path": "replay/c3-etl.1.2.3.srt", "hash": "a1b2c3...ef01" },
    { "kind": "sha256", "path": "replay/c3-etl.1.2.3.mp4", "hash": "f0e1d2...ba34" }
  ]
}
```

Note: this is the first contract shape that uses an array of falsifiers. Spec 001
evaluator must handle it. (Open question in spec 001 §9 — settle before P0.)

### 4.3. Implementation sketch

**New tool:** `rmedia replay-samples` subcommand (CI-only) that:

1. Iterates a `config/replay.toml` whitelist of lessons.
2. For each, re-renders the first 25 s of the lesson with the pinned rmedia version.
3. Hashes source SRT and output MP4.
4. Emits `<lesson-id>.replay.json`.
5. Uploads the triple (srt, mp4, replay.json) to `s3://replay.noahgift.com/`.

**Replay player (browser):**

```rust
pub async fn replay(lesson_id: &str, verify: bool) -> Result<(), ReplayError> {
    let manifest = fetch_replay_json(lesson_id).await?;
    let mp4_url  = format!("/replay/{lesson_id}.mp4");
    let srt_url  = format!("/replay/{lesson_id}.srt");

    inject_video_tag(&mp4_url)?;

    if verify {
        let mp4_bytes = fetch_bytes(&mp4_url).await?;
        let srt_bytes = fetch_bytes(&srt_url).await?;
        let observed_mp4 = sha256_hex(&mp4_bytes);
        let observed_srt = sha256_hex(&srt_bytes);
        assert_hash_match("srt", &observed_srt, &manifest.srt_hash)?;
        assert_hash_match("mp4", &observed_mp4, &manifest.mp4_hash)?;
    }
    Ok(())
}
```

**CI binding (course-studio):**

```yaml
name: Replay samples
on:
  push:
    branches: [main]
    paths: [config/**/*.lua, crates/rmedia-animation/**]
jobs:
  render:
    runs-on: self-hosted-intel
    steps:
      - uses: actions/checkout@v4
      - name: Install rmedia
        run: cargo install --path crates/rmedia-cli --locked
      - name: Render samples
        run: rmedia replay-samples --whitelist config/replay.toml --out out/replay/
      - name: Upload
        run: aws s3 sync out/replay/ s3://replay.noahgift.com/
```

## 5. Contracts produced

Per-lesson REPLAY_<id>_HASHES contracts (above). Plus a rollup:

```json
{
  "id": "REPLAY_SAMPLE_COUNT",
  "claim": "At least 5 lesson samples available to verify",
  "value": 10,
  "source": "public/replay/index.json",
  "falsified_if": { "kind": "jq_count", "target": ".lessons | length", "op": ">=", "value": 5 }
}
```

```json
{
  "id": "REPLAY_RMEDIA_VERSION_PINNED",
  "claim": "All replay samples rendered with pinned rmedia version 0.3.121",
  "value": "0.3.121",
  "source": "public/replay/index.json",
  "falsified_if": {
    "kind": "jq_count",
    "target": "[.lessons[].rmedia_version] | unique | length",
    "op": "==",
    "value": 1
  }
}
```

## 6. Falsification recipe

```bash
# Sample count
test "$(curl -s https://noahgift.com/replay/index.json | jq '.lessons | length')" -ge 5

# Per-lesson hash verify
for id in $(curl -s https://noahgift.com/replay/index.json | jq -r '.lessons[].id'); do
  manifest=$(curl -s "https://noahgift.com/replay/${id}.replay.json")
  expected_srt=$(echo "$manifest" | jq -r .srt_hash)
  expected_mp4=$(echo "$manifest" | jq -r .mp4_hash)
  observed_srt=$(curl -s "https://noahgift.com/replay/${id}.srt" | sha256sum | awk '{print $1}')
  observed_mp4=$(curl -s "https://noahgift.com/replay/${id}.mp4" | sha256sum | awk '{print $1}')
  [ "$observed_srt" = "$expected_srt" ] || { echo "✗ $id SRT drift"; exit 1; }
  [ "$observed_mp4" = "$expected_mp4" ] || { echo "✗ $id MP4 drift"; exit 1; }
done
echo "✓ all replay samples match"

# Version pin
curl -s https://noahgift.com/replay/index.json \
  | jq -e '[.lessons[].rmedia_version] | unique | length == 1'
```

## 7. Performance budget

| Metric                                 | Target        |
| -------------------------------------- | ------------- |
| Replay MP4 size (25 s, 720p H.264)     | ≤ 6 MB        |
| SRT size                               | ≤ 5 KB        |
| Cold `replay --verify` (fetch + hash)  | ≤ 2 s         |
| Video start-to-play                    | ≤ 500 ms      |
| SHA-256 of 6 MB payload                | ≤ 80 ms       |
| Total replay catalog size (10 lessons) | ≤ 70 MB       |

## 8. Failure modes and fallback

| Failure                               | Behavior                                      |
| ------------------------------------- | --------------------------------------------- |
| Hash mismatch                         | Print both hashes, refuse to claim verified; exit 1 |
| Lesson not in whitelist               | Exit 2 with `replay --list` hint              |
| Video codec unsupported in browser    | Show fallback "download .mp4 to verify locally" |
| rmedia version drift since CI         | Banner in output; contract PINNED_VERSION fails in CI |

## 9. Open questions

- **Q1.** Which 5-10 lessons get picked as the initial replay set? Lean: 1 per
  specialization, biased toward visually interesting animation content.
- **Q2.** Do we keep historical replay samples (v0.3.121, v0.3.122, ...) for drift
  comparison, or always overwrite? Lean: overwrite (storage cost vs. value).
- **Q3.** Hashes of audio track vs video track separately? Combined (container) is
  simpler and matches the "SRT-lock" framing. Lean combined.
- **Q4.** Does the replay UI inject a `<video>` tag or render onto `<canvas>` via WOS's
  virtual filesystem? `<video>` is far simpler; canvas only if we want per-frame
  overlays.

## 10. References

- SRT-lock architecture: `../../../course-studio/CLAUDE.md` §"Deterministic Animation"
- Animation pipeline: `../../../rmedia/crates/rmedia-animation/`
- Spec 001 (evaluator must handle array falsifiers): `./001-prove-claim.md` §Q1
- Spec 008 (`audit site` includes replay checks): `./008-audit-site.md`
