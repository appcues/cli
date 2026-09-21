# Hermes (local)

Runs [Hermes Agent](https://hub.docker.com/r/nousresearch/hermes-agent) as a local container: a sandbox for developing and testing the Appcues skills. The container is not part of the product: the `appcues` CLI itself installs natively anywhere (binary on PATH + keys), and agents with a shell use it directly. To run Hermes natively instead, see the install guide in the skills repo: https://github.com/appcues/skills/blob/main/docs/install.md.

## Start

All configuration (dashboard auth, model keys, Appcues credentials) lives in one file, `../.env` (`runtimes/.env`, gitignored), loaded into the container's process environment via compose's `env_file:`. The sandbox script generates the dashboard auth, seeds `data/config.yaml` with the standard model (`claude-sonnet-5`), reports which secrets you still need to add by hand, and starts the container:

```bash
../sandbox.sh up hermes
```

- Gateway API: `localhost:8642`
- Dashboard: `localhost:9119`, or run `../sandbox.sh open hermes` to open it pre-authenticated

Hermes state lives in `./data/` (gitignored), mounted into the container as `/opt/data`, so each checkout gets its own isolated instance, and multiple checkouts can run side by side.

> `runtimes/.env` is one file shared by every runtime, loaded via each service's `env_file:` entry. Edits take effect on the **next** `docker compose up -d` (container recreate), not live, since it's read into the environment at container start, not watched on disk.

## Model provider keys and model selection

Open `../.env`, set the keys the sandbox script reported missing (`ANTHROPIC_API_KEY` for the model provider), then recreate the container so the change is picked up:

```bash
docker compose up -d
```

`docker exec -it hermes hermes setup` runs an interactive wizard, but it writes a container-local `data/.env` (Hermes prefers that file over process env when present). That is fine for a one-off override on this checkout, but keys meant to apply everywhere belong in the shared `runtimes/.env` instead.

Pick the default model and provider (the runtimes here standardize on `claude-sonnet-5`: set `model.default` in `data/config.yaml`, or use the picker), then verify the key registered:

```bash
docker exec -it hermes hermes model    # interactive picker
docker exec hermes hermes status      # expect Anthropic ✓, Model: claude-sonnet-5
```

## Giving the container the `appcues` CLI

The container has no Rust toolchain, and `data/.local/bin` is on its PATH. One script builds a static musl binary in a throwaway Rust container and installs it into every runtime sandbox. Re-run it after any CLI change:

```bash
../install-cli.sh
docker compose exec --user hermes hermes bash -lc 'appcues --version'  # verify
```

The script also writes `data/.profile` with `export PATH="$HOME/.local/bin:$PATH"`. That file is load-bearing: Hermes' terminal tool builds each session's environment from a login-shell snapshot, and Debian's `/etc/profile` resets PATH, dropping `data/.local/bin`. The snapshot sources `~/.profile` (`$HOME` is `/opt/data`), which puts it back. Without it, the agent's terminal gets `appcues: command not found` even though `docker exec` finds the binary fine. Terminal sessions snapshot at creation, so a fresh session (or ~5 idle minutes, the session lifetime) is needed to pick up changes.

Appcues credentials for the container go in `../.env` (`APPCUES_API_KEY`, `APPCUES_API_SECRET`, `APPCUES_ACCOUNT_ID`, `APPCUES_ENV`). `../install-cli.sh --release` downloads the latest linux release instead of compiling.

## Pointing it at the skills

The skills live in the [appcues/skills](https://github.com/appcues/skills) repo. Real users install them as a plugin; that repo's README has the commands.

For skill development, `../sandbox.sh up` clones that repo into the gitignored `../skills/` on first run, and `docker-compose.yml` mounts its `skills/` directory into the container as the `appcues` category (read-only). `hermes skills list` shows them, and edits in that checkout are live without reinstalling; pull it like any clone to pick up new skills. Start with `../sandbox.sh up hermes` rather than `docker compose up` directly: if the checkout is missing when compose runs, Docker creates an empty directory in its place. A host-side symlink does not work here: the container cannot resolve symlinks to unshared host paths.

Two gotchas when testing skills in the chat:

- `/reload-skills` only detects **added or removed skill directories**; "no new skill detected" after editing a SKILL.md body is normal: the body is read fresh from disk on every skill view, no reload needed. A _new_ skill also needs `/reload-skills` (or a new session) before an open session's `/skills` index shows it.
- Renamed or added a skill directory and the agent still reaches for the old name? The gateway caches the skills index it puts in the system prompt (`data/.skills_prompt_snapshot.json`) at startup, and `/reload-skills` does not rebuild it. `docker compose restart` does; then start a new session.
- The agent can author its own skills into `data/skills/`, and a bad session will fossilize its workarounds there (hardcoded URLs, "known broken" claims about things that work). Those skills compete with the repo's on every prompt. When a test run behaves oddly, check `data/skills/` (or the dashboard's skills page) and prune.

## What persists when the container dies

Everything lives in the `./data/` bind mount, so the container is fully disposable: `docker compose down && up` returns the identical agent.

| What                                                                  | Where it lives                         | Survives recreate? |
| --------------------------------------------------------------------- | -------------------------------------- | ------------------ |
| Sessions, memories, kanban, cron jobs                                 | `data/`                                | yes                |
| Skills the agent installs or writes                                   | `data/skills/`                         | yes                |
| The `appcues` binary                                                  | `data/.local/bin/`                     | yes                |
| Config (`config.yaml`)                                                | `data/`                                | yes                |
| Keys (`../.env`, loaded via `env_file:`)                              | `../.env` (shared with other runtimes) | yes                |
| The skills (mounted read-only)                                        | `../skills/`, a clone of appcues/skills | yes                |
| Anything written elsewhere in the container fs (`/tmp`, apt installs) | container layer                        | no                 |

Two caveats. Persistent is not backed up: `data/` is gitignored and only on this machine (`hermes backup` exists if its contents become worth keeping). And agent-authored skills land in the unversioned `data/skills/`; promoting one means copying it into the `appcues/skills` repo and reviewing it like any contribution.
