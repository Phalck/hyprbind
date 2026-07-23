#!/usr/bin/env bash
# Build hyprbind from source and install it as `hyprbind` on your PATH.
#
# Usage:
#   ./install.sh                                   (from a clone of this repo)
#   curl -fsSL https://raw.githubusercontent.com/Phalck/hyprbind/master/install.sh | sh
#
# Override the install location with HYPRBIND_INSTALL_DIR (default: ~/.local/bin).
set -euo pipefail

REPO_URL="https://github.com/Phalck/hyprbind.git"
BIN_NAME="hyprbind"
INSTALL_DIR="${HYPRBIND_INSTALL_DIR:-$HOME/.local/bin}"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo (Rust) is required to build hyprbind." >&2
    echo "Install Rust first: https://rustup.rs" >&2
    exit 1
fi

# Build in the current checkout if this script is being run from inside the hyprbind repo
# (e.g. `./install.sh` after cloning); otherwise clone a fresh copy to a temp directory first,
# so the curl-pipe one-liner above works too.
CLEANUP=""
if [ -f "Cargo.toml" ] && grep -q '^name = "hyprbind"' Cargo.toml 2>/dev/null; then
    SRC_DIR="$(pwd)"
else
    if ! command -v git >/dev/null 2>&1; then
        echo "error: git is required to fetch hyprbind's source." >&2
        exit 1
    fi
    SRC_DIR="$(mktemp -d)"
    CLEANUP="$SRC_DIR"
    trap '[ -n "$CLEANUP" ] && rm -rf "$CLEANUP"' EXIT
    echo "Cloning hyprbind..."
    git clone --depth 1 "$REPO_URL" "$SRC_DIR"
fi

echo "Building hyprbind (release)..."
(cd "$SRC_DIR" && cargo build --release)

mkdir -p "$INSTALL_DIR"
install -m 755 "$SRC_DIR/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

echo "Installed to $INSTALL_DIR/$BIN_NAME"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo
        echo "warning: $INSTALL_DIR is not on your PATH."
        echo "Add this to your shell config, then restart your shell:"
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac
