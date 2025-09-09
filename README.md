Deadlock CLI
============

A production-grade Rust CLI to look up Deadlock player profiles and stats by Steam account.

Features
- Interactive menu and non-interactive subcommands
- Converts Steam Community vanity names/URLs to SteamID64
- Queries the Deadlock API Players endpoints for profile, MMR and hero stats
- Pretty terminal tables and optional `--json` output
- Retries with backoff, error handling for 404 and rate limits (exit code 29)
- PostgreSQL persistence (SQLx) enabled by default; data auto-saves on lookups

Install / Build
- Requires Rust (edition 2024). In dev, env is loaded from `.env` via `dotenvy`.
- If your toolchain predates the 2024 edition, run `rustup update stable`.

Commands
- `make build` – builds release
- `make run` – runs the CLI
- `make fmt` – rustfmt
- `make clippy` – clippy with `-D warnings`
- `make test` – unit tests

Environment Variables
- `DEADLOCK_API_BASE` (default `https://api.deadlock-api.com`)
- `DEADLOCK_API_KEY` (optional; sent via `X-API-KEY` header if present)
- `STEAM_WEB_API_KEY` (required for vanity resolution)
- `STEAM_WEB_API_BASE` (optional; defaults to `https://api.steampowered.com`, used for tests)
- `DATABASE_URL` (optional; default `postgres://postgres:@localhost:5432/deadlock`)

Usage
- Interactive (no args):
  Deadlock CLI
  1) Lookup by SteamID64
  2) Lookup by Steam Community ID (vanity name)
  3) Lookup by Steam Community URL
  j) Toggle JSON output
  q) Quit

- Non-interactive:
  - `deadlock-cli by-steamid --id 76561197960435530`
  - `deadlock-cli by-steamid3 --id3 [U:1:388674065]` (or raw account id)
  - `deadlock-cli by-vanity --name gabelogannewell`
  - `deadlock-cli by-url --url https://steamcommunity.com/id/gabelogannewell`
  - Add `--json` to any for raw JSON

- Matches ingestion:
  - Known IDs: `deadlock-cli matches sync --id 1234567890,1234567891`
  - From a player: `deadlock-cli matches sync --from-steamid 7656119XXXXXXXXXX`
  - Range probe: `deadlock-cli matches sync --since-id 120000000 --limit 1000 --batch-size 100`
  - Flags:
    - `--id <i64>[,<i64>...]` repeatable/comma-separated explicit match IDs
    - `--from-steamid <id|url|vanity>` or `--from-id3 <[U:1:Z]|Z>` derive match IDs from the player's MMR history
    - `--since-id <i64>`: only ingest matches with `match_id > since_id` (default: `MAX(match_id)` in DB)
    - `--until-id <i64>`: stop at `match_id <= until_id`
    - `--limit <usize>`: max total matches to pull (default 500)
    - `--batch-size <usize>`: IDs per bulk request (default 100)
    - `--include-info`, `--include-players`: include expanded payload blocks
    - `--dry-run`: fetch and parse only; skip DB writes

Data Sources
- Deadlock API OpenAPI: https://api.deadlock-api.com/docs (we use:
  - `GET /v1/players/steam` (SteamProfile)
  - `GET /v1/players/mmr` (MMRHistory)
  - `GET /v1/players/hero-stats` (HeroStats)
)
- Vanity resolution: `https://api.steampowered.com/ISteamUser/ResolveVanityURL/v1/`

Example Output (tables)
- Profile table: name, SteamID64, account_id, country, profile URL, rank
- Stats table: total matches, wins, win rate

JSON Output
- Includes combined payload: profile, latest MMR, hero stats, steamid64 and account_id.

Exit Codes
- 0 on success
- 1 on general error
- 29 on API rate limit (429)
PostgreSQL
- Enabled by default; set `DATABASE_URL` to configure connection.
- Requires a reachable PostgreSQL (default: `postgres://postgres:@localhost:5432/deadlock`).
- Migrations run automatically during lookups; manual:
  - `deadlock-cli migrate` – create DB if missing and run migrations.
- On every lookup (by-steamid/by-vanity/by-url/interactive), the CLI fetches data and persists it.
- Additionally, the CLI fetches the player's stored match history by default and persists it to `matches` and `match_players`.
- Schema highlights:
  - `players` (1 row per account, plus profile extras in `jsonb`)
  - `latest_mmr` (snapshot) and `mmr_history` (append-only)
  - `hero_stats_current` (per-hero aggregates) and `hero_stats_history` (append-only JSON snapshots)
  - Generated columns for `profile_domain` and `win_rate`
 - Match history (per player):
   - `deadlock-cli matches history --steamid 7656119XXXXXXXXXX`
   - `deadlock-cli matches history --id3 [U:1:388674065]`
   - Flags:
     - `--account-id | --steamid | --id3` to choose the player
     - `--force-refetch` or `--only-stored-history` (mutually exclusive)
     - `--dry-run` to skip DB writes
