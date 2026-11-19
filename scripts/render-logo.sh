#!/usr/bin/env bash
# Regenerates every logo asset from one square-ish transparent PNG.
# Usage: scripts/render-logo.sh [SOURCE]   (default: assets/logo/veloce.png)
set -euo pipefail

cd "$(dirname "$0")/.."
source="${1:-assets/logo/veloce.png}"
logo_dir=assets/logo
master="$logo_dir/veloce.png"
icns=assets/macos/AppIcon.icns
# App icon tile, matching the welcome screen backdrop.
tile_top="#1E222A"
tile_bottom="#0B0D11"
tile_glow="rgba(40,200,170,0.30)"
tile_rim="rgba(255,255,255,0.14)"
logo_in_tile=676

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
cp "$master" assets/macos/AppIcon.icon/Assets/veloce.png

resize() {
    magick "$master" -filter Lanczos -resize "$2x$2" -strip "$1"
}

for size in 1024 512 256 128 64 32 16; do
    resize "$logo_dir/veloce-$size.png" "$size"
done

# Legacy AppIcon.icns, used only when the bundle is built without Xcode's actool. With actool the
# app icon comes from assets/macos/AppIcon.icon, whose logo layer is refreshed above.
squircle() {
    awk 'BEGIN {
        n = 4; a = 412; c = 512; pi = atan2(0, -1)
        for (i = 0; i < 720; i++) {
            t = 2 * pi * i / 720; ct = cos(t); st = sin(t)
            x = c + a * (ct < 0 ? -1 : 1) * (ct < 0 ? -ct : ct) ^ (2 / n)
            y = c + a * (st < 0 ? -1 : 1) * (st < 0 ? -st : st) ^ (2 / n)
            printf "%.2f,%.2f ", x, y
        }
    }'
}

render_tile() {
    local outline
    outline="$(squircle)"
    magick -size 1024x1024 xc:none -fill white -draw "polygon $outline" "$work/mask.png"
    magick -size 1024x1024 gradient:"$tile_top"-"$tile_bottom" \
        \( -size 1024x1024 radial-gradient:"$tile_glow"-none \) -compose over -composite \
        "$work/mask.png" -alpha off -compose CopyOpacity -composite "$work/tile.png"
    magick -size 1024x1024 xc:none -fill none -stroke "$tile_rim" -strokewidth 3 \
        -draw "polygon $outline" "$work/mask.png" -compose DstIn -composite "$work/rim.png"
    magick "$work/mask.png" -fill black -colorize 100 -channel A -evaluate multiply 0.35 \
        +channel -blur 0x14 "$work/shadow.png"
    magick -size 1024x1024 xc:none "$work/shadow.png" -geometry +0+10 -compose over -composite \
        "$work/tile.png" -geometry +0+0 -composite "$work/rim.png" -composite \
        \( "$master" -filter Lanczos -resize "${logo_in_tile}x${logo_in_tile}" \) \
        -gravity center -composite -strip "$logo_dir/veloce-app-icon.png"
}

render_tile
if command -v iconutil >/dev/null; then
    iconset="$work/AppIcon.iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        for scale in 1 2; do
            pixels=$((size * scale))
            name="icon_${size}x${size}"
            [[ $scale == 2 ]] && name="${name}@2x"
            magick "$logo_dir/veloce-app-icon.png" -filter Lanczos -resize "${pixels}x${pixels}" \
                -strip "$iconset/$name.png"
        done
    done
    iconutil -c icns "$iconset" -o "$icns"
    echo "Wrote $icns"
fi

echo "Wrote $master (${side}x${side}), sizes 16 to 1024 and veloce-app-icon.png in $logo_dir"
