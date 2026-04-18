# Sub-spec — `RegistryBuilder` API

**Parent:** [../011-runtime-bundle.md](../011-runtime-bundle.md) §5
**Source:** `../../../../rmedia/crates/rmedia-wos-kit/src/lib.rs`

---

## 1. Why a builder, not a factory

Each command takes a distinct construction payload:

- `prove` needs `Contracts + serde_json::Value + Vec<u8>` (three types)
- `ask` needs an `AskIndex` (built from `Vec<Chunk>`)
- `grep` needs a `GrepIndex` (built from `Vec<DocSource>`)
- `recommend` needs `(Graph, Roles)` — two structs
- `transcribe` needs `Arc<dyn WhisperBackend>` (trait object)
- `audit` needs a snapshotted `Arc<Registry>` (self-referential, see §6)

A single `with_defaults()` factory would need 10+ positional arguments or a
giant config struct. A builder is the right shape: per-method, typed, additive.

## 2. Struct

```rust
#[derive(Default)]
pub struct RegistryBuilder {
    stateless:   bool,
    ask:         Option<AskIndex>,
    grep:        Option<GrepIndex>,
    recommend:   Option<(Graph, Roles)>,
    transcribe:  Option<Arc<dyn WhisperBackend>>,
    prove:       Option<(Contracts, serde_json::Value, Vec<u8>)>,
    replay:      Option<ReplayCommand>,
    audit:       bool,
}
```

Every field is optional. `stateless` and `audit` are `bool` toggles; the rest
are payloads. `Default` derived so `RegistryBuilder::new()` is free.

## 3. Methods

```rust
impl RegistryBuilder {
    pub fn new() -> Self { Self::default() }

    /// Register cat, jq, score. Always safe to call.
    pub fn stateless(mut self) -> Self { self.stateless = true; self }

    pub fn with_prove(
        mut self,
        contracts: Contracts,
        fixtures_json: serde_json::Value,
        fixtures_bytes: Vec<u8>,
    ) -> Self {
        self.prove = Some((contracts, fixtures_json, fixtures_bytes));
        self
    }

    pub fn with_ask(mut self, index: AskIndex) -> Self            { self.ask = Some(index); self }
    pub fn with_grep(mut self, index: GrepIndex) -> Self          { self.grep = Some(index); self }
    pub fn with_recommend(mut self, g: Graph, r: Roles) -> Self   { self.recommend = Some((g, r)); self }
    pub fn with_transcribe(mut self, b: Arc<dyn WhisperBackend>) -> Self { self.transcribe = Some(b); self }
    pub fn with_replay(mut self, cmd: ReplayCommand) -> Self      { self.replay = Some(cmd); self }
    pub fn with_audit(mut self) -> Self                           { self.audit = true; self }

    pub fn build(self) -> Registry { /* see §4 */ }
}
```

Methods take `self` by value (not `&mut self`) so the chain reads naturally:
`RegistryBuilder::new().stateless().with_prove(...).build()`.

## 4. `build()` semantics

```rust
pub fn build(self) -> Registry {
    let mut reg = Registry::new();

    if self.stateless { reg.register_stateless(); }

    if let Some((c, f, b)) = self.prove      { reg.register(ProveCommand::new(c, f, b)); }
    if let Some(idx)       = self.ask        { reg.register(AskCommand::new(idx)); }
    if let Some(idx)       = self.grep       { reg.register(GrepCommand::new(idx)); }
    if let Some((g, r))    = self.recommend  { reg.register(RecommendCommand::new(g, r)); }
    if let Some(b)         = self.transcribe { reg.register(TranscribeCommand::new(b)); }
    if let Some(cmd)       = self.replay     { reg.register(cmd); }

    if self.audit {
        let snapshot = Arc::new(reg.clone());
        reg.register(AuditCommand::new(snapshot));
    }

    reg
}
```

Ordering invariants:

1. **Stateless first.** cat/jq/score seed the registry before any data-bearing
   command. This matters for pipelines that use `jq` between stages.
2. **Audit last.** The snapshot must be taken after every other command is
   registered so audit rolls up a complete picture.
3. **No duplicates.** `Registry::register` replaces by name. Calling `with_ask`
   twice yields the last value; the kit does not warn.

## 5. Convenience fn

```rust
pub fn stateless_registry() -> Registry {
    RegistryBuilder::new().stateless().build()
}
```

Used by the browser cookbook panel to boot a minimal registry with no data
files. Saves the WASM loader one builder invocation.

## 6. Infallibility

`build()` returns `Registry`, not `Result<Registry, _>`. Reasons:

- Every input is already constructed (indexes built, backends boxed). Failure
  can only happen at load time, which the caller handles.
- `Registry` construction is pure HashMap insertion — no allocation can fail in
  a way we propagate.
- The builder does not touch I/O.

If a caller needs fallibility (e.g., loading JSON from disk), it goes in the
caller — see `rmedia-cli/src/wos_cmd.rs::load_prove` for the pattern.

## 7. Clone cost

`Registry` is `Clone` because `AuditCommand::new` needs an `Arc<Registry>` that
captures the pre-audit state. `Registry::clone()` is shallow: it clones the
`HashMap<String, Arc<dyn WosCommand>>` but all the `Arc` command values are
reference-counted, not deep-copied. Empirical cost: ~5 µs for a full registry.

## 8. Tests

`src/lib.rs::tests`:

- `builder_empty_yields_empty_registry` — `Registry::names()` is empty
- `builder_stateless_registers_trio` — cat, jq, score present
- `builder_with_transcribe_registers_command` — name "transcribe" present
- `builder_with_audit_does_not_recurse` — audit → cat passes, not stackoverflow
- `builder_chained_composition` — full chain → all 10 commands
- `build_is_deterministic` — two builds of same inputs yield equal `names()` set
- `stateless_registry_helper` — `stateless_registry()` matches manual builder

## 9. Non-goals

- **Configuration validation.** The builder does not check that `with_audit()`
  was preceded by any data-bearing `with_*`. An audit-only registry is valid
  (it audits a stateless-only site, returning a degenerate report).
- **Async construction.** No `build_async`. Callers that load from disk wrap
  the builder in their own async shell (see CLI sub-spec).
- **Extensibility.** Third-party commands cannot be added via the builder.
  They must use `Registry::register` directly after `.build()`.
