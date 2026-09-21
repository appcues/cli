#!/bin/sh
# One entry point for the local runtime sandboxes: bootstraps the
# gitignored config each container needs, clones the skills repo the
# compose files mount, then sequences install-cli.sh and the compose files.
#
#   ./sandbox.sh up [runtime]      bootstrap + start + install CLI if missing
#   ./sandbox.sh down [runtime]    stop (state in data/ is kept)
#   ./sandbox.sh status [runtime]  container + model per runtime
#   ./sandbox.sh reset [runtime]   wipe data/ and rebuild from scratch (asks)
#   ./sandbox.sh open [runtime]    open dashboard(s) pre-authenticated
#
# [runtime] is openclaw or hermes; omitted = all.
set -eu
cd "$(dirname "$0")"

usage() {
  echo "usage: sandbox.sh up|down|status|reset|open [openclaw|hermes]" >&2
  exit 1
}

verb=${1-}
sel=${2-}
runtimes="openclaw hermes"
if [ -n "$sel" ]; then
  case " $runtimes " in
  *" $sel "*) runtimes=$sel ;;
  *) echo "unknown runtime: $sel" >&2 && usage ;;
  esac
fi

dc() { d=$1 && shift && docker compose -f "$d/docker-compose.yml" "$@"; }

MODEL=claude-sonnet-5 # the standard model for every local runtime

# Idempotent: creates the gitignored config each container needs before its
# first start; existing files and .env values are never touched.
bootstrap() {
  touch .env
  need() { grep -q "^$1=" .env || { printf '%s=%s\n' "$1" "$2" >>.env; echo "added $1 to .env"; }; }
  need HERMES_DASHBOARD_BASIC_AUTH_USERNAME "$(whoami)"
  need HERMES_DASHBOARD_BASIC_AUTH_PASSWORD "$(openssl rand -base64 12)"
  need HERMES_DASHBOARD_BASIC_AUTH_SECRET "$(openssl rand -hex 32)"
  for k in ANTHROPIC_API_KEY APPCUES_API_KEY APPCUES_API_SECRET APPCUES_ACCOUNT_ID APPCUES_ENV; do
    grep -q "^$k=" .env || echo "MISSING in runtimes/.env: $k — add it by hand"
  done

  # openclaw: gateway token + default model
  if [ ! -f openclaw/data/openclaw.json ]; then
    mkdir -p openclaw/data
    cat >openclaw/data/openclaw.json <<CONF
{
  "gateway": {
    "mode": "local",
    "auth": { "mode": "token", "token": "$(openssl rand -hex 32)" }
  },
  "agents": {
    "defaults": {
      "model": { "primary": "anthropic/$MODEL" },
      "models": { "anthropic/$MODEL": {} }
    }
  }
}
CONF
    echo "wrote openclaw/data/openclaw.json"
  fi

  # hermes: default model (hermes fills the rest of config.yaml from its own
  # defaults; env HERMES_MODEL does NOT override this file)
  if [ ! -f hermes/data/config.yaml ]; then
    mkdir -p hermes/data
    printf 'model:\n  default: %s\n  provider: anthropic\n' "$MODEL" >hermes/data/config.yaml
    echo "wrote hermes/data/config.yaml"
  fi

  # skills: the compose files bind-mount ./skills/skills (gitignored) from a
  # checkout of appcues/skills. Must exist before compose runs, or Docker
  # creates an empty directory in its place. Edit or pull it like any clone.
  if [ ! -d skills/.git ]; then
    git clone https://github.com/appcues/skills skills
    echo "cloned appcues/skills into runtimes/skills"
  fi
}

cli_missing() {
  for rt in $runtimes; do
    case $rt in
    openclaw) [ -x openclaw/data/bin/appcues ] || return 0 ;;
    hermes) [ -x hermes/data/.local/bin/appcues ] || return 0 ;;
    esac
  done
  return 1
}

case $verb in
up)
  bootstrap
  for rt in $runtimes; do dc "$rt" up -d; done
  if cli_missing; then ./install-cli.sh; fi
  ;;
down)
  for rt in $runtimes; do dc "$rt" down; done
  ;;
status)
  for rt in $runtimes; do
    echo "== $rt =="
    dc "$rt" ps
    case $rt in
    openclaw) dc openclaw exec openclaw node dist/index.js models status 2>&1 | head -3 || true ;;
    hermes) dc hermes exec hermes hermes status 2>&1 | grep -E '(Model|Provider):' || true ;;
    esac
  done
  ;;
reset)
  printf 'wipe data/ for [%s] — sessions, memories, agent skills all gone. Continue? [y/N] ' "$runtimes"
  read -r ans
  [ "$ans" = y ] || [ "$ans" = Y ] || { echo aborted && exit 1; }
  for rt in $runtimes; do
    dc "$rt" down
    rm -rf "$rt/data"
  done
  bootstrap
  for rt in $runtimes; do dc "$rt" up -d; done
  ./install-cli.sh
  ;;
open)
  # percent-encode the chars openssl base64/usernames can produce
  enc() { printf %s "$1" | sed -e 's/%/%25/g' -e 's|/|%2F|g' -e 's/+/%2B/g' -e 's/=/%3D/g' -e 's/@/%40/g' -e 's/:/%3A/g'; }
  for rt in $runtimes; do
    case $rt in
    openclaw)
      token=$(sed -n 's/.*"token": *"\([^"]*\)".*/\1/p' openclaw/data/openclaw.json 2>/dev/null || true)
      [ -n "$token" ] || { echo "no token in openclaw/data/openclaw.json — run: ./sandbox.sh up openclaw" >&2 && exit 1; }
      echo "http://localhost:18789/#token=$token"
      open "http://localhost:18789/#token=$token"
      ;;
    hermes)
      user=$(sed -n 's/^HERMES_DASHBOARD_BASIC_AUTH_USERNAME=//p' .env | tail -1)
      pass=$(sed -n 's/^HERMES_DASHBOARD_BASIC_AUTH_PASSWORD=//p' .env | tail -1)
      [ -n "$user" ] && [ -n "$pass" ] || { echo "no HERMES_DASHBOARD_BASIC_AUTH_* in .env — run: ./sandbox.sh up hermes" >&2 && exit 1; }
      echo "http://localhost:9119/  (user: $user  pass: $pass)"
      open "http://$(enc "$user"):$(enc "$pass")@localhost:9119/"
      ;;
    esac
  done
  ;;
*) usage ;;
esac
