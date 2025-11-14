#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version_file="$repo_root/.qlty-version"

if [[ -f "$version_file" ]]; then
    default_version="$(tr -d '[:space:]' < "$version_file")"
else
    echo "warning: .qlty-version not found; defaulting to latest" >&2
    default_version=latest
fi

if [[ -z "${default_version}" ]]; then
    echo "error: qlty version is empty; update .qlty-version" >&2
    exit 1
fi

QLTY_VERSION="${QLTY_VERSION:-$default_version}"
export QLTY_VERSION

install_dir="${QLTY_INSTALL:-$HOME/.qlty}"
bin_dir="${QLTY_INSTALL_BIN_PATH:-$install_dir/bin}"
qlty_bin="$bin_dir/qlty"

if [[ "$QLTY_VERSION" != "latest" && -x "$qlty_bin" ]]; then
    current_version="$("$qlty_bin" --version | awk '{print $2}')"
    if [[ "$current_version" == "$QLTY_VERSION" ]]; then
        echo "qlty ${current_version} already installed at $qlty_bin"
        exit 0
    fi
fi

echo "Installing qlty ${QLTY_VERSION} via https://qlty.sh ..."
curl -fsSL https://qlty.sh | sh

echo
echo "qlty ${QLTY_VERSION} installed under $bin_dir (set PATH if needed)."
