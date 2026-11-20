#!/usr/bin/env bash
# Regenerates the README screenshots in docs/media and the hop recording, rendered offscreen by
# tests/readme_media.rs from three minutes of the open movie Sintel. The recording is written to
# target/readme-media/veloce-hops.mp4 and hosted as a GitHub attachment, so it stays out of git.
# Needs FFmpeg (with libfreetype for the captions), ImageMagick and a GPU adapter.
set -euo pipefail

cd "$(dirname "$0")/.."
sintel_url="https://ftp.nluug.nl/pub/graphics/blender/demo/movies/Sintel.2010.1080p.mkv"
work="target/readme-media"
clip="$work/Sintel (2010).mkv"
media=docs/media
font=assets/fonts/InterVariable.ttf
clip_width=1600
clip_quality=23

mkdir -p "$work" "$media"
if [[ ! -f "$clip" ]]; then
    echo "Fetching three minutes of Sintel (CC BY 3.0, Blender Foundation)"
    ffmpeg -v error -y -ss 00:01:00 -i "$sintel_url" -t 180 \
        -map 0:v:0 -map 0:a:0 -map 0:s -c copy "$clip"
fi

rm -rf "$work/out"
VELOCE_README_CLIP="$clip" VELOCE_README_OUT="$work/out" \
    cargo test --release --test readme_media -- --ignored

for name in player inspector filters subtitles welcome; do
    magick "$work/out/$name.png" -resize 1600x -quality 88 -strip "$media/$name.jpg"
done

# One caption per key press, drawn over the recording at the moment it was pressed.
captions=""
while read -r start end label; do
    captions+=",drawtext=fontfile=$font:text='$label':fontsize=50:fontcolor=white"
    captions+=":box=1:boxcolor=0x0E1015@0.82:boxborderw=28:x=(w-text_w)/2:y=80"
    captions+=":enable='between(t,$start,$end)'"
done < "$work/out/hops-captions.txt"

ffmpeg -v error -y -i "$work/out/hops.mp4" \
    -vf "scale=$clip_width:-2:flags=lanczos$captions" \
    -c:v libx264 -preset slow -crf "$clip_quality" -pix_fmt yuv420p -movflags +faststart -an \
    "$work/veloce-hops.mp4"

ls -lh "$media" "$work/veloce-hops.mp4"
echo "Upload $work/veloce-hops.mp4 to GitHub and paste its link into README.md (see the comment there)."
