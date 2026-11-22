Veloce for Linux (x86_64)
=========================

Run it directly:

    ./veloce
    ./veloce /path/to/video.mkv

Or install it for your user (application menu entry, icon, "Open With" for media files):

    ./install.sh

Requirements
------------
- A 64-bit x86 Linux released since 2022 (glibc 2.35 or newer: Ubuntu 22.04+, Debian 12+,
  Fedora 36+, Mint 21+, Pop!_OS 22.04+, Arch, ...).
- A GPU driver with Vulkan or OpenGL ES 3 (Mesa, NVIDIA or AMD drivers all qualify).
- ALSA, PipeWire or PulseAudio for sound (libasound2 is installed on every desktop distribution).
- The file chooser uses the desktop portal (xdg-desktop-portal), present on GNOME, KDE and most
  other desktops. Drag and drop works everywhere.

FFmpeg is built in, nothing else needs to be installed. Video is decoded on the CPU, or with
NVDEC on NVIDIA GPUs. VAAPI (Intel and AMD hardware decoding) is not included in this build.

Shortcuts: Space play/pause, Left/Right jump 5 s (Shift for 30 s), [ and ] change speed,
Shift+[ and Shift+] by 0.05, Ctrl+O open a file. Full list:
https://github.com/Adamska/veloce#getting-started

Settings and recent files are kept in ~/.local/share/veloce.
To uninstall: rm ~/.local/bin/veloce ~/.local/share/applications/veloce.desktop
              ~/.local/share/icons/hicolor/256x256/apps/veloce.png
