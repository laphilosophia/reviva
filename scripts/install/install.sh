#!/usr/bin/env sh
set -eu

REPO="${REVIVA_REPO:-laphilosophia/reviva}"
VERSION="${REVIVA_VERSION:-latest}"
BIN_DIR="${REVIVA_BIN_DIR:-$HOME/.local/bin}"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

need_cmd uname
need_cmd curl
need_cmd tar

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m | tr '[:upper:]' '[:lower:]')"

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  arm64|aarch64) ARCH="aarch64" ;;
esac

ASSET=""
case "$OS" in
  linux)
    if [ "$ARCH" != "x86_64" ]; then
      echo "error: unsupported Linux architecture: $ARCH (supported: x86_64)" >&2
      exit 1
    fi
    ASSET="reviva-linux-x86_64.tar.gz"
    ;;
  darwin)
    if [ "$ARCH" != "aarch64" ]; then
      echo "error: unsupported macOS architecture: $ARCH (supported: aarch64)" >&2
      exit 1
    fi
    ASSET="reviva-macos-aarch64.tar.gz"
    ;;
  *)
    echo "error: unsupported OS: $OS (supported: linux, darwin)" >&2
    exit 1
    ;;
esac

if [ "$VERSION" = "latest" ]; then
  URL="https://github.com/$REPO/releases/latest/download/$ASSET"
else
  URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
fi

TMP_DIR="$(mktemp -d)"
ARCHIVE="$TMP_DIR/$ASSET"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT INT TERM

echo "Downloading $URL"
curl -fL "$URL" -o "$ARCHIVE"

echo "Extracting archive"
tar -xzf "$ARCHIVE" -C "$TMP_DIR"

mkdir -p "$BIN_DIR"
install -m 755 "$TMP_DIR/reviva" "$BIN_DIR/reviva"

echo "Installed: $BIN_DIR/reviva"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    echo "warning: $BIN_DIR is not in PATH"
    echo "Add this line to your shell profile:"
    echo "  export PATH=\"$BIN_DIR:\$PATH\""
    ;;
esac

echo "Run: reviva --help"
