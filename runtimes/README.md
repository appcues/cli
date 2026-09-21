# Runtimes

A runtime is the agent product a skill runs inside — Claude Code, OpenClaw,
Hermes, and so on. Each `runtimes/<name>/` directory answers exactly one
question: **how do I boot this runtime and point it at the Appcues
skills?** That means compose files, the runtime's own config format, its cron
syntax — and nothing else.

These are local dev sandboxes for developing and testing skills quickly and
safely — not something end users install. Real users run their own runtimes;
they only need the `appcues` CLI, their credentials, and the skills.

`./sandbox.sh` is the day-to-day entry point — it sequences everything
below and takes an optional runtime name (omitted = all):

```bash
./sandbox.sh up [openclaw|hermes]    # bootstrap + start + install CLI if missing
./sandbox.sh down [...]              # stop; state in data/ survives
./sandbox.sh status [...]            # container + model per runtime
./sandbox.sh reset [...]             # wipe data/ and rebuild from scratch (asks first)
./sandbox.sh open [...]              # open dashboard(s) pre-authenticated
```

`up` also bootstraps the gitignored config every container needs — gateway
token, dashboard auth, the standard model (`claude-sonnet-5`) — and tells
you which secrets to add to `.env` by hand. Idempotent; existing files and
values are never touched.

`./install-cli.sh` builds the CLI from this checkout (static musl linux
binary, no local Rust toolchain needed) and installs it into every sandbox;
re-run it after any CLI change. `--release` downloads the latest GitHub
release instead of building.

Nothing Appcues-specific belongs here: no commands, no analytics vocabulary,
no recipes, no workflow logic. All of that lives in the `appcues/skills`
repo and must work on every runtime.

The test: delete `runtimes/` entirely and the skills should still describe
complete work. If deleting a file here would lose knowledge about Appcues
rather than about booting a runtime, that file is in the wrong place.

This directory staying nearly empty is the point. The premise of the repo is
that a `SKILL.md` is portable across runtimes; every line that has to be
written per-runtime is that premise leaking. Treat the size of this directory
as a signal to watch, not as normal growth.
