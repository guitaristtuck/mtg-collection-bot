# fb3k_discord_bot

Discord bot (Rust, serenity 0.12) for a friend group that plays Magic: The Gathering. Searches friends' Archidekt/Moxfield collections and does a few fun interactions.

## Rules
- **Never push to `main`.** Every push to `main` builds and deploys straight to the production server (`.github/workflows/main_deploy.yaml`). Work on a branch and open a PR.
- Don't start the bot (`cargo run`) unless asked — it connects to Discord with real credentials. The user tests manually in the **FB3K Bot Playground** server (guild id `1223050445128466553`).
- Never print or commit `.env` contents.

## Commands
CI (`pr_build.yaml`) runs these on every PR; run them before calling work done:
```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```
Rust version is pinned in `rust-toolchain.toml`; keep the workflow `dtolnay/rust-toolchain@<ver>` tags in sync with it.

## Layout
The crate is a library (`src/lib.rs`, declares all modules) plus a thin binary.
- `src/main.rs` — startup only: log4rs, dotenv, token, config, client build. Gateway intents: `GUILDS`, `GUILD_VOICE_STATES`.
- `src/handler.rs` — `Handler` (serenity `EventHandler`): registers guild slash commands in `ready`, dispatches by name in `interaction_create`, forwards voice events.
- `src/commands/` — one file per slash command, each exposing `register() -> CreateCommand` and `run(...) -> EditInteractionResponse`. `ping.rs` is the minimal example; `mtg.rs` shows subcommands and async.
- `src/interactions/` — non-command event handlers (`nubby.rs`: voice-join announcement).
- `src/mtg/` — collection search, community decks, provider HTTP clients (`providers/archidekt.rs`, `providers/moxfield.rs`; Moxfield is rate-limited to 1 req/s).
- `src/models/config.rs` — `BotConfig`, loaded from `config/*.yaml`. Runtime state lives here too (e.g. a `Mutex` in `NubbyConfig`).

### Adding a slash command
1. Create `src/commands/<name>.rs` with `register()` and `run()`; add `pub mod <name>;` to `src/commands/mod.rs`.
2. Add `commands::<name>::register()` to the `set_commands` vec in `ready` and a match arm in `interaction_create` (`src/handler.rs`).

## Runtime
- Env (via `.env`, gitignored): `DISCORD_TOKEN`, `GUILD_ID`, `MOXFIELD_USER_AGENT`.
- Reads `config/` and `log4rs.yaml` relative to the working directory, so run from the repo root.
- No persistence and no background scheduler exist yet; anything stateful is in-memory and lost on restart.

## Testing
- Tests are offline and deterministic. Never hit the real Archidekt or Moxfield APIs (or Discord); use `wiremock` for HTTP.
- Put logic in plain functions that don't need a Discord `Context`, so it can be tested directly.
- Assert serenity builders (`EditInteractionResponse`, `CreateEmbed`, ...) by serializing them with `serde_json::to_value` and checking fields (see `tests/ping.rs`).
- Use paused tokio time for anything rate-limited or time-based: `#[tokio::test(start_paused = true)]`. Don't sleep for real.
- Unit tests go in an in-module `#[cfg(test)] mod tests`; integration tests go in `tests/` and use the library crate (`fb3k_discord_bot::...`).
- PR CI prints a `cargo llvm-cov` coverage summary (report-only). The deploy workflow runs `cargo test` and won't deploy if it fails.

## Agent skills

### Issue tracker

Issues live in GitHub Issues on `guitaristtuck/mtg-collection-bot` (via `gh`). See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
