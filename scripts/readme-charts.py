#!/usr/bin/env python3
"""Writes the README benchmark figure as light and dark SVGs in docs/media.

The numbers come from `cargo run --release --example seek_bench` (see docs/performance.md).
Each panel is an emphasis chart: Veloce in the accent hue, the baseline in neutral gray,
every bar labelled with its value so colour is never the only cue.
"""

from pathlib import Path

PANELS = [
    {
        "title": "Cold seek to the exact frame",
        "subtitle": "Median over random targets, H.264 1080p 24 fps, 10 s keyframe spacing. Lower is better.",
        "callout": "6.5× faster",
        "rows": [("Earlier build", 412, "412 ms", False), ("Veloce", 63, "63 ms", True)],
    },
    {
        "title": "Rapid 5 s hops whose frame never appeared",
        "subtitle": "80 presses (40 forward, 40 back), one every 90 ms. Lower is better.",
        "callout": "72 to 1",
        "rows": [("Earlier build", 72, "72 of 80", False), ("Veloce", 1, "1 of 80", True)],
    },
    {
        "title": "Decoding speed, H.264 1080p",
        "subtitle": "Frames per second available to fill the window. Higher is better.",
        "callout": "9.4× faster",
        "rows": [
            ("VideoToolbox", 210, "210 fps", False),
            ("Software (chosen)", 1965, "1,965 fps", True),
        ],
    },
]
FOOTNOTE = (
    "Apple M1 Max, release build, examples/seek_bench.rs. "
    "Earlier build: hardware decoding forced, no speculative decoders."
)

THEMES = {
    "light": {
        "ink": "#0b0b0b",
        "secondary": "#52514e",
        "muted": "#898781",
        "baseline": "#c3c2b7",
        "accent": "#1baf7a",
        "context": "#898781",
    },
    "dark": {
        "ink": "#f0efec",
        "secondary": "#c3c2b7",
        "muted": "#898781",
        "baseline": "#383835",
        "accent": "#199e70",
        "context": "#6b6a65",
    },
}

WIDTH = 820
LABEL_WIDTH = 150
VALUE_SPACE = 92
BAR = 22
ROW_GAP = 10
PANEL_GAP = 34
RADIUS = 4
FONT = "system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"


def bar_path(x: float, y: float, length: float) -> str:
    """Square at the baseline, 4 px rounded at the data end."""
    r = min(RADIUS, length / 2, BAR / 2)
    return (
        f"M{x:.1f},{y:.1f} H{x + length - r:.1f} "
        f"Q{x + length:.1f},{y:.1f} {x + length:.1f},{y + r:.1f} "
        f"V{y + BAR - r:.1f} Q{x + length:.1f},{y + BAR:.1f} {x + length - r:.1f},{y + BAR:.1f} "
        f"H{x:.1f} Z"
    )


def escape(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def render(theme: dict) -> str:
    parts = []
    y = 8.0
    plot_left = LABEL_WIDTH
    plot_width = WIDTH - LABEL_WIDTH - VALUE_SPACE
    for panel in PANELS:
        parts.append(
            f'<text x="0" y="{y + 16:.1f}" font-size="16" font-weight="600" fill="{theme["ink"]}">'
            f'{escape(panel["title"])}</text>'
        )
        parts.append(
            f'<text x="{WIDTH}" y="{y + 16:.1f}" font-size="15" font-weight="600" text-anchor="end" '
            f'fill="{theme["ink"]}">{escape(panel["callout"])}</text>'
        )
        parts.append(
            f'<text x="0" y="{y + 36:.1f}" font-size="12.5" fill="{theme["secondary"]}">'
            f'{escape(panel["subtitle"])}</text>'
        )
        y += 52
        top = y
        peak = max(value for _, value, _, _ in panel["rows"])
        for label, value, shown, highlighted in panel["rows"]:
            length = max(plot_width * value / peak, 2.0)
            color = theme["accent"] if highlighted else theme["context"]
            weight = "600" if highlighted else "400"
            parts.append(
                f'<text x="{plot_left - 12}" y="{y + BAR / 2 + 4.5:.1f}" font-size="13" '
                f'text-anchor="end" font-weight="{weight}" fill="{theme["ink"]}">{escape(label)}</text>'
            )
            parts.append(f'<path d="{bar_path(plot_left, y, length)}" fill="{color}"/>')
            parts.append(
                f'<text x="{plot_left + length + 8:.1f}" y="{y + BAR / 2 + 4.5:.1f}" font-size="13" '
                f'font-weight="{weight}" fill="{theme["ink"]}">{escape(shown)}</text>'
            )
            y += BAR + ROW_GAP
        parts.append(
            f'<line x1="{plot_left}" y1="{top - 4:.1f}" x2="{plot_left}" y2="{y - ROW_GAP + 4:.1f}" '
            f'stroke="{theme["baseline"]}" stroke-width="1"/>'
        )
        y += PANEL_GAP - ROW_GAP
    parts.append(
        f'<text x="0" y="{y + 6:.1f}" font-size="11.5" fill="{theme["muted"]}">{escape(FOOTNOTE)}</text>'
    )
    height = y + 16
    body = "\n  ".join(parts)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height:.0f}" '
        f'viewBox="0 0 {WIDTH} {height:.0f}" font-family="{FONT}" role="img" '
        f'aria-label="Veloce benchmark results">\n  <title>Veloce benchmark results</title>\n  {body}\n</svg>\n'
    )


def main() -> None:
    out = Path(__file__).resolve().parent.parent / "docs" / "media"
    out.mkdir(parents=True, exist_ok=True)
    for name, theme in THEMES.items():
        (out / f"benchmarks-{name}.svg").write_text(render(theme), encoding="utf-8")
        print(f"Wrote {out / f'benchmarks-{name}.svg'}")


if __name__ == "__main__":
    main()
