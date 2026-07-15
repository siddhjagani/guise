#!/usr/bin/env bash
# guise installer - builds the release binary and installs it to a bin dir.
#
# Usage:
#   ./install.sh                 # build + install to ~/.local/bin (or /usr/local/bin)
#   PREFIX=/usr/local ./install.sh
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_NAME="guise"

# Pick an install dir: honor $PREFIX/bin, else ~/.local/bin, else /usr/local/bin.
if [[ -n "${PREFIX:-}" ]]; then
  INSTALL_DIR="${PREFIX}/bin"
elif [[ -d "${HOME}/.local/bin" ]]; then
  INSTALL_DIR="${HOME}/.local/bin"
else
  INSTALL_DIR="/usr/local/bin"
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "guise currently supports macOS only." >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo (the Rust toolchain) is required. Install from https://rustup.rs" >&2
  exit 1
fi

echo "Building ${BIN_NAME} (release)..."
cargo build --release --manifest-path "${REPO_DIR}/Cargo.toml"

echo "Installing to ${INSTALL_DIR} ..."
mkdir -p "${INSTALL_DIR}"
install -m 0755 "${REPO_DIR}/target/release/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"

echo "Installed ${INSTALL_DIR}/${BIN_NAME}"
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) : ;;
  *) echo "Note: add ${INSTALL_DIR} to your PATH to run '${BIN_NAME}' directly." ;;
esac
echo "Run '${BIN_NAME} doctor' to verify your environment."
