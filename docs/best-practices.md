# Best practices

Engineering practices for this repo, distilled from real review findings
(started from PR #3). Follow them while building; they double as a
self-review pass before opening a PR. Add an item only when a review
actually caught the mistake; delete items that stop earning their place.

## Timeouts and long-running work

- **Know what each timeout covers.** ureq's `timeout_global` spans the
  whole request *including body streaming* — a client-wide cap silently
  aborts large healthy downloads. Override per-request when a call's
  duration is legitimately unbounded (`.config().timeout_global(None)`).
- **Never trade a cap for an unbounded hang.** Removing a timeout needs a
  replacement stall guard. ureq has no idle timeout, so pair
  `timeout_global(None)` with a generous `timeout_recv_body` cap.
- **Deadline loops: cap the sleep to the time remaining.** A
  sleep-then-check loop overshoots its deadline by up to one full sleep
  (`sleep(gap.min(deadline.saturating_sub(started.elapsed())))`).

## Error taxonomy (typed exit codes)

- **Every new error path must land in the right exit-code bucket.** A bare
  `?` or `bail!` on a config-shaped failure classifies as exit 1
  "unexpected" instead of 3. When adding a fallible path, ask: which type
  should `classify()` see — `ConfigError`, `ApiError`, or genuinely
  unexpected? Add a downcast test for the new path.
- **Sibling error paths should give symmetric guidance.** If the timeout
  error points at `appcues jobs get`, the FAILED error should too.

## Cross-cutting flags

- **`--dry-run` covers every write, not just API writes.** `profiles add`
  writes a file and needed the guard too. When adding any command that
  mutates *anything* (API, filesystem, container state), wire the guard
  in before `confirm()` and add a zero-HTTP-hits test.
- A convention added "on everything" deserves a grep for the paths that
  don't go through the common seam.

## Secrets and untrusted hosts

- **Never send Authorization to presigned/third-party URLs.** The download
  path deliberately omits the auth header — keep that property and say so
  in a comment.
- **Presigned URLs are themselves secret-ish** (signature in the query
  string); avoid echoing them whole into error messages.
- **Never make an agent relay a credential-bearing value through its own
  transcript.** Agent runtimes mask credential-like strings (AWS key ids,
  API keys) in displayed output, so a presigned URL an agent copies back
  out of what it saw is corrupted. Put the fetch inside the tool instead
  (`jobs download` exists for exactly this reason — the URL never
  appears in any output).
- Never read or print `.env` contents — move/mount/chmod only.

## Streaming vs buffering

- `read_to_string` in ureq caps at 10MB. Anything user-sized (exports,
  downloads) streams via `body_mut().as_reader()` + `io::copy`.

## Docs

- **READMEs must work on a fresh clone.** If a step needs a gitignored
  file (a `.env`), the bootstrap that creates it comes *before* the
  first command that requires it.
- Docs change in the same PR as the behavior they describe (README
  exit-code table, CHANGELOG entry).
- **`--help` text is an agent routing surface.** Agents explore a CLI via
  `--help` before (or instead of) loading any skill, so subcommand
  one-liners should steer toward the intended path and away from traps
  (observed: "Export the raw events" routed an agent to the legacy
  export command purely on its help line).

## Environment facts to verify, not assume

- Verify API behavior against the public docs before building around it
  (the export API is async-only; list endpoints don't paginate — an
  "auto-pagination" feature would have been a no-op).
- Docker Desktop (virtiofs) rejects a single-file bind mount nested
  inside an already-mounted directory — use compose `env_file:` instead.
- `rust-version = 1.88`, the MCP SDK's floor; keep it there unless a
  dependency forces a bump.

## Gate

The pass/fail bar every change must clear before it's committed — same
checks CI runs. From `crates/appcues/`: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test` — all clean,
test output pristine (warnings in test output are findings).
