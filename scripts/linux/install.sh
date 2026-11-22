#!/usr/bin/env sh
# Installs Veloce for the current user: the binary in ~/.local/bin, plus a menu entry and icon
# so it shows up in the application launcher and in "Open With" for video and audio files.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="$HOME/.local/bin"

mkdir -p "$bin" "$data/applications" "$data/icons/hicolor/256x256/apps"
cp "$here/veloce" "$bin/veloce"
cp "$here/veloce.png" "$data/icons/hicolor/256x256/apps/veloce.png"
cat > "$data/applications/veloce.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Veloce
GenericName=Media Player
Comment=GPU media player with instant, frame-exact seeking
Exec=$bin/veloce %F
Icon=veloce
Terminal=false
Categories=AudioVideo;Video;Player;
MimeType=video/mp4;video/x-matroska;video/webm;video/quicktime;video/x-msvideo;video/mpeg;video/mp2t;video/x-flv;video/ogg;audio/flac;audio/mpeg;audio/mp4;audio/ogg;audio/x-wav;audio/x-matroska;
EOF
update-desktop-database "$data/applications" >/dev/null 2>&1 || true

echo "Installed Veloce to $bin/veloce"
case ":$PATH:" in
    *":$bin:"*) ;;
    *) echo "Add $bin to your PATH to start it from a terminal, or launch it from the app menu." ;;
esac
