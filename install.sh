#!/usr/bin/env bash
# guise installer.
#
#   curl -fsSL https://raw.githubusercontent.com/siddhjagani/guise/main/install.sh | bash
#
# Works two ways:
#   * piped from curl (no clone)      -> downloads a prebuilt binary from the
#                                        latest GitHub release
#   * run inside a cloned checkout    -> builds from source with cargo
#
# Env overrides:
#   PREFIX=/usr/local     install to $PREFIX/bin
#   GUISE_VERSION=v1.2.3  install a specific release tag (default: latest)
set -euo pipefail

REPO="siddhjagani/guise"
BIN_NAME="guise"
ASSET="guise-macos.tar.gz"

# --- pick an install dir (no sudo needed by default) -----------------------
if [[ -n "${PREFIX:-}" ]]; then
  INSTALL_DIR="$PREFIX/bin"
elif [[ -w "/usr/local/bin" ]]; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="$HOME/.local/bin"
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "guise currently supports macOS only." >&2
  exit 1
fi

# BASH_SOURCE is unset when piped through curl; guard it under `set -u`.
SCRIPT_SRC="${BASH_SOURCE[0]:-}"
REPO_DIR=""
if [[ -n "$SCRIPT_SRC" ]]; then
  REPO_DIR="$(cd "$(dirname "$SCRIPT_SRC")" && pwd)"
fi

mkdir -p "$INSTALL_DIR"

install_from_source() {
  echo "Building $BIN_NAME from source (release)…"
  cargo build --release --manifest-path "$REPO_DIR/Cargo.toml"
  install -m 0755 "$REPO_DIR/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
}

install_prebuilt() {
  local ver url
  ver="${GUISE_VERSION:-latest}"
  if [[ "$ver" == "latest" ]]; then
    url="https://github.com/$REPO/releases/latest/download/$ASSET"
  else
    url="https://github.com/$REPO/releases/download/$ver/$ASSET"
  fi
  # `tmp` is intentionally global (not `local`) so the EXIT trap below can
  # still see it after this function returns; `${tmp:-}` keeps `set -u` happy.
  tmp="$(mktemp -d)"
  trap 'rm -rf "${tmp:-}"' EXIT
  echo "Downloading $BIN_NAME ($ver)…"
  if ! curl -fsSL "$url" -o "$tmp/$ASSET"; then
    echo "error: could not download $url" >&2
    echo "       (no release yet? install from source: git clone https://github.com/$REPO && cd guise && ./install.sh)" >&2
    exit 1
  fi
  tar -xzf "$tmp/$ASSET" -C "$tmp"
  install -m 0755 "$tmp/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
}

if [[ -n "$REPO_DIR" && -f "$REPO_DIR/Cargo.toml" ]] && command -v cargo >/dev/null 2>&1; then
  install_from_source
else
  install_prebuilt
fi

echo "✓ Installed $INSTALL_DIR/$BIN_NAME"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) : ;;
  *)
    echo "Note: $INSTALL_DIR is not on your PATH. Add this to your shell profile:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac
echo "Run '$BIN_NAME doctor' to verify your setup, then '$BIN_NAME add <name>'."
