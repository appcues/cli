# Appcues CLI & agentic tooling

A command-line client for the [Appcues API v2](https://api.appcues.com). It
covers flows, experiences, checklists, segments, tags, users, and groups,
plus analytics queries and exports, with profile-based authentication.
Mutating commands (publishing, editing, deleting) are supported; access
control comes from the API key's permissions (read-only vs write) and from
the operator running the CLI.

The Rust CLI crate lives in `crates/appcues/`; all `cargo` commands run
from there.

The agent skills that use this CLI, and the guide to setting up an agent
runtime with it, live in [appcues/skills](https://github.com/appcues/skills).
Start there; this README documents the CLI itself.

This CLI is a companion to the Appcues MCP server,
not a replacement for it. The MCP server is built for agents (Claude, other
LLM tools) to call the API programmatically inside a conversation. This CLI
is built for a human at a terminal or a CI script — scriptable, pipeable, and
scoped to `-o json` output when you need to feed the result to something
else. It also doubles as a local MCP server: `appcues mcp` exposes the same
commands as typed tools over stdio, using the saved profile's credentials,
for agents that run on your machine (see "MCP server" below).

## Install

With Homebrew (macOS and Linux):

```bash
brew install appcues/tap/appcues
```

The formula lives in [appcues/homebrew-tap](https://github.com/appcues/homebrew-tap)
and is updated after each release. Upgrade with `brew upgrade appcues`.

### From source

```bash
git clone https://github.com/appcues/cli.git
cd cli
cargo install --path crates/appcues
```

This installs an `appcues` binary onto your `PATH` (wherever `cargo install`
puts binaries, typically `~/.cargo/bin`).

### Releases

Every merge to `main` that bumps the version in `crates/appcues/Cargo.toml`
publishes a GitHub Release tagged `vX.Y.Z`, with the matching CHANGELOG
section as its notes and one tarball per platform:
`appcues-<target>.tar.xz` for `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`aarch64-unknown-linux-gnu`, and `x86_64-unknown-linux-gnu`. Each holds a
directory of the same name containing the `appcues` binary. Merges that do
not bump the version release nothing. Download a tarball from the Releases
page, or with the GitHub CLI:

```bash
gh release download -R appcues/cli -p 'appcues-aarch64-apple-darwin.tar.xz'
tar -xf appcues-aarch64-apple-darwin.tar.xz --strip-components=1 '*/appcues'
```

Then put the binary on your PATH:

```bash
mkdir -p ~/.local/bin
install -m 755 appcues ~/.local/bin/
appcues --version
```

If the last command fails, `~/.local/bin` is not on your PATH; export it from
your shell's login profile. The macOS binary is unsigned and not notarized, so
Gatekeeper quarantines browser downloads; clear the flag with
`xattr -d com.apple.quarantine appcues`, or download with `gh`, which skips
quarantine.

## Quick start

```bash
appcues profiles add
appcues status
appcues flows list
appcues flows publish 3f8a1c2e-1111-4444-9999-abcdefabcdef
```

`profiles add` prompts for an API key, API secret, account ID, and environment,
and saves them as the `default` profile, which every command uses unless you
pass `--profile`. `status` checks the saved credentials against the API.
Everything else reads from that profile unless you override it with flags or
environment variables.

In CI/automation contexts, pass `--api-key`, `--api-secret`, `--account-id`,
and `--env` flags to `profiles add` to skip all prompts — no interaction
needed. Alternatively, set credentials via environment variables
(`APPCUES_API_KEY`, `APPCUES_API_SECRET`, `APPCUES_ACCOUNT_ID`,
`APPCUES_ENV`) and skip `profiles add` entirely.

## Commands

```
appcues profiles add [NAME] [--api-key K --api-secret S --account-id A]
                     [--env prod|prod-eu] [--base-url <URL>]
                     [--tools-base-url <URL>]
                                           Save a new profile (default name: default)
appcues profiles list                      List saved profiles (credentials truncated)
appcues profiles edit <NAME> [--api-key K --api-secret S --account-id A --env E --base-url U --tools-base-url U]
                                           Update fields; omitted flags keep their value
appcues profiles remove <NAME>             Delete a profile
appcues status                             Verify the active profile's credentials
appcues mcp                                Serve the account as MCP tools over stdio (for
                                           Claude Code, Codex, and other MCP clients)

appcues flows list                         List all flows
appcues flows get <FLOW_ID>                Show one flow
appcues flows publish <FLOW_ID>            Publish a flow
appcues flows unpublish <FLOW_ID>          Unpublish a flow
appcues flows +digest [--days 7]           One-call digest: published flows' performance
                                           this period vs the previous period, with deltas

appcues experiences list <TYPE>            List all experiences of one type
appcues experiences get <TYPE> <EXPERIENCE_ID>
                                           Show one experience
appcues experiences publish <TYPE> <EXPERIENCE_ID>
                                           Publish an experience
appcues experiences unpublish <TYPE> <EXPERIENCE_ID>
                                           Unpublish an experience
                                           TYPE: pins|mobile|launchpads|banners|flows-v2|embeds|nps
                                           (the API has no single experiences route; each
                                           type is its own resource)

appcues checklists list                    List all checklists
appcues checklists get <CHECKLIST_ID>      Show one checklist
appcues checklists publish <CHECKLIST_ID>  Publish a checklist
appcues checklists unpublish <CHECKLIST_ID>
                                           Unpublish a checklist

appcues screenshots <RESOURCE_ID> [--out FILE]
                                           Download a flow/experience/checklist's draft
                                           screenshots as a ZIP (default:
                                           <RESOURCE_ID>-screenshots.zip); one API route
                                           serves every type, the id alone identifies it

appcues segments list                      List all segments
appcues segments get <SEGMENT_ID>          Show one segment
appcues segments create --name <NAME> [--description <DESCRIPTION>]
appcues segments update <SEGMENT_ID> [--name <NAME>] [--description <DESCRIPTION>]
appcues segments delete <SEGMENT_ID>
appcues segments add-users <SEGMENT_ID> --user-ids <USER_IDS>
appcues segments remove-users <SEGMENT_ID> --user-ids <USER_IDS>

appcues tags list                          List all tags
appcues tags get <TAG_ID>                  Show one tag

appcues users get <USER_ID>                Show a user's profile
appcues users update <USER_ID> --attr <ATTRS>       Update profile attributes (repeat --attr key=value)
appcues users delete <USER_ID>             Delete a user's profile
appcues users events <USER_ID> [--limit <LIMIT>]    Show a user's recent events
appcues users track <USER_ID> --name <NAME> [--timestamp <TIMESTAMP>] [--attr <ATTRS>]

appcues groups get <GROUP_ID>               Show a group's profile
appcues groups update <GROUP_ID> --attr <ATTRS>     Update group attributes (repeat --attr key=value)
appcues groups add-users <GROUP_ID> --user-ids <USER_IDS>

appcues analytics query --spec <FILE|-> [--async]
                                           Run an analytics spec (sync), or submit an async export job
appcues analytics +compare --spec <FILE|-> [--days 7]
                                           Run an aggregate spec for this period and the previous
                                           one, joined per dimension value with per-metric deltas
appcues jobs get <JOB_ID>                  Show an analytics export job's status
appcues jobs wait <JOB_ID> [--timeout 15m] Poll an analytics export job until done or failed
appcues jobs download <JOB_ID> [--out FILE] [--timeout 15m]
                                           Wait for the job, download its result, print the path

appcues tools list [--full]                List the account tools available to this API key
                                           (summary by default; --full includes schemas)
appcues tools describe <NAME>              Show one tool's entry, including its input schema
appcues tools call <NAME> [--input <JSON|-> | --input-file <FILE> | --attr k=v ...]
                   [--raw] [--out <DIR>]
                                           Call a tool by name; prefer this over raw API
                                           calls for anything without a typed command
```

Run `appcues --help` or `appcues <group> --help` for the exact flags on any
command — the above is a summary, not a substitute.

Notes on flag shapes:

- `--user-ids` takes a comma-separated list: `--user-ids user_1,user_2,user_3`.
- `--attr` is repeatable, one `key=value` pair per flag:
  `--attr plan=pro --attr seats=12`.
- Destructive commands run without asking by default. Pass `-i` (or
  `--interactive`), or set `interactive = true` in the profile, to get a
  y/N prompt first; see "Interactive confirmation" below.
- `--timestamp` on `users track` takes RFC 3339 (e.g.
  `2026-08-12T00:00:00Z`) and defaults to now.
- `--dry-run` previews the request a write command would send (method, path,
  body) and exits 0 without calling the API; reads still run normally.
  Dry-run previews bypass confirmation prompts, even in interactive mode.

### Analytics queries and exports

`analytics query` POSTs a spec JSON file (or stdin with `--spec -`) to the
v2 analytics API. The CLI checks only that the spec is well-formed JSON:
the server validates everything else and returns structured 400s naming
the offending field. Sync (the default) returns the rows inline as a bare
JSON array (use `-o json` to pipe it); `--async` submits an export job
and returns a `job_id`. `jobs download <JOB_ID>` waits for the job and
downloads its result to a local file (default `<JOB_ID>.json`), printing
the path — the presigned URL never touches the output. To handle the
steps yourself, poll with `jobs get`, or block with `jobs wait <JOB_ID>`
until it reports `done` (which includes a presigned `download_url`,
valid for 1 hour — if it expires, `jobs get` mints a fresh one) or
`failed` (which includes a `failure_reason`).

Responses from these routes carry rate-limit state (`X-RateLimit-*`,
`X-Concurrency-*`) and a truncation marker (`X-Rows-Truncated`) when the
server clamps a sync result to its row cap; the CLI echoes them as one
JSON line on stderr, e.g.
`{"rate_limit":{"remaining":41},"rows_truncated":true}`, so scripts and
agents can pace themselves.

```bash
appcues analytics query --spec q.json -o json
cat q.json | appcues analytics query --spec - --async
appcues jobs download <JOB_ID> --out result.json
```

`jobs get` and `jobs wait` serve analytics export jobs only; job ids
minted by the legacy email-delivery export system are not visible on
this route and return a 404.

### Composite commands

Commands whose names start with `+` (Google Workspace CLI style) combine
several API calls into one expressive answer. `flows +digest` runs
`GET flows` plus two analytics queries — the last `--days` (default 7)
and the same-length window immediately before — and joins them
client-side into one report: shown, completed, skipped, errors, unique
users, and completion rate per published flow, each with the
previous-period numbers and deltas.

`analytics +compare` does the same period math for any aggregate spec
(`metrics` + `dimensions`): it replaces the spec's `start_time`/`end_time`
with the last `--days` and the same-length window before, runs both, and
joins the rows on their dimension values. Each row carries `current`,
`previous`, and `deltas` (`<metric>_pct`, `null` when the previous value is
0), sorted by the first metric's current value descending. A row present
in only one window counts as zero in the other. Raw `columns` specs are
rejected: there is nothing to compare.

```bash
appcues flows +digest --days 7 -o json                 # weekly digest input, one call
appcues analytics +compare --spec errors.json --days 1 -o json   # today vs yesterday
```

### Account tools

`appcues tools` talks to a second origin: the account tools service, which
serves the same tool set as the Appcues MCP server over plain HTTP
(`GET /v1/tools`, `GET /v1/tools/:name`, `POST /v1/tools/:name`). It uses
the same API key and secret as everything else, sent as the
`appcues-api-key` header. Tool names are identical on both transports, so
a skill can name a tool once and run on either.

`tools list` requests the lightweight summary view (name, role,
description; a fraction of the full payload), which pairs with
`tools describe <name>` as a search-then-load flow: find the tool in the
summary, then fetch only its schema. `--full` fetches complete entries
with `inputSchema` and `annotations` in one call. A server without the
summary view returns the full listing for either form, so the flag is
safe against older servers.

`tools call` builds the tool's `arguments` object from `--input '<json>'`
(or `--input -` for stdin), `--input-file <path>`, or repeated
`--attr key=value` pairs (values are JSON-typed: `3` is a number, `true`
a boolean). The flags are mutually exclusive; with none of them the tool
runs with empty arguments.

Output follows the data-first rule: when the result carries a `data` list
it is rendered (columns `type`, `id`, `name`); otherwise a single text
result that parses as JSON is printed parsed; otherwise the text prints
verbatim. `--raw` prints the whole result envelope instead. Image results
(screenshots) are written to `--out <dir>` (default: the current
directory) as PNG files.

In interactive mode (`-i`, or `interactive = true` in the profile), a
tool that is not read-only asks for confirmation first, after one extra
`GET /v1/tools/<name>` to check the tool's `readOnlyHint`. The
non-interactive default calls the tool directly with no extra request.
`--dry-run` prints the would-be POST and sends nothing, like every other
write.

A tool that runs but reports failure (`isError`) exits 4 with `type`
`"tool"` in the one-line JSON error; the full envelope is embedded under
`body`.

### MCP server

`appcues mcp` starts a [Model Context Protocol](https://modelcontextprotocol.io)
server on stdin/stdout. It is the same binary and the same command code as
the CLI, presented as strongly typed tools: `list_flows`, `get_flow`,
`publish_flow`, `list_experiences(experience_type)`,
`create_segment(name, description?)`, `update_user(user_id, attributes)`,
`run_analytics_query(spec)`, `start_analytics_export(spec)`,
`download_export_job(job_id, ...)`, `call_account_tool(name, arguments)`,
and so on, one per CLI command. There is no "run a CLI command" tool: every
tool has a name, a description, and a JSON schema an agent can read without
knowing the CLI syntax. Run `appcues mcp` and send a `tools/list` request,
or look at the tool list in your MCP client, for the full catalog.

Credentials, account, and environment come from the active profile exactly
as for the CLI (`--profile`, `--account`, and the `APPCUES_*` variables all
apply), so the MCP client never sees or handles the API key.

Register it for yourself in the current project only (stored in your own
Claude Code config, not in the repo):

```bash
claude mcp add appcues -- appcues mcp
```

Register it for every project on this machine:

```bash
claude mcp add -s user appcues -- appcues mcp
```

Register it for everyone who clones a repo, written to that repo's
`.mcp.json` so it is versioned:

```bash
claude mcp add -s project appcues -- appcues mcp
```

Any of these accepts CLI flags before `mcp`, e.g. `--profile eu` to pin
a profile or `--dry-run` for a preview-only server:

```bash
claude mcp add appcues -- appcues --profile eu mcp
```

Any other MCP client takes the equivalent stdio configuration. For Codex,
add to `~/.codex/config.toml`:

```toml
[mcp_servers.appcues]
command = "appcues"
args = ["mcp"]
```

For clients that read a JSON `mcpServers` map (Claude Desktop, Cursor, and
most others):

```json
{
  "mcpServers": {
    "appcues": {
      "command": "appcues",
      "args": ["mcp"]
    }
  }
}
```

Behavior worth knowing:

- Read tools return the API's JSON verbatim, except the two composites
  (`flow_performance_digest`, `compare_analytics_periods`), which return the
  CLI's computed envelope, and `call_account_tool`, which forwards the account
  tool's own content blocks (text, images). Write tools return a one-line
  confirmation, the same text the CLI prints.
- A failure comes back as an MCP `isError` result whose text is the CLI's
  one-line JSON error (`type`, `status`, `message`, `body`, `rate_limit`),
  so an agent can tell a bad argument (`api`, 4xx) from a credentials
  problem (`auth`, `config`) from a transient one (`rate_limited`,
  `server`) and act accordingly. Secrets never appear in tool results.
- Interactive confirmation is always off in MCP mode, whatever the profile
  says: stdin is the protocol channel, there is nobody to answer a prompt.
  Safety comes from the API key's permissions and from the MCP client's own
  tool approval, guided by the `readOnlyHint` and `destructiveHint`
  annotations each tool carries.
- `appcues --dry-run mcp` serves a preview-only server: every write tool
  returns the request it would have sent and sends nothing. Handy for
  trying an agent against a production profile.
- The job-polling tools (`wait_for_export_job`, `download_export_job`)
  default to a 120 second timeout because MCP clients time out long calls;
  pass `timeout_secs` or call again. `download_screenshots` and
  `download_export_job` write files inside the server's working directory
  (the MCP client's, usually) and return the path; an `out_path` that is
  absolute, contains `..`, or passes through a symlink leaving that
  directory is rejected.
- stdout carries only protocol messages; the rate-limit and truncation
  lines the CLI emits go to stderr as before, where MCP clients log them.

### Global flags

These work before or after the subcommand:

| Flag | Description |
|---|---|
| `--profile <PROFILE>` | Config profile to use (env: `APPCUES_PROFILE`, default: `default`) |
| `--account <ACCOUNT>` | Override the profile's account ID |
| `-o, --output <table\|json>` | Output format (default: `table`) |
| `--dry-run` | Print write requests instead of sending them |
| `-i, --interactive` | Prompt y/N before destructive commands (default: run without asking) |

Use `-o json` for scripting: `list` and `get` commands emit structured JSON
on success, so you can pipe output to `jq` or another tool instead of
parsing table text. Mutation commands (`publish`/`unpublish`, `segments
delete`/`add-users`/`remove-users`, `users delete`/`track`, `groups
add-users`, `status`) print a confirmation message regardless of
`-o`, since there's no structured result to return. For `list` commands,
`-o json` prints the bare array — if the API response wraps it in an
object, the CLI unwraps it for you.

```bash
appcues flows list -o json | jq '.[] | select(.published) | .id'
```

## Configuration

Credentials live in `~/.config/appcues/config.toml`, one profile per
section:

```toml
[profiles.default]            # prod: nothing else to specify
api_key = "test-key"
api_secret = "test-secret"
account_id = "12345"

[profiles.eu]
api_key = "test-key-eu"
api_secret = "test-secret-eu"
account_id = "67890"
env = "prod-eu"

[profiles.local]              # any explicit URL wins over env
api_key = "test-key"
api_secret = "test-secret"
account_id = "13579"
base_url = "http://localhost:4000"
```

`appcues profiles add eu` writes to a named profile; the config file
is written with `0600` permissions. Change a saved profile with `appcues
profiles edit eu --account-id 99999` (omitted flags keep their
value), and delete one with `appcues profiles remove eu`. Select a
profile per-invocation with `--profile eu` or the `APPCUES_PROFILE`
env var.

### Environment variable overrides

Any of these override the active profile's file values, field by field —
handy for CI, where you don't want a config file on disk at all:

| Variable | Overrides |
|---|---|
| `APPCUES_API_KEY` | `api_key` |
| `APPCUES_API_SECRET` | `api_secret` |
| `APPCUES_ACCOUNT_ID` | `account_id` |
| `APPCUES_ENV` | `env` (named environment, see below) |
| `APPCUES_BASE_URL` | `base_url` (explicit API origin) |
| `APPCUES_TOOLS_BASE_URL` | `tools_base_url` (explicit tools origin) |
| `APPCUES_INTERACTIVE` | `interactive` (y/N prompts before destructive commands) |
| `APPCUES_PROFILE` | which profile to load |

If the credential variables are set, no config file is needed at all:

```bash
export APPCUES_API_KEY=test-key
export APPCUES_API_SECRET=test-secret
export APPCUES_ACCOUNT_ID=12345
appcues flows list -o json
```

### Environments

The API origin resolves as: `APPCUES_BASE_URL` env var > the profile's
`base_url` > `APPCUES_ENV` env var > the profile's named `env` > `prod`.
The named environments:

| `env` | API URL | Tools URL |
|---|---|---|
| `prod` (default) | `https://api.appcues.com` | `https://mcp.appcues.com` |
| `prod-eu` | `https://api.eu.appcues.com` | `https://mcp.eu.appcues.com` |

Set the environment per profile at `profiles add` time (`--env prod-eu`), or
per invocation with `APPCUES_ENV=prod-eu`. `base_url` and `tools_base_url`
are the escape hatch for anything the CLI doesn't know by name (a local api,
a proxy, a non-production stack). Note that credentials are per environment:
a US key will not work against the EU API.
`appcues status` prints the URL it verified against, so you can always
see where a profile points.

## Reliability

Requests that hit a `429` or `5xx` are retried with backoff automatically.

## Interactive confirmation

Destructive commands (`delete`, `remove`, non-read-only `tools call`) run
without asking by default: the CLI is built for agents and scripts, and
safety is delegated to the API key's permissions. Humans who want a y/N
prompt before those commands opt in, in one of three ways:

- per invocation: `appcues -i segments delete s1`
- per profile: `interactive = true` under the profile in
  `~/.config/appcues/config.toml` (set by hand; new profiles start
  non-interactive)
- per environment: `APPCUES_INTERACTIVE=true` (overrides the profile
  field; `APPCUES_INTERACTIVE=false` forces a profile's `true` off)

The `-i` flag turns prompting on for that invocation and wins over the
profile. Interactive mode requires a terminal: with prompting on and
stdin not a tty, the command refuses with a config error (exit 3) instead
of hanging, since there is nobody to answer.

## Exit codes and errors

Failures print exactly one JSON line to stderr and exit with a typed code,
so scripts and agents can branch without parsing prose:

```json
{"error":true,"type":"api","status":404,"message":"API error 404: flow not found","exit_code":4}
```

| Exit code | `type` | Meaning |
|---|---|---|
| 0 | — | success |
| 1 | `unexpected` | network/IO/other unexpected failure |
| 2 | — | command-line usage error (from clap, prose on stderr) |
| 3 | `config`, `auth` | missing/invalid credentials or profile; HTTP 401/403 |
| 4 | `api`, `tool` | the API rejected the request (other 4xx), or a tool ran and reported failure (`isError`) |
| 5 | `rate_limited`, `server` | 429 or 5xx still failing after retries |

## Roadmap

This is the v1 command set. Planned next, roughly in order: the remaining
`jobs` subcommand (`list`), bulk import/export, ingestion rules, and SDK
key management.

## Distribution roadmap

Planned install channels beyond the release tarballs:

- **Installers** via [cargo-dist](https://opensource.axo.dev/cargo-dist/):
  shell and Homebrew installers plus a Windows target.
- **Homebrew tap**: `brew install appcues/tap/appcues`.
- **npm shim**: `npm install -g @appcues/cli`, a thin package that
  downloads the platform binary, for agents and JS toolchains that
  already have `npx`.

## Development

The crate lives in `crates/appcues/`, and `scripts/dev` runs cargo from
there so you can work from the repo root:

```bash
scripts/dev test
scripts/dev build
scripts/dev install
scripts/dev run flows list
```

`test` is the CI gate (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`). `build` produces `crates/appcues/target/release/appcues`;
`install` puts the binary on your PATH via `cargo install`.

`docs/best-practices.md` collects this repo's engineering practices; follow
it while building and as a self-review pass before opening a PR.

See `CHANGELOG.md` for release history.
