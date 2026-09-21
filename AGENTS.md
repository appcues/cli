# AGENTS.md

Agent-facing guide to this repo. `CLAUDE.md` points here.

## Hard rules

- **Never read, print, or diff `.env` files** — they hold live keys.
  Move/mount/chmod only.
- **Tests never touch real services or real config.** All HTTP is mocked
  with `httpmock` (client takes its base URL as a parameter); config tests
  use temp files; credentials in tests are always fake
  (`test-key`/`test-secret`).
- **Keep all HTTP behind `client.rs`.** One module boundary, no trait
  system, no plugin registry. The tools routes (the MCP tool set over
  HTTP) are the second backend: same `Client`, built with
  `Client::with_header_auth` against `Env::tools_base_url()`.

## What this repo is

The Rust CLI around the Appcues Public API v2, in `crates/appcues/` (all
cargo commands run from here).

The agent skills that use it, and the runtime setup guides (Hermes,
OpenClaw, Claude Code, Codex), live in the `appcues/skills` repo.

## CLI architecture

Data flow: `main.rs` (clap → dispatch) → `config.rs` (profile file
`~/.config/appcues/config.toml` + `APPCUES_*` env overrides; named
environments `prod|prod-eu` map to base URLs,
`base_url`/`APPCUES_BASE_URL` overrides them; the repo is public, so
non-production hostnames never go in code or docs, only in a local profile) → `commands/*` (thin: build
path + body, pick table columns) →
`client.rs` (Basic auth, retry/backoff on 429/5xx, ~50 req/s throttle,
streaming `download`) → `output.rs` (`-o table|json`).

Responses stay `serde_json::Value` — no typed response structs by design;
the CLI displays what the API returns.

`mcp.rs` is a second front end over the same `commands/*` functions: an
[rmcp](https://github.com/modelcontextprotocol/rust-sdk) stdio server
(`appcues mcp`) with one typed tool per command. Each tool runs the command
function in `spawn_blocking` with the process `Ctx` (format forced to JSON,
interactive forced off, `dry_run` kept) and returns the rendered string as
text; an `Err` becomes an `isError` result whose text is
`output::render_error`'s one-line JSON. A new command gets a tool by adding
a params struct (`Deserialize + JsonSchema`, doc comments become the
schema descriptions) and a `#[tool]` method that calls the command; the
catalog test in `tests/mcp.rs` lists the names it expects. Nothing
MCP-specific may build requests or talk HTTP: that stays in `commands/*` and
`client.rs`.

Conventions every command inherits:

- **Errors**: `client::ApiError` / `config::ConfigError` are downcast in
  `main::classify` to typed exit codes (0 ok, 1 unexpected, 2 usage,
  3 config/auth, 4 API 4xx, 5 rate-limited/5xx). Every failure prints
  exactly one JSON line to stderr. A new fallible path must land in the
  right bucket — add a downcast test.
- **`--dry-run`**: every write command guards with `ctx.dry_run_output()`
  before any request and before `confirm()`. Anything that mutates
  (API, filesystem) gets the guard.
- **`confirm()` is opt-in**: destructive commands run without asking by
  default (agent-first; safety is the API key's permissions). Prompting
  happens only in interactive mode (`-i`, profile `interactive = true`,
  or `APPCUES_INTERACTIVE`), and off a tty it is a ConfigError, never a
  hang. There is no `--yes` flag.
- **Data to stdout, everything else to stderr/JSON-error.** This is also
  what keeps `appcues mcp` working: stdout is the MCP transport there, so
  nothing in the library may `println!`; only `main.rs` prints the result.

Composite commands (clap subcommand names starting with `+`, e.g.
`flows +digest`) orchestrate several client calls client-side and join the
results; shared period math lives in `commands/composite.rs`. They follow
every existing convention: dry-run prints the would-be POSTs and sends
nothing, errors from any step propagate as the usual one-line JSON with
typed exit codes, and specs embed documented event names only (the server
does not validate condition values — a wrong event name yields zero rows).

API facts (verified against https://api.appcues.com/v2/docs): list
endpoints do not paginate. There is no single `experiences` route —
experience content is split into per-type routes (`pins`, `mobile`,
`launchpads`, `banners`, `flows-v2`, `embeds`, `nps`); the `experiences`
command's TYPE value doubles as the path segment. Screenshots are one
type-agnostic route (`GET screenshots/:resource_id`) returning a ZIP, and
only for draft content. The legacy email-delivery export route
(`POST export/events`) is deliberately not exposed by this CLI; raw
event dumps go through the analytics routes below. Presigned download
URLs are fetched **without** the Authorization header.

The v2 analytics routes (`POST analytics/query`, `POST analytics/exports`,
`GET analytics/exports/:job_id`) take a general spec JSON the CLI never
validates semantically (JSON well-formedness only; the server's normalize
layer is the single validator). Sync query 200s are a bare rows array.
Job statuses are `queued|running|done|failed`; `jobs get`/`jobs wait`
serve this route only, so legacy export-system job ids 404 there.
Segment membership export jobs are one of those legacy cases: they
report status at `GET /v2/accounts/:id/jobs/:job_id` (the legacy
job_api route) with statuses `IN_PROGRESS|COMPLETED|FAILED`, and those
ids 404 on the analytics exports route. Error
bodies use api's v2 ErrorView shape
(`{"error", "status", "title", "detail"}`). Responses carrying
`X-RateLimit-*`/`X-Concurrency-*`/`X-Rows-Truncated` headers are echoed
as one `{"rate_limit":{...},"rows_truncated":true}` JSON line on stderr;
API error bodies and rate-limit state are embedded in the one-line JSON
error (`body`, `rate_limit` keys).

`docs/best-practices.md` collects review-derived practices; follow it
while building and as the pre-PR self-review pass.

## Commands

```bash
cd crates/appcues
cargo test                          # full suite (hermetic, fast)
cargo test --test analytics         # one integration suite (tests/*.rs)
cargo test invalid_env              # tests matching a name
cargo fmt --check                   # gate: formatting
cargo clippy --all-targets -- -D warnings   # gate: lints
cargo audit                         # gate: dependency advisories (CI runs it too)
cargo llvm-cov --summary-only       # line coverage per file (cargo install cargo-llvm-cov)
cargo install --path .              # install the binary
```

`scripts/dev {build|install|test|run}` wraps the same commands from the
repo root; `scripts/dev test` is the gate.

The gate (fmt --check, clippy -D warnings, test, audit) must be clean before
a change counts as done; warnings in test output are findings. `cargo audit`
warns about the unmaintained `async-std` that httpmock pulls in as a dev
dependency; that warning is expected and does not fail the gate.

## Gotchas

- `rust-version = "1.88"` (rmcp's floor); let-chains are allowed, and
  clippy's `collapsible_if` will ask for them.
- ureq 3: `timeout_global` covers body streaming — long downloads override
  it per-request and bound stalls with `timeout_recv_body` instead.
  `read_to_string` caps at 10MB; stream big bodies via
  `body_mut().as_reader()` + `io::copy`.
- `docs/superpowers/` and `.superpowers/` are gitignored working docs —
  don't reference them from versioned files.
