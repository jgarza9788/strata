#!/usr/bin/env bash
# Runs the most recently built Strata binary. Usage: ./run.sh [strata args...]
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

latest=
for candidate in target/release/strata target/debug/strata; do
  if [[ -x $candidate && ( -z $latest || $candidate -nt $latest ) ]]; then
    latest=$candidate
  fi
done

if [[ -z $latest ]]; then
  echo "No build found. Run ./build.sh first." >&2
  exit 1
fi

echo "Running $latest" >&2
exec "$latest" "$@"
