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
source_dir="$prepared/audioflux/src"
flags=(-std=c99 -D_DEFAULT_SOURCE -D_USE_MATH_DEFINES -I"$source_dir")
if [[ $mode == sanitized ]]; then
  flags+=(-O1 -g -fno-omit-frame-pointer -fsanitize=address,undefined -fno-sanitize-recover=all)
else
  flags+=(-O2)
fi
compiler=${CC:-clang}
"$compiler" --version > "$output/compiler.txt"
"$compiler" "${flags[@]}" -Wall -Wextra -Werror -c "$prepared/frozen/tools/native-onset-check/driver.c" -o "$output/driver.o"
sources=(stft_algorithm.c dsp/fft_algorithm.c dsp/flux_window.c mir/onset_algorithm.c flux_spectral.c vector/flux_vector.c vector/flux_vectorInt.c vector/flux_vectorOp.c vector/flux_complex.c)
objects=("$output/driver.o")
for source in "${sources[@]}"; do
  object="$output/${source//\//_}.o"
  "$compiler" "${flags[@]}" -Wno-unknown-pragmas -c "$source_dir/$source" -o "$object"
  objects+=("$object")
done
"$compiler" "${flags[@]}" "${objects[@]}" -lm -o "$output/native-onset-check"
sha256sum --check source-files.sha256
sha256sum "$output/native-onset-check" > "$output/binary.sha256"
