#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 3 || ( $3 != native && $3 != sanitized ) ]]; then
  exit 2
fi
prepared=$(realpath "$1")
output=$2
mode=$3
mkdir -- "$output"
output=$(realpath "$output")
cd -- "$prepared"
sha256sum --check source-files.sha256
flags=(-std=gnu99 -D_USE_MATH_DEFINES -I"$prepared/btt")
if [[ $mode == sanitized ]]; then
  flags+=(-O1 -g -fno-omit-frame-pointer -fsanitize=address,undefined -fno-sanitize-recover=all)
else
  flags+=(-O2)
fi
compiler=${CC:-clang}
"$compiler" --version > "$output/compiler.txt"
set -x
"$compiler" "${flags[@]}" -Wall -Wextra -Werror -c "$prepared/frozen/driver.c" -o "$output/driver.o"
objects=("$output/driver.o")
for source in BTT DFT STFT Filter Statistics fastsin; do
  "$compiler" "${flags[@]}" -c "$prepared/btt/src/$source.c" -o "$output/$source.o"
  objects+=("$output/$source.o")
done
"$compiler" "${flags[@]}" "${objects[@]}" -lm -o "$output/native-tempo-check"
set +x
sha256sum --check source-files.sha256
sha256sum "$output/native-tempo-check" > "$output/binary.sha256"
sha256sum "$prepared/source-files.sha256" > "$output/source-manifest.sha256"
