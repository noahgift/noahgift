# Spec 005 — `transcribe <clip>`

**Status:** Draft
**Priority:** P2 (gated on aprender+whisper WASM readiness)
**Depends on:** 000
**Owner:** Noah Gift

---

## 1. Problem

noahgift.com's animation pipeline is SRT-first — every rendered course video derives its
timing from a whisper-produced transcript. The site mentions this, but a visitor has no
way to *see* it. The instructor's claim "animation is deterministic because whisper
runs once in prepare, never at render" is provable only if a visitor can run whisper
too.

## 2. Non-goals

- Real-time streaming transcription. Single-shot audio → SRT only.
- Languages other than English in v0.1. (aprender's whisper.apr ships English models.)
- Audio files > 30 seconds. The P2 tier keeps the WASM download and CPU budget tight.
- Diarization, word-level confidence, punctuation correction.

## 3. User story

As a curious visitor on the noahgift.com homepage, I click "Try the pipeline: upload 10
seconds of audio", drop a `.wav` file, run `transcribe mic.wav`, and within 4 seconds
see the SRT output with line-by-line timestamps and word-level token bars — rendered
client-side by the same whisper binary that generated the course SRTs.

## 4. Design

### 4.1. Surface

```
$ transcribe <path>                 # path to a file in WOS's virtual FS
$ transcribe --model tiny           # default; also: base, small (gated on flag)
$ transcribe --hotwords "rust,simd,aprender"   # logit bias matching CLAUDE.md pattern
$ transcribe --detail               # show per-segment confidence bars
$ transcribe --json                 # structured output
```

WOS exposes a file upload affordance tied to virtual FS paths:

```
$ upload /clip.wav                  # opens file picker; writes into WOS virtual FS
$ transcribe /clip.wav
```

### 4.2. Data contract

**Input:** 16 kHz PCM-encoded audio, mono, ≤ 30 seconds. Supported source formats:
`.wav`, `.mp3`, `.webm` (decoded via WebAudio API before handing to whisper).

**Output (SRT, default):**

```
1
00:00:00,000 --> 00:00:03,120
Welcome to the SRT-first animation pipeline.

2
00:00:03,120 --> 00:00:06,880
Every frame transition snaps to whisper's word timing.
```

**Output (`--json`):**

```json
{
  "segments": [
    { "start_ms": 0,    "end_ms": 3120,  "text": "Welcome to the SRT-first animation pipeline.",
      "words": [ { "t": "Welcome", "start_ms": 20, "end_ms": 460, "confidence": 0.97 }, ... ] },
    { "start_ms": 3120, "end_ms": 6880,  "text": "Every frame transition snaps to whisper's word timing.",
      "words": [ ... ] }
  ],
  "model": "whisper-tiny",
  "duration_ms": 2840,
  "source_sha256": "a3c8…b212"
}
```

### 4.3. Implementation sketch

**New crate:** `../../../rmedia/crates/rmedia-wos-transcribe/` (thin shim over
`aprender::models::whisper`).

**Dependencies:**
- `aprender` with `wasm` + `whisper` features
- `trueno` for SIMD GEMM in the decoder
- `web-sys` for `AudioContext` (decoding source audio to 16 kHz PCM)

**Model shipping:**
- `public/models/whisper-tiny.apr` — ~39 MB quantized int8
- `public/models/whisper-base.apr` — ~74 MB (gated, loaded on `--model base`)

**Flow:**

```rust
#[wasm_bindgen]
pub struct WasmWhisper {
    model: aprender::models::WhisperModel,
}

#[wasm_bindgen]
impl WasmWhisper {
    pub async fn load(model_url: &str) -> Result<WasmWhisper, JsValue> {
        let bytes = fetch_bytes(model_url).await?;
        let model = aprender::models::WhisperModel::from_apr_bytes(&bytes)?;
        Ok(Self { model })
    }

    pub fn transcribe(&self, pcm_16k: &[f32], hotwords: Vec<String>) -> JsValue {
        let result = self.model.transcribe_pcm(pcm_16k, TranscribeConfig {
            hotwords,
            language: Some("en"),
            temperature: 0.0,   // determinism contract
        });
        serde_wasm_bindgen::to_value(&result).unwrap()
    }
}
```

**Audio decode path:**

```rust
fn decode_to_16k_pcm(file: web_sys::File) -> Promise {
    // 1. AudioContext::decodeAudioData(file.arrayBuffer())
    // 2. Resample to 16 kHz if not already (linear interpolation is fine at this scale)
    // 3. Return Float32Array (length = duration_ms × 16)
}
```

### 4.4. Determinism contract

`transcribe` ships with `temperature=0.0` and seeded sampling, so the same audio input
produces the same SRT output on every run. A determinism contract binds this:

```json
{
  "id": "TRANSCRIBE_DETERMINISM",
  "claim": "Same audio input produces bit-identical SRT output",
  "value": true,
  "source": "rmedia-wos-transcribe/tests/determinism.rs",
  "falsified_if": { "kind": "sha256", "path": "transcribe/gold-clip.srt.sha", "hash": "<bound>" }
}
```

## 5. Contracts produced

Already above (TRANSCRIBE_DETERMINISM), plus:

```json
{
  "id": "TRANSCRIBE_MODEL_SIZE_MB",
  "claim": "whisper-tiny.apr is ≤ 40 MB",
  "value": 39.1,
  "source": "HEAD /models/whisper-tiny.apr",
  "falsified_if": { "kind": "jq_count", "target": ".size_mb", "op": "<=", "value": 40 }
}
```

```json
{
  "id": "TRANSCRIBE_LATENCY_P95_MS_PER_SEC_AUDIO",
  "claim": "whisper-tiny WASM runs at ≤ 400 ms per second of audio on a mid-range laptop",
  "value": 320,
  "source": "tests/perf/transcribe-benchmark.log",
  "falsified_if": { "kind": "jq_count", "target": ".p95_ms_per_sec", "op": "<=", "value": 400 }
}
```

## 6. Falsification recipe

```bash
# Determinism: run twice on same input, compare bit-exact
node tests/perf/transcribe.js gold-clip.wav > /tmp/a.srt
node tests/perf/transcribe.js gold-clip.wav > /tmp/b.srt
diff -q /tmp/a.srt /tmp/b.srt   # empty output = identical

# Model size
test "$(curl -sI https://noahgift.com/models/whisper-tiny.apr \
  | awk '/Content-Length/{print $2}')" -le 41943040

# Latency: 10 seconds of audio should transcribe in ≤ 4 seconds
node tests/perf/transcribe-benchmark.js 10s-clip.wav --model tiny \
  | jq -e '.duration_ms <= 4000'
```

## 7. Performance budget

| Metric                                       | Target         |
| -------------------------------------------- | -------------- |
| whisper-tiny WASM + model download (gzipped) | ≤ 42 MB        |
| Cold load (first `transcribe` invocation)    | ≤ 3 s          |
| Warm transcription rate (tiny, SIMD)         | ≤ 0.4× realtime|
| Warm transcription rate (base, WebGPU)       | ≤ 0.2× realtime|
| Memory high-water                            | ≤ 512 MB       |

## 8. Failure modes and fallback

| Failure                               | Behavior                                         |
| ------------------------------------- | ------------------------------------------------ |
| SIMD not supported                    | Refuse; print "browser lacks SIMD" to stderr     |
| Audio > 30 s                          | Truncate to first 30 s; emit warning             |
| Model download interrupted            | Resume if supported; otherwise clear cache + retry |
| Non-English audio                     | Transcribe anyway (whisper is multilingual), prefix warning |
| WebAudio decode failure               | Exit 2 with the browser's error message          |

## 9. Open questions

- **Q1.** Is WebGPU mandatory for `--model base`? With WebGPU gone from Safari's stable
  channel as of 2026-04, the answer is no; base stays WASM-only.
- **Q2.** Ship hotwords derived from the site's own vocabulary (from course key_concepts)
  as a default? Yes — a dropdown "pre-load course vocabulary" makes demos shine.
- **Q3.** Do we want a `--diff` flag that compares against an uploaded SRT? Deferred to
  v0.2.

## 10. References

- aprender + whisper: `../../../aprender/crates/whisper.apr/`
- SRT-first architecture: `../../../course-studio/CLAUDE.md` §"Deterministic Animation"
- Related spec (RAG over SRTs): `./009-ask-transcripts.md`
