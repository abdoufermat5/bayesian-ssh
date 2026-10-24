# bssh AI: workstreams

Design: `bssh-cloud/docs/design/ai-agent-access.md` (v2). Read sections 3-13 before coding.
Integration branch: `feat/ai`. Each workstream works on `feat/ai-wN`, branched from `feat/ai`,
in its own worktree `bssh-wt/ai-wN`. The integrator merges them into `feat/ai`.

## Contract (written first, compiles, do not break)

- `crates/cli/src/services/ai/types.rs`: shared types and traits.
- `crates/cli/src/services/ai/ipc.rs`: control-socket protocol (mcp <-> desktop / CLI).
- The stub signatures in every `services/ai/*.rs` file: other workstreams call them.
- `crates/cli/src/cli/parser_ai.rs`: `bssh ai` arguments, one `AiArgs*` struct per command.

You may **add** to the contract (new fields with `#[serde(default)]`, new functions, new enum
variants). Don't rename or remove anything, and don't change another owner's signatures. If
you must, append a line to `bssh-wt/logs/ai-contract-changes.md` (who, what, why) and keep
the old signature working.

## Ownership

| WS | Scope | Files (under `crates/cli/src/` unless noted) |
|---|---|---|
| W1 | Groundwork: exec timeouts, output caps, non-interactive auth, `AiExecutor` | `services/transport/*`, `cli/commands/exec.rs`, `services/ai/exec.rs` |
| W2 | Command analyzer, redaction, `bssh ai explain`, `bssh ai policy` | `services/ai/analyzer/**`, `services/ai/redact.rs`, `cli/commands/ai/{explain,policy}.rs` |
| W3 | Policy engine, approvals, grants, budgets, runs store, control socket, `approve/stop/status` | `services/ai/{policy,approval,store}.rs`, `cli/commands/ai/{approve,stop,status}.rs` |
| W4 | MCP server, tools, observe parsers, change plans, edit/undo, run orchestration, `bssh ai mcp` | `services/ai/{mcp/**,observe.rs,plan.rs}`, `cli/commands/ai/mcp.rs` |
| W5 | Host enforcement: jail, `setup`, `doctor`, local CA, approval-bound certificates, cert auth in russh | `services/ai/{jail,hostsetup,cert}.rs`, `cli/commands/ai/{setup,doctor,ca}.rs`, a `bssh-jail` helper crate if needed |
| W6 | Local audit chain, recordings of agent output, run reports, `log/replay/install`, guard hooks | `services/ai/{audit,report,install}.rs`, `cli/commands/ai/{log,replay,install}.rs` |
| W7 | Desktop: approvals dialog, live view, Stop, AI settings page | `crates/gui/**` (new `commands/ai.rs`), `desktop/src/**` |
| W8 | Teams agents and gateway: teams-proto, bssh-cloud server, `RelayBroker`, `bssh ai serve/token`, `bssh teams agent` | `crates/teams-proto/**`, `services/ai/{gateway,teams}.rs`, `services/teams/**`, `cli/commands/ai/{serve,token}.rs`, `cli/commands/teams.rs`, bssh-cloud repo |

Shared files you may touch minimally: `crates/cli/Cargo.toml` (add your deps in a block
commented `# bssh AI (WN)`), your own struct in `parser_ai.rs`, `services/ai/mod.rs` (only to
add a submodule you own).

## Rules

- Rust 2021, the repo's style: same comment density, `anyhow` for errors, no `unwrap()` on
  runtime paths, `tracing` for logs. Docs and messages in English, short and plain.
- Every module ships tests. Tests never need the network or a real host: use fakes of the
  `Executor` trait, temp dirs (`tempfile` is a dev-dependency) and fixtures.
- Before each commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test -p bayesian-ssh` (W7 also `cd desktop && npm run check && npm run build`).
- Commits: `git commit -s`, conventional messages (`feat(ai): ...`), **no AI trailers, no
  Co-Authored-By**. Several small commits are fine. Never push, never touch `main` or `feat/ai`.
- Never read or print secrets (keys, tokens, `.env`). Never run commands against real hosts.
- Don't edit `CHANGELOG.md` or the README; the integrator does.
- Finish with a report: what works, what is stubbed and why, contract additions, the exact
  test/clippy output summary.
