#!/usr/bin/env bash
# Checks out the Mutable Instruments sources the `$mi.*` modules are ported
# from, at the commits the generated tables and fixtures were made from.
set -euo pipefail

EURORACK_COMMIT=f4cfa7697095a28d292bff758beb51deb49b5a86
STMLIB_COMMIT=d18def816c51d1da0c108236928b2bbd25c17481

here="$(cd "$(dirname "$0")" && pwd)"
src="$here/.src"

checkout() {
  local url=$1 dir=$2 commit=$3
  if [ ! -d "$dir/.git" ]; then
    git init -q "$dir"
    git -C "$dir" remote add origin "$url"
  fi
  if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" != "$commit" ]; then
    git -C "$dir" fetch -q --depth 1 origin "$commit"
    git -C "$dir" checkout -q FETCH_HEAD
  fi
}

checkout https://github.com/pichenettes/eurorack.git "$src/eurorack" "$EURORACK_COMMIT"
checkout https://github.com/pichenettes/stmlib.git "$src/eurorack/stmlib" "$STMLIB_COMMIT"
echo "$src/eurorack"
