#!/usr/bin/env bash
set -euo pipefail
brand_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
magick "$brand_dir/icons/symbol-source.png" -resize 1024x1024 "$brand_dir/icons/symbol-1024.png"
for size in 16 32 48 64 128 256 512; do
    magick "$brand_dir/icons/symbol-1024.png" -resize "${size}x${size}" "$brand_dir/icons/symbol-${size}.png"
done
magick "$brand_dir/icons/symbol-1024.png" -define icon:auto-resize=256,128,64,48,32,16 "$brand_dir/icons/cocobeat.ico"
magick "$brand_dir/icons/symbol-1024.png" -define icon:auto-resize=48,32,16 "$brand_dir/icons/favicon.ico"
