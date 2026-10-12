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
- `src/main.rs` — `Handler` (serenity `EventHandler`): registers guild slash commands in `ready`, dispatches by name in `interaction_create`, forwards voice events. Gateway intents: `GUILDS`, `GUILD_VOICE_STATES`.
- `src/commands/` — one file per slash command, each exposing `register() -> CreateCommand` and `run(...) -> EditInteractionResponse`. `ping.rs` is the minimal example; `mtg.rs` shows subcommands and async.
- `src/interactions/` — non-command event handlers (`nubby.rs`: voice-join announcement).
- `src/mtg/` — collection search, community decks, provider HTTP clients (`providers/archidekt.rs`, `providers/moxfield.rs`; Moxfield is rate-limited to 1 req/s).
- `src/models/config.rs` — `BotConfig`, loaded from `config/*.yaml`. Runtime state lives here too (e.g. a `Mutex` in `NubbyConfig`).

### Adding a slash command
1. Create `src/commands/<name>.rs` with `register()` and `run()`; add `pub mod <name>;` to `src/commands/mod.rs`.
2. Add `commands::<name>::register()` to the `set_commands` vec in `ready` and a match arm in `interaction_create` (`src/main.rs`).

## Runtime
- Env (via `.env`, gitignored): `DISCORD_TOKEN`, `GUILD_ID`, `MOXFIELD_USER_AGENT`.
- Reads `config/` and `log4rs.yaml` relative to the working directory, so run from the repo root.
- No persistence and no background scheduler exist yet; anything stateful is in-memory and lost on restart.

## Testing
There are no tests yet. Put new logic in plain functions that don't need a Discord `Context`, so it can be unit-tested with `#[cfg(test)]` modules.

## Agent skills

### Issue tracker

Issues live in GitHub Issues on `guitaristtuck/mtg-collection-bot` (via `gh`). See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
