# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `LICENSE` (MIT).
- CI runs `cargo audit` against `Cargo.lock` on every push and pull request.

### Removed
- Named environments other than `prod` and `prod-eu`. Any other origin is set explicitly with `base_url` / `APPCUES_BASE_URL` and `tools_base_url` / `APPCUES_TOOLS_BASE_URL`. A saved profile with an unknown `env` value fails to load with a config error naming it.

### Security
- Bumped `rustls` to 0.23.45 (RUSTSEC-2026-0285) and `time` to 0.3.55 (RUSTSEC-2026-0009).

## [0.3.0] - 2026-09-17

### Added
- `appcues mcp`: a Model Context Protocol server over stdio, so Claude Code (`claude mcp add appcues -- appcues mcp`), Codex, and other MCP clients can use the account as typed tools (`list_flows`, `publish_flow`, `create_segment`, `update_user`, `run_analytics_query`, `call_account_tool`, ...). Tools call the same command functions as the CLI with the active profile's credentials; failures come back as `isError` results carrying the CLI's one-line JSON error. Interactive prompting is always off in MCP mode; `--dry-run mcp` serves a preview-only server.
- `scripts/dev {build|install|test|run}`: runs cargo from `crates/appcues` so the gate, a release build, `cargo install`, and `cargo run` work from the repo root.

