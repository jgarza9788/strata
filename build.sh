#!/usr/bin/env bash
# Builds Strata on the host. Usage: ./build.sh [--debug] [--no-rar] [extra cargo args...]
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

profile=(--release)
profile_dir=release
features=()

while (( $# > 0 )); do
  case "$1" in
    --debug) profile=(); profile_dir=debug ;;
    --no-rar) features=(--no-default-features) ;;
    -h|--help)
      sed -n '2p' "$0" | sed 's/^# //'
      exit 0
      ;;
    *) break ;;
  esac
  shift
done

missing=()
command -v cargo >/dev/null 2>&1 || missing+=("cargo (install Rust, or run 'mise install')")
command -v pkg-config >/dev/null 2>&1 || missing+=("pkg-config")
if command -v pkg-config >/dev/null 2>&1; then
  pkg-config --exists fontconfig || missing+=("fontconfig")
  pkg-config --exists 'gtk4 >= 4.12' || missing+=("gtk4 >= 4.12")
  pkg-config --exists gtksourceview-5 || missing+=("gtksourceview5")
  pkg-config --exists poppler-glib || missing+=("poppler-glib")
fi
if (( ${#missing[@]} > 0 )); then
  printf 'Missing build dependencies:\n' >&2
  printf '  - %s\n' "${missing[@]}" >&2
  exit 1
fi

cargo build --locked "${profile[@]}" "${features[@]}" "$@"

echo "Built target/${profile_dir}/strata"
