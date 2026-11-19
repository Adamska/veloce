#!/usr/bin/env bash
# Regenerates every logo asset from one square-ish transparent PNG.
# Usage: scripts/render-logo.sh [SOURCE]   (default: assets/logo/veloce.png)
set -euo pipefail

cd "$(dirname "$0")/.."
source="${1:-assets/logo/veloce.png}"
logo_dir=assets/logo
master="$logo_dir/veloce.png"
icns=assets/macos/AppIcon.icns
# Apple's macOS icon grid draws the artwork in 824 of 1024 px so it lines up with other apps.
icon_body_ratio="824/1024"

command -v magick >/dev/null || { echo "ImageMagick (magick) is required" >&2; exit 1; }
mkdir -p "$logo_dir"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Drop near-invisible alpha noise, trim the transparent margin, then centre on a square canvas.
magick "$source" \( +clone -alpha extract -black-threshold 1% \) -alpha off \
    -compose CopyOpacity -composite -trim +repage "$work/trimmed.png"
side="$(magick "$work/trimmed.png" -format '%[fx:max(w,h)]' info:)"
magick "$work/trimmed.png" -background none -gravity center -extent "${side}x${side}" \
    -strip "$work/master.png"
mv "$work/master.png" "$master"

resize() {
    magick "$master" -filter Lanczos -resize "$2x$2" -strip "$1"
}

for size in 1024 512 256 128 64 32 16; do
    resize "$logo_dir/veloce-$size.png" "$size"
done

if command -v iconutil >/dev/null; then
    iconset="$work/AppIcon.iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        for scale in 1 2; do
            pixels=$((size * scale))
            body=$((pixels * ${icon_body_ratio}))
            name="icon_${size}x${size}"
            [[ $scale == 2 ]] && name="${name}@2x"
            magick "$master" -filter Lanczos -resize "${body}x${body}" -background none \
                -gravity center -extent "${pixels}x${pixels}" -strip "$iconset/$name.png"
        done
    done
    iconutil -c icns "$iconset" -o "$icns"
    echo "Wrote $icns"
fi

echo "Wrote $master (${side}x${side}) and sizes 16 to 1024 in $logo_dir"
