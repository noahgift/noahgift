# Sub-spec — `rmedia wos` CLI subcommand

**Parent:** [../011-runtime-bundle.md](../011-runtime-bundle.md) §7
**Source:**
- `../../../../rmedia/crates/rmedia-cli/src/wos_cmd.rs`
- `../../../../rmedia/crates/rmedia-cli/tests/wos_cli_t.rs`

---

## 1. Why a host CLI

The WOS kernel runs in-browser. A contributor writing or regression-testing a
WOS command needs to drive it from a shell. Before this spec, the options were:

- Write a throwaway Rust `main()` that depends on every `rmedia-wos-*` crate
- Run the full site locally and exercise it via Playwright

Both are slow feedback loops. `rmedia wos` gives shell parity: every pipeline
that works in-browser works identically in bash, and CI can smoke the full
matrix in under 5 seconds.

## 2. Surface

```
rmedia wos --list                                       # print registered command names
rmedia wos --pipe "cmd1 --arg | cmd2 | cmd3"            # pipeline (spec 010)
rmedia wos <cmd> [args...]                              # single command
```

Stdin is read once and passed to the first stage (single command or pipeline
head). Stdout of the final stage is written to the process's stdout. The last
stage's stderr is written to the process's stderr. Exit codes propagate
(see §5).

## 3. clap definition

```rust
#[derive(Args)]
pub struct WosArgs {
    #[arg(long, conflicts_with_all = ["pipe", "cmd"])]
    pub list: bool,

    #[arg(long, conflicts_with = "cmd")]
    pub pipe: Option<String>,

    #[arg(long, env = "RMEDIA_WOS_CONTRACTS")]
    pub contracts: Option<PathBuf>,

    #[arg(long, env = "RMEDIA_WOS_FIXTURES")]
    pub fixtures: Option<PathBuf>,

    #[arg(long, env = "RMEDIA_WOS_ASK_CHUNKS")]
    pub ask_chunks: Option<PathBuf>,

    #[arg(long, env = "RMEDIA_WOS_GREP_DOCS")]
    pub grep_docs: Option<PathBuf>,

    #[arg(long, env = "RMEDIA_WOS_GRAPH")]
    pub graph: Option<PathBuf>,

    #[arg(long, env = "RMEDIA_WOS_ROLES")]
    pub roles: Option<PathBuf>,

    #[arg(long, env = "RMEDIA_WOS_REPLAY_MANIFEST")]
    pub replay_manifest: Option<PathBuf>,

    #[arg(long, env = "RMEDIA_WOS_REPLAY_ASSETS_DIR")]
    pub replay_assets_dir: Option<PathBuf>,

    #[arg(long)]
    pub fake_transcribe: bool,

    #[arg(long)]
    pub audit: bool,

    #[arg(trailing_var_arg = true)]
    pub cmd: Vec<String>,
}
```

`conflicts_with_all` makes `--list`, `--pipe`, and the positional command
mutually exclusive. Every data flag has a matching `RMEDIA_WOS_*` env var so
CI can wire once via `env:` and forget.

## 4. Flag pairing

Four flag pairs must be supplied together:

| Pair                                  | Enables          | Mismatch behavior                            |
|---------------------------------------|------------------|----------------------------------------------|
| `--contracts` + `--fixtures`          | `prove`          | Exit 2 with "must be supplied together"      |
| `--graph` + `--roles`                 | `recommend`      | Exit 2 with "must be supplied together"      |
| `--replay-manifest` + `--replay-assets-dir` | `replay`   | Exit 2 with "must be supplied together"      |
| (`--contracts` OR `--fixtures`) + `--audit` | `audit`    | `--audit` alone registers audit but rollup returns degenerate (no sub-commands) |

`--fake-transcribe` has no pair; it registers `transcribe` with a deterministic
backend used for tests and spec-005 contract validation.

## 5. Exit code matrix

| Condition                              | Exit |
|----------------------------------------|------|
| Command succeeds                       | 0    |
| Pipeline succeeds (all stages 0)       | 0    |
| Unknown command                        | 1    |
| Underlying command non-zero            | command's own (1–255) |
| Missing paired flag                    | 2    |
| Data file unreadable                   | 1    |
| Data file parse error                  | 1    |
| Pipe parse error (shlex)               | 2    |
| Pipe buffer overflow (> 4 MB)          | 3    |
| Stdin read error                       | 1    |

Exit codes match spec 010 §8's table where pipelines are involved.

## 6. Registry construction

