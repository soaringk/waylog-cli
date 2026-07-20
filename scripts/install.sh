#!/bin/sh
# Install a pre-built WayLog binary from GitHub Releases.

set -eu

VERSION="${WAYLOG_VERSION:-latest}"
INSTALL_DIR="${WAYLOG_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
    Darwin) os="apple-darwin" ;;
    Linux) os="unknown-linux-musl" ;;
    *)
        echo "Unsupported operating system: $(uname -s)" >&2
        echo "Windows users should run scripts/install.ps1." >&2
        exit 1
        ;;
esac

case "$(uname -m)" in
    arm64|aarch64) arch="aarch64" ;;
    x86_64|amd64) arch="x86_64" ;;
    *)
        echo "Unsupported architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

archive="waylog-${arch}-${os}.tar.gz"
if [ "$VERSION" = "latest" ]; then
    base_url="https://github.com/soaringk/waylog-cli/releases/latest/download"
else
    case "$VERSION" in
        v*) tag="$VERSION" ;;
        *) tag="v$VERSION" ;;
    esac
    base_url="https://github.com/soaringk/waylog-cli/releases/download/${tag}"
fi

for command_name in curl tar; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "Required command not found: $command_name" >&2
        exit 1
    fi
done

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

echo "Downloading ${archive}..."
curl -fsSL "${base_url}/${archive}" -o "${tmp_dir}/${archive}"
curl -fsSL "${base_url}/${archive}.sha256" -o "${tmp_dir}/${archive}.sha256"

expected="$(tr -d '[:space:]' < "${tmp_dir}/${archive}.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "${tmp_dir}/${archive}" | cut -d ' ' -f 1)"
elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "${tmp_dir}/${archive}" | cut -d ' ' -f 1)"
else
    echo "A SHA-256 tool (sha256sum or shasum) is required." >&2
    exit 1
fi

if [ "$expected" != "$actual" ]; then
    echo "Checksum verification failed for ${archive}." >&2
    exit 1
fi

tar -xzf "${tmp_dir}/${archive}" -C "$tmp_dir"
mkdir -p "$INSTALL_DIR"
install -m 0755 "${tmp_dir}/waylog" "${INSTALL_DIR}/waylog"

echo "Installed waylog to ${INSTALL_DIR}/waylog"
"${INSTALL_DIR}/waylog" --version

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "Add ${INSTALL_DIR} to PATH to run waylog directly." ;;
esac
