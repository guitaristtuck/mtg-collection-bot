# mtg-collection-bot
MTG collection bot to be used with a group of friends

## Running locally
1. Create a `.env` in the repo root:
   ```sh
   DISCORD_TOKEN=...          # bot token
   GUILD_ID=...               # server to register slash commands in (use the playground server for testing)
   MOXFIELD_USER_AGENT=...
   ```
2. From the repo root (config is read from `config/` and `log4rs.yaml`): `cargo run`

## Development
PRs must pass `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
Merging to `main` deploys to the production server automatically.
