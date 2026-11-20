# Performance

All numbers below come from `examples/seek_bench.rs` and can be reproduced with the commands at the end of this page. The [README](../README.md) shows the headline results as a chart.

## Method

Measured with `examples/seek_bench.rs`, a headless driver for the same engine the player uses. Release build, Apple M1 Max (10 cores), 64 GB, macOS, FFmpeg 7.1, default 3 GiB budget. The clips were generated with FFmpeg from `testsrc2`: a film-like x264 encode with B-frames and ten-second keyframe spacing, a 60 fps encode with two-second spacing, and a 10-bit HEVC 4K encode with two-second spacing.

## Results

A press counts as **instant** when the exact frame was already decoded at the moment the key was pressed.

| Measurement | Procedural 4K 60 | H.264 1080p 24 | H.264 1080p 60 | HEVC 10-bit 2160p 30 |
| --- | --- | --- | --- | --- |
| Decoder chosen | generator | software | software | VideoToolbox |
| Window fill rate | 746 fps | 1965 fps (82x) | 1683 fps (28x) | 133 fps (4.4x) |
| In-window seek, median | 0.4 us | 0.5 us | 0.4 us | 0.4 us |
| Cold random seek to exact frame, median | 0.6 ms | 63 ms | 43 ms | 75 ms |
| Cold random seek, slowest | 12 ms | 114 ms | 74 ms | 147 ms |
| `+5s` x40, one press every 90 ms: instant | 40 | 40 | 40 | 40 |
| `-5s` x40, one press every 90 ms: instant | 40 | 36 | 40 | 40 |
| `+30s`, one press every 90 ms: instant | 19 of 19 | 6 of 9 | 7 of 7 | 6 of 7 |
| `+5s` x40, one press every 33 ms: instant | 40 | 9 | 15 | 21 |

Every press in the 90 ms rows that was not instant still showed its exact frame before the next press, except one each in the two H.264 1080p 24 rows. The benchmark fails if any hop shows a frame other than the one covering the target, or leaves a gap in the window.

## Before and after

For comparison, the same film clip with the earlier behaviour (hardware decoding forced, no speculative decoders):

| Measurement | Before | Now |
| --- | --- | --- |
| Cold random seek, median | 412 ms | 63 ms |
| `+5s` x40 every 90 ms: frames never shown before the next press | 35 | 0 |
| `-5s` x40 every 90 ms: frames never shown before the next press | 37 | 1 |

## Reading the numbers


- An in-window seek is a lock, a binary search and a reference count increment. The frame reaches the screen on the next refresh, at most 8.3 ms later at 120 Hz.
- A cold seek costs a container seek plus decoding the dependency chain from the preceding keyframe. That chain cannot be skipped without showing an inexact frame, so it sets the floor for the first press of a new pattern.
- A held key is sustained as long as one pad can be decoded per key repeat. At a 33 ms repeat that is beyond this machine for long keyframe intervals; presses that cannot be prepared in time are still decoded exactly, never approximated.
- Fill rate bounds sustained playback speed.

## Reproduce


```sh
cargo run --release --example seek_bench -- --demo
cargo run --release --example seek_bench -- /path/to/file.mkv
cargo run --release --example seek_bench -- --lanes 0 /path/to/file.mkv   # without speculation
```

The inspector (`Cmd/Ctrl` + `I`) shows the same counters live: window extent, memory in use, seek hits (and how many were prepared), resolve time, seek-to-frame latency, dropped frames and clock correction.
