#!/usr/bin/env bash
# Compiles each reference driver against the pinned Mutable Instruments sources
# and writes its fixture to crates/modular_core/tests/fixtures/mi/.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
src="$("$here/fetch.sh")"
out="$here/../../crates/modular_core/tests/fixtures/mi"
mkdir -p "$here/build" "$out"

for driver in "$here"/drivers/*.cc; do
  name="$(basename "$driver" .cc)"
  c++ -std=c++11 -O2 -ffp-contract=off -DTEST -include initializer_list \
    -I"$src" -o "$here/build/$name" "$driver" "$src/plaits/resources.cc"
  "$here/build/$name" "$out/$name.f32"
done
