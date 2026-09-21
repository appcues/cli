#!/bin/sh
# Build the appcues CLI for linux and install it into every local runtime
# sandbox (hermes, openclaw). Re-run after any CLI change to update them.
#
#   ./install-cli.sh            build from this checkout (default)
#   ./install-cli.sh --release  download the latest GitHub release instead
set -e

ROOT=$(cd "$(dirname "$0")/.." && pwd)

if [ "${1:-}" = "--release" ]; then
    # Linux glibc tarball for the docker platform's arch (both sandbox
    # images are Debian-based). Needs the GitHub CLI, like the README's
    # install steps.
    case $(docker info --format '{{.Architecture}}') in
    aarch64 | arm64) target=aarch64-unknown-linux-gnu ;;
    x86_64 | amd64) target=x86_64-unknown-linux-gnu ;;
    *) echo "error: unsupported docker arch" >&2 && exit 2 ;;
    esac
    DL="$ROOT/crates/appcues/target/linux/release"
    mkdir -p "$DL"
    echo "==> downloading latest appcues-$target release"
    gh release download -R appcues/cli -p "appcues-$target.tar.xz" -D "$DL" --clobber
    tar -xJf "$DL/appcues-$target.tar.xz" -C "$DL" --strip-components=1 '*/appcues'
else
    # Static musl build in a throwaway container (runs on any linux container,
    # glibc or alpine; matches the docker platform's arch). CARGO_TARGET_DIR
    # keeps linux artifacts out of the host build's target/release; the named
    # volume caches the crate registry across runs.
    echo "==> building linux binary in a rust:alpine container (first run pulls"
    echo "    the image and downloads crates — later runs are much faster)"
    docker run --rm \
        -v "$ROOT":/w -w /w/crates/appcues \
        -v appcues-cli-cargo-registry:/usr/local/cargo/registry \
        -e CARGO_TARGET_DIR=/w/crates/appcues/target/linux \
        rust:alpine sh -c 'apk add -q musl-dev && cargo build --release'
fi

BIN="$ROOT/crates/appcues/target/linux/release/appcues"

# hermes: data/.local/bin is on the container PATH, but hermes' terminal
# tool builds its env from a login-shell snapshot and Debian's /etc/profile
# resets PATH — ~/.profile ($HOME is /opt/data) is sourced into the
# snapshot and puts it back.
echo "==> installing into hermes (data/.local/bin)"
mkdir -p "$ROOT/runtimes/hermes/data/.local/bin"
cp "$BIN" "$ROOT/runtimes/hermes/data/.local/bin/"
PROFILE="$ROOT/runtimes/hermes/data/.profile"
if ! grep -qs '.local/bin' "$PROFILE"; then
    echo "==> writing $PROFILE (puts .local/bin back on the terminal PATH)"
    echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$PROFILE"
fi

# openclaw: data/bin is on the container PATH via compose.
echo "==> installing into openclaw (data/bin)"
mkdir -p "$ROOT/runtimes/openclaw/data/bin"
cp "$BIN" "$ROOT/runtimes/openclaw/data/bin/"

echo "==> done: $(du -h "$BIN" | cut -f1) binary installed in both sandboxes"