```rust
fn build_registry(args: &WosArgs) -> Result<Registry> {
    let mut b = RegistryBuilder::new().stateless();

    // prove
    if let (Some(c), Some(f)) = (&args.contracts, &args.fixtures) {
        let (contracts, json, bytes) = load_prove(c, f)?;
        b = b.with_prove(contracts, json, bytes);
    } else if args.contracts.is_some() ^ args.fixtures.is_some() {
        return Err(RmediaError::Other(
            "wos: --contracts and --fixtures must be supplied together".into()
        ));
    }

    if let Some(p) = &args.ask_chunks  { b = b.with_ask(load_ask_index(p)?); }
    if let Some(p) = &args.grep_docs   { b = b.with_grep(load_grep_index(p)?); }

    // recommend
    match (&args.graph, &args.roles) {
        (Some(g), Some(r)) => { b = b.with_recommend(load_graph(g)?, load_roles(r)?); }
        (Some(_), None) | (None, Some(_)) => {
            return Err(RmediaError::Other(
                "wos: --graph and --roles must be supplied together".into()
            ));
        }
        (None, None) => {}
    }

    // replay
    if let Some(m) = &args.replay_manifest {
        let dir = args.replay_assets_dir.as_ref().ok_or_else(|| RmediaError::Other(
            "wos: --replay-manifest requires --replay-assets-dir".into()
        ))?;
        b = b.with_replay(load_replay(m, dir)?);
    }

    if args.fake_transcribe { b = b.with_transcribe(Arc::new(DeterministicFakeBackend { model: "whisper-tiny-fake".into() })); }
    if args.audit           { b = b.with_audit(); }

    Ok(b.build())
}
```

Load helpers return `Result<_, RmediaError>`; IO errors wrap the path in the
message for immediate user feedback.

## 7. Dispatch

```rust
pub fn run(args: WosArgs) -> Result<()> {
    let registry = build_registry(&args)?;

    if args.list {
        for name in registry.names() { println!("{name}"); }
        return Ok(());
    }

    let mut stdin = Vec::new();
    std::io::stdin().read_to_end(&mut stdin)?;

    if let Some(pipe) = args.pipe {
        let result = exec_pipeline_with_stdin(&pipe, &stdin, &registry)?;
        std::io::stdout().write_all(&result.stdout)?;
        if let Some((_, last)) = result.stages.last() {
            if !last.stderr.is_empty() { std::io::stderr().write_all(&last.stderr)?; }
        }
        if !result.passed() { std::process::exit(result.exit() as i32); }
        return Ok(());
    }

    if args.cmd.is_empty() {
        return Err(RmediaError::Other("wos: usage: rmedia wos <cmd> [args...] | --pipe <pipeline> | --list".into()));
    }

    let name = &args.cmd[0];
    let cmd = registry.get(name).ok_or_else(||
        RmediaError::Other(format!("wos: unknown command: {name}. try `rmedia wos --list`"))
    )?;
    let out = cmd.run(&stdin, &args.cmd[1..].to_vec());
    std::io::stdout().write_all(&out.stdout)?;
    if !out.stderr.is_empty() { std::io::stderr().write_all(&out.stderr)?; }
    if out.exit != 0 { std::process::exit(out.exit as i32); }
    Ok(())
}
```

Three code paths:

1. `--list` — print names and exit 0.
2. `--pipe <str>` — parse, exec, propagate last-stage exit and stderr.
3. single command — look up, run, propagate exit and stderr.

## 8. Fixture resolution in tests

Integration tests (`tests/wos_cli_t.rs`) use `assert_cmd::Command::cargo_bin`
and resolve fixtures via `env!("CARGO_MANIFEST_DIR")`:

```rust
fn fixture(name: &str) -> String {
    format!(
        "{}/../rmedia-wos-prove/tests/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"), name
    )
}
```

This works because `CARGO_MANIFEST_DIR` points at `crates/rmedia-cli/` at test
compile time, and the `rmedia-wos-prove` crate sits beside it. The CLI binary
under test has no manifest-dir concept at runtime — the test passes resolved
absolute paths via `--contracts` and `--fixtures`.

## 9. Test coverage

`tests/wos_cli_t.rs` — 9 `assert_cmd` tests:

1. `wos_list_shows_stateless_trio` — cat/jq/score always present
2. `wos_jq_filters_stdin_json` — single-cmd dispatch with trailing args
3. `wos_pipe_chains_cat_into_jq` — `--pipe` path
4. `wos_unknown_command_errors_out` — exit 1 + stderr message
5. `wos_fake_transcribe_registers_transcribe` — flag-driven registration
6. `wos_contracts_without_fixtures_errors` — paired-flag validation
7. `wos_prove_all_with_fixtures_passes` — real contracts happy path
8. `wos_audit_rolls_up_contracts_and_emits_grade` — `--audit audit --quick`
9. `wos_pipeline_audit_piped_through_jq` — audit | jq end-to-end

Runs under 4 seconds on a cold target dir.

## 10. Parity with in-browser WOS

A pipeline string produces byte-identical stdout whether dispatched by:

- `rmedia wos --pipe "<string>"` (host)
- The WOS terminal in the browser (WASM)

This is enforced by the shared `exec_pipeline_with_stdin` implementation in
`rmedia-wos` — no host-specific codepaths. The only divergence is startup
latency (host: ~50 ms binary launch; WASM: one-time ~500 ms download, then
cached).

## 11. Non-goals

- **Not a replacement for the WOS browser UI.** The CLI has no history, no
  tab-completion, no ANSI coloring beyond what commands emit. It exists for
  automation.
- **No command discovery from disk.** Only commands registered via the kit's
  builder are available. No plugin loader.
- **No shell integration.** No bash completion file, no zsh function. clap's
  `--help` is the discovery surface.
