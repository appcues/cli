# OpenClaw (local)

Runs [OpenClaw](https://openclaw.ai) as a local container — a sandbox for
developing and testing the Appcues skills, fast and safely. The container is
not part of the product: real users run their own OpenClaw; the `appcues`
CLI installs natively anywhere (binary on PATH + keys).

## Start

The gateway refuses to start unconfigured, so use the sandbox script — it
bootstraps `data/openclaw.json` (gateway token + default model
`anthropic/claude-sonnet-5`), reports which secrets are still missing from
`../.env` (`runtimes/.env`, gitignored, loaded via compose's `env_file:` —
needs `ANTHROPIC_API_KEY` and the `APPCUES_*` credentials), and starts the
container:

```bash
../sandbox.sh up openclaw
```

To change the model later:
`docker compose exec openclaw node dist/index.js models set <model>`
then `docker compose restart` (the model is read at gateway startup).

Control UI: `localhost:18789` (paste the token from `data/openclaw.json`),
or run `../sandbox.sh open openclaw` to open it pre-authenticated.

## Credentials

All keys live in the shared `../.env` created above. To change or add one,
edit the file and recreate the container:

```bash
docker compose up -d   # recreate to pick up the change
```

> `runtimes/.env` is one file shared by every runtime, loaded via each
> service's `env_file:` entry. Edits take effect on the **next**
> `docker compose up -d` (container recreate) — not live, since it's read
> into the environment at container start, not watched on disk.

## Giving the container the `appcues` CLI

One script builds a static musl binary in a throwaway Rust container and
installs it into every runtime sandbox (here: `data/bin/`, which compose
puts on the container's PATH) — re-run it after any CLI change, or pass
`--release` to download the latest GitHub release instead:

```bash
../install-cli.sh
docker compose exec openclaw appcues --version   # verify
```

## Skills

`../sandbox.sh up` clones the [appcues/skills](https://github.com/appcues/skills) repo
into the gitignored `../skills/` on first run, and compose mounts its
`skills/` directory read-only as the `appcues` group under OpenClaw's managed
skills root. Edits in that checkout are live. Verify with:

```bash
docker compose exec openclaw node dist/index.js skills list
```

## What persists when the container dies

Everything lives in the `./data/` bind mount; the container is disposable
(`docker compose down && up` returns the same agent).

| What | Where it lives | Survives recreate? |
|---|---|---|
| Config + gateway token (`openclaw.json`) | `data/` | yes |
| Keys (`../.env`, loaded via `env_file:`) | `../.env` (shared with other runtimes) | yes |
| Sessions, memory, workspace | `data/` | yes |
| The `appcues` binary | `data/bin/` | yes |
| The skills (mounted read-only) | `../skills/`, a clone of appcues/skills | yes |
| Anything else in the container fs | container layer | no |

`data/` is gitignored and only on this machine — persistent, not backed up.
