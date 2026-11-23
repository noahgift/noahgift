# Sub-spec — Audit snapshot pattern

**Parent:** [../011-runtime-bundle.md](../011-runtime-bundle.md) §6
**Source:**
- `../../../../rmedia/crates/rmedia-wos-kit/src/lib.rs::RegistryBuilder::build`
- `../../../../rmedia/crates/rmedia-wos-audit/src/lib.rs::AuditCommand`

---

## 1. The recursion hazard

Spec 008 defines `audit site` as a rollup command that dispatches every other
command through a held registry reference. The naive wiring looks like:

```rust
// BROKEN: audit holds a reference to the same registry it lives in
let mut reg = Registry::new();
reg.register(AuditCommand::new(&reg));   // ← cannot borrow mutably while borrowing
```

Even if the borrow checker let you, the runtime hazard is live:

```
  audit ─▶ dispatches ▶ audit ─▶ dispatches ▶ audit ─▶ …
```

`audit` sees its own name in the registry it iterates and re-enters. Every
sub-check becomes infinite.

## 2. The snapshot pattern

The kit's `build()` takes an `Arc<Registry>` snapshot **before** registering
audit:

```rust
if self.audit {
    let snapshot = Arc::new(reg.clone());   // clone excludes audit
    reg.register(AuditCommand::new(snapshot));
}
```

- `reg.clone()` at this point has every other command but not audit
- `Arc::new(...)` freezes it behind shared-ownership
- `AuditCommand` holds the `Arc<Registry>` and dispatches against it
- Then audit itself goes into `reg` (the outer, mutable one)

Result:

```
outer Registry { cat, jq, score, prove, ask, grep, …, audit(snapshot) }
                                                           │
                            Arc<Registry> { cat, jq, score, prove, ask, grep, … }  (no audit)
```

Calling `audit` from the outer registry dispatches against the snapshot. The
snapshot has no audit. No recursion.

## 3. Why `Arc`, not `Rc` or `&Registry`

| Candidate         | Why rejected                                     |
|-------------------|--------------------------------------------------|
| `&Registry`       | Lifetime tied to `build()`'s stack frame; cannot live inside registry |
| `Rc<Registry>`    | `WosCommand: Send + Sync`, `Rc` isn't `Send`     |
| `Box<Registry>`   | No shared ownership; audit would own the only copy |
| `Arc<Registry>` ✓ | `Send + Sync`, shared, compatible with the browser's single-threaded WASM runtime and future multi-threaded host use |

The `Send + Sync` bound on `WosCommand` (from spec 010 §4.2) forces `Arc`.

## 4. Snapshot semantics

- **Frozen at build time.** Adding a command to `reg` after `.build()` returns
  does not propagate to the snapshot. The snapshot captures the state at audit
  registration, not at dispatch.
- **Shared, not copied.** Because `Registry` holds `Arc<dyn WosCommand>` values,
  cloning the registry clones the HashMap but not the commands. Total cost:
  one HashMap walk.
- **Immutable through the Arc.** `AuditCommand` never calls any mutating method
  on `Arc<Registry>`. If a future change needed `Arc<Mutex<Registry>>`, we'd
  need to revisit dispatcher re-entrancy.

## 5. What if audit is NOT included?

```rust
let reg = RegistryBuilder::new().stateless().build();  // no .with_audit()
```

No snapshot is taken. `reg` contains cat + jq + score. `audit` is not in the
registry; `rmedia wos audit` exits 1 with "unknown command". This is the
expected behavior for minimal deployments.

## 6. Nested audits

`audit | audit` through the pipe executor dispatches audit twice in sequence:

- **Stage 1** — outer audit runs. It dispatches its sub-checks against the
  snapshot (which has no audit). Emits JSON report to stdout.
- **Stage 2** — second audit runs with the JSON from stage 1 as stdin. It
  ignores stdin and re-runs the rollup. Emits a new JSON report.

Both stages dispatch against the same snapshot. The outer registry never
recurses into its own audit command, because the snapshot doesn't contain audit.

## 7. Audit invoking itself programmatically

`AuditCommand::new(snapshot)` takes `Arc<Registry>` by value. If a caller
hand-constructs an audit with a registry that *includes* audit (bypassing the
builder), recursion returns. The builder is the safety boundary.

Defensive option rejected: an explicit guard inside `AuditCommand` that refuses
to dispatch a command named "audit". Rejected because:

- It conflates name with identity — a differently-named audit wrapper around
  audit would still loop.
- It adds a string compare per dispatch in a hot path.
- The builder-level guarantee is sufficient; hand-constructed registries are
  a caller concern.

## 8. Contract

```json
{
  "id": "AUDIT_SNAPSHOT_NON_RECURSIVE",
  "claim": "AuditCommand dispatches against a pre-audit snapshot (no self-recursion)",
  "value": true,
  "source": "rmedia/crates/rmedia-wos-kit/src/lib.rs::RegistryBuilder::build",
  "falsified_if": {
    "kind": "shell",
    "cmd": "rmedia wos --contracts tests/c.json --fixtures tests/f.json --audit --pipe 'audit --json | jq . | audit --json'",
    "op": "exit",
    "value": 0
  }
}
```

Falsifier: run audit → jq → audit. If the snapshot pattern regresses, the
second audit's internal dispatch loops and the process either stackoverflows
(exit code > 0) or hangs. On green, exit is 0 and the second audit's JSON is
valid.

## 9. Tests

`rmedia-wos-kit/src/lib.rs`:

- `builder_with_audit_does_not_recurse` — `.with_audit()` + dispatch audit
  completes in ≤ 1 s on a registry with every other command present.

`rmedia-wos-kit/tests/cookbook.rs`:

- `audit_quick_rollup` — `--audit audit --json --quick` exits 0 with grade
- `audit_site_fn_matches_command` — programmatic `audit_site(&snapshot)`
  output equals the CLI's JSON

## 10. Performance note

The snapshot's `reg.clone()` is O(N) in registered commands (9-10 entries) and
touches only the HashMap and its Arc-pointed values. Measured cost: ~5 µs on
host, ~15 µs in WASM. Well below the 10 ms `build()` budget.
