#!/bin/bash
set -euo pipefail

usage() {
    echo "Usage: scripts/macos/bundle.sh [--install] [--destination DIR]"
    echo
    echo "Builds a release Veloce.app in target/release/bundle."
    echo "  --install            Copy the app to /Applications, or ~/Applications when"
    echo "                       /Applications is not writable, and register it with Finder"
    echo "  --destination DIR    Install into DIR instead"
}

root=$(cd "$(dirname "$0")/../.." && pwd)
install=false
destination=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --install) install=true ;;
        --destination) install=true; destination="${2:?--destination needs a directory}"; shift ;;
        -h | --help) usage; exit 0 ;;
        *) usage >&2; exit 2 ;;
    esac
    shift
done

cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
build=$(git rev-list --count HEAD 2>/dev/null || echo 1)

cargo build --release --bin veloce

app="target/release/bundle/Veloce.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/veloce "$app/Contents/MacOS/veloce"
# macOS 26 and later only render an icon natively from an Icon Composer document compiled into
# Assets.car; a plain .icns is treated as legacy and shrunk into a grey tile.
if xcrun --find actool >/dev/null 2>&1; then
    xcrun actool assets/macos/AppIcon.icon --compile "$app/Contents/Resources" \
        --output-format human-readable-text --errors \
        --output-partial-info-plist "$app/icon-info.plist" --app-icon AppIcon \
        --include-all-app-icons --enable-on-demand-resources NO --development-region en \
        --target-device mac --platform macosx --minimum-deployment-target 11.0 >/dev/null
    rm -f "$app/icon-info.plist"
else
    echo "actool not found (install Xcode); falling back to the legacy icon." >&2
    cp assets/macos/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
fi
sed -e "s/__VERSION__/$version/" -e "s/__BUILD__/$build/" assets/macos/Info.plist \
    > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
plutil -lint -s "$app/Contents/Info.plist"
codesign --force --sign - --timestamp=none "$app"
echo "Built $root/$app"

if ! $install; then
    exit 0
fi

if [[ -z "$destination" ]]; then
    if [[ -w /Applications ]]; then
        destination=/Applications
    else
        destination="$HOME/Applications"
    fi
fi
mkdir -p "$destination"
target="$destination/Veloce.app"
if pgrep -xq veloce; then
    echo "Veloce is running; quit it before installing." >&2
    exit 1
fi
rm -rf "$target"
ditto "$app" "$target"
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
"$lsregister" -u "$root/$app" 2>/dev/null || true
"$lsregister" -f -R "$target"
echo "Installed $target"