### Changed
- Minimum supported Rust is now 1.88 (the MCP SDK's floor).

## [0.2.3] - 2026-09-10

### Removed
- `docs/install-macos.md` moved to the skills repo as [docs/install.md](https://github.com/appcues/skills/blob/main/docs/install.md), the one install guide across runtimes. The README's Install section keeps the CLI binary steps.
- `docs/cross-runtime-skill-portability.md` and `docs/skill-ecosystem-survey.md` moved to the skills repo's `docs/` as well.

## [0.2.2] - 2026-09-10

### Removed
- `skills/`, `plugin.json`, and `.claude-plugin/` moved to the [appcues/skills](https://github.com/appcues/skills) repo, which installs as one plugin on Hermes, Claude Code, Codex, and OpenClaw. `runtimes/sandbox.sh` clones that repo into the gitignored `runtimes/skills/` and the sandboxes mount it.

## [0.2.1] - 2026-09-10

### Added
- `-v` as a short alias for `--version` (`-V` still works).

### Changed
- `profiles list` table shows the API URL and tools URL stacked in a single `url` column. JSON output is unchanged.

## [0.2.0] - 2026-09-08

### Changed

- **Breaking**: destructive commands (`segments delete`, `users delete`, `profiles remove`, non-read-only `tools call`) no longer prompt for confirmation by default, they just run; safety is delegated to API key permissions. The `--yes` flag is removed everywhere. Prompting is now opt-in: `-i`/`--interactive` per invocation, `interactive = true` in the profile, or `APPCUES_INTERACTIVE=true`; setting `APPCUES_INTERACTIVE=false` overrides a profile's `interactive = true` and disables prompting. With prompting on and stdin not a terminal, the command refuses with a config error (exit 3) instead of hanging, so an agent holding a pty can never block on a y/N prompt.
- `tools call` in the non-interactive default no longer makes the `GET /v1/tools/<name>` readOnlyHint pre-check; the lookup and prompt happen only in interactive mode.

### Added

- `appcues analytics +compare --spec <FILE|-> [--days N]`: run any aggregate spec (`metrics` + `dimensions`) over the last N days and the same-length window before, joined client-side per dimension value with `current`, `previous`, and `deltas.<metric>_pct` (`null` without a baseline). Rows missing from one window count as zero there. Raw `columns` specs are rejected. `--dry-run` prints both would-be POSTs
- Three read-only skills built on `analytics +compare`, each with verified reference specs and a MUST report skeleton: `experience-errors` (error-rate spikes per experience from the account tools, plus step errors by flow/experience, step, and error message vs the previous period, covering `appcues:step_error`, `appcues:step_child_error`, and Flows 2.0 `appcues:v2:step_error`; written for a daily run by an external scheduler), `nps-sentiment` (NPS surveys and results through the account tools with the analytics specs as fallback, themes and verbatim quotes from the written replies), and `flow-step-dropoff` (per-step funnel of one flow)
- Two read-only skills on the account tools (the MCP tool set, callable as `appcues tools call`): `dashboard-digest` (narrates an existing dashboard card by card with the change vs the previous window) and `campaign-report` (a campaign's objective, tactics, content, and engagement vs the previous period)
- `analytics-query/references/spec.md` lists the analytics engine's dimensions and metrics by group, including the verified server limits on the NPS metrics and raw columns

- CI publishes a GitHub Release on every merge to `main` that bumps the Cargo version: tag `vX.Y.Z`, the CHANGELOG section as notes, and `appcues-<target>.tar.xz` for mac arm64/x86_64 and linux arm64/x86_64. Replaces the 90-day workflow artifacts

### Fixed

- `--dry-run -o json` on composite commands (`flows +digest`, `analytics +compare`) prints one JSON array of the would-be requests instead of two concatenated documents, so the output pipes into `jq`
- `appcues tools list|describe|call`: discover and call account tools (the MCP tool set) over HTTP, against a second per-environment origin (`mcp.appcues.com`, `mcp.eu.appcues.com`), authenticated with the existing key/secret sent as the `appcues-api-key` header
- `tools list` requests the lightweight summary view by default (`GET /v1/tools?view=summary`: name, role, description); `--full` fetches complete entries with `inputSchema` and `annotations`. Paired with `tools describe <name>`, the default gives agents a search-then-load flow instead of holding every schema in context
- `tools call <NAME>` takes arguments as `--input '<json>'` (or `--input -` for stdin), `--input-file <path>`, or repeated `--attr key=value`; renders data-first (the envelope's `data` list, else JSON parsed out of a single text result, else the text verbatim), `--raw` prints the whole envelope, and image results are written to `--out <dir>` as PNG files
- In interactive mode, non-read-only tools confirm before running (one extra `GET /v1/tools/<name>` checks `readOnlyHint`); `--dry-run` prints the would-be POST and sends nothing
- A tool that runs but reports failure (`isError: true`) exits 4 with `type: "tool"` and the full envelope embedded in the one-line stderr JSON error
- `tools_base_url` profile field, `--tools-base-url` on `profiles add|edit`, and `APPCUES_TOOLS_BASE_URL` override, mirroring `base_url`; `profiles list` shows the resolved tools origin

## [0.1.3] - 2026-08-31

### Added

- `appcues experiences publish|unpublish <TYPE> <EXPERIENCE_ID>` — publish lifecycle for all per-type experience routes (pins, mobile, launchpads, banners, flows-v2, embeds, nps)
- `appcues checklists publish|unpublish <CHECKLIST_ID>` — publish lifecycle for checklists
- `appcues screenshots <RESOURCE_ID> [--out FILE]` — download a flow/experience/checklist's draft screenshots ZIP via the type-agnostic `GET screenshots/{resource_id}` route, streamed to disk with the same temp-file-then-rename safety as `jobs download`

## [0.1.2] - 2026-08-31

### Added

- Restored all mutating commands removed in 0.1.1: `flows publish|unpublish`, `segments create|update|delete|add-users|remove-users`, `users update|delete|track`, and `groups update|add-users`. Safety is delegated to API key permissions (read-only vs write, chosen at key creation) and to the operator running the CLI.
- Claude Code plugin packaging (`.claude-plugin/marketplace.json` + `plugin.json`): the repo installs as a plugin marketplace via `/plugin marketplace add appcues/cli`, shipping all `skills/` in one `appcues-skills` plugin with the skill directories untouched

## [0.1.1] - 2026-08-27

### Removed

- All commands that mutate customer-visible state, making the CLI's API surface read-only: `flows publish|unpublish`, `segments create|update|delete|add-users|remove-users`, `users update|delete|track`, and `groups update|add-users`. Analytics queries and export jobs stay (they create server-side job resources, not customer-visible changes), local `profiles` management stays, and the `--dry-run` plumbing stays for when writes return.

### Added

- `appcues experiences list|get <TYPE>` — read experience content by type (`pins|mobile|launchpads|banners|flows-v2|embeds|nps`); the public API has no single experiences route, so the type value maps directly to its per-type route
- `appcues checklists list|get` — read checklists
- `appcues flows +digest [--days 7]` — first composite command: published flows' performance for the last N days vs the previous same-length period, with deltas, joined client-side from `GET flows` plus two analytics queries
- `appcues analytics query --spec <FILE|->` — run a general analytics spec against the v2 analytics API (sync by default, rows inline; `--async` submits an export job and returns a `job_id`); the spec is validated server-side only, stdin supported via `--spec -`
- `appcues jobs wait <JOB_ID>` — poll an analytics export job until done or failed, with `--timeout` (default 15m)
- `appcues jobs download <JOB_ID> [--out FILE]` — wait for an analytics export job, stream its result to a local file (default `<JOB_ID>.json`, fetched without auth headers), and print the path; the presigned URL never appears in output
- `appcues profiles list` — list saved profiles (name, account, env, URL) with credentials truncated to their first 5 characters
- Rate-limit and truncation visibility: responses carrying `X-RateLimit-*`/`X-Concurrency-*`/`X-Rows-Truncated` headers are echoed as one `{"rate_limit":{...},"rows_truncated":true}` line on stderr, and API errors embed the structured error body and rate-limit state in the JSON error line
- `appcues jobs get <JOB_ID>` — inspect an async job's status
- `--dry-run` global flag: write commands print the request they would send (method, path, body) and exit without calling the API
- Typed exit codes (0 ok, 1 unexpected, 2 usage, 3 config/auth, 4 API 4xx, 5 rate-limited/5xx after retries) and failures as one structured JSON line on stderr, so scripts and agents can branch without parsing prose
- Non-interactive `appcues profiles add` via `--api-key`/`--api-secret`/`--account-id`/`--env` flags; missing flags without a terminal fail with a clear error instead of hanging on a prompt
- Client-side request throttling (~50 req/s) to stay under the API's 60 req/s limit
- `runtimes/` directory with local-docker skill-dev sandboxes for Hermes (`runtimes/hermes/`) and OpenClaw (`runtimes/openclaw/`), plus the runtime-vs-skill convention in `runtimes/README.md`
- `skills/` directory with the portable skill contract (`skills/README.md`) and three skills authored against it: `account-inventory` (account contents report), `analytics-query` (analytics questions and raw event exports, with verified reference specs), and `weekly-performance-digest` (published flows' performance vs the previous period, built on `flows +digest`)
- CI release binaries: every merge to `main` builds `appcues` for linux arm64 and macOS arm64, uploaded as workflow artifacts named `appcues-<arch>-<version>-<short-sha>`

### Changed

- `auth` command group renamed to `profiles`: `auth login` is now `profiles add [NAME]` (positional name, default `default`; refuses an existing profile and points at `edit`), `auth status` is now the top-level `appcues status`, plus new `profiles edit <NAME>` (partial update, omitted flags keep their value, `--base-url ""` clears the URL) and `profiles remove <NAME> [--yes]`
- `region` (us/eu) replaced by named environments: `env = prod|prod-eu` on the profile, `--env` on `profiles add`, `APPCUES_ENV` override; configs still carrying the legacy `region` key (or any unknown key) are rejected with a migration hint instead of silently changing environments, and `base_url` values must be absolute http(s) URLs
- New `base_url` profile field and `APPCUES_BASE_URL` env var: an explicit API origin that wins over `env`, for local apis and proxies; `appcues status` now prints the URL it verified against
- `appcues jobs get` now reads the analytics exports route; job ids minted by the legacy email-delivery export system return 404 and cannot be inspected via the CLI
- Repo restructured: the Rust crate moved from the root into `crates/appcues/`, making room for `skills/` and other non-CLI deliverables
- Runtime credentials now live in one shared, gitignored `runtimes/.env` loaded via docker-compose `env_file` (moved from per-runtime `data/.env`)

### Fixed

- `appcues groups add-users` now sends `POST` instead of `PATCH` — the API only defines `POST` for `/groups/:group_id/users`

## [0.1.0] - 2026-08-12

### Added

- `appcues auth login` / `appcues auth status` — profile-based credentials in `~/.config/appcues/config.toml`, env-var overrides for CI
- `appcues flows list|get|publish|unpublish`
- `appcues segments list|get|create|update|delete|add-users|remove-users`
- `appcues tags list|get`
- `appcues users get|update|delete|events|track`
- `appcues groups get|update|add-users`
- Global flags `--profile`, `--account`, `-o table|json`
- US/EU region support, retry with backoff on 429/5xx, confirmation prompts on destructive commands
