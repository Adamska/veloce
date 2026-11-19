# Veloce

Veloce is a native, GPU-accelerated media player written in Rust. It is built around one idea: scrubbing, frame stepping and speed changes should be answered from memory instead of from the decoder. A sliding window of decoded frames and audio stays resident around the playhead, so a `+5s` or `-5s` tap that lands inside the window is a lookup, not a demuxer seek.

It plays local files and network streams through FFmpeg, decodes on the GPU where the platform allows it, renders video and interface in a single `wgpu` pass, and keeps dialogue at its natural pitch from `0.25x` to `4.0x`.

## Highlights

- Seeks inside the buffered window resolve in well under a microsecond and never flush the decoder.
- Bursts of out-of-window seeks preempt each other; only the final target is decoded to completion.
- Audio is the master clock. Video frames are selected against it on every display refresh.
- Pitch-preserving time stretching (Signalsmith Stretch), 10-band equalizer, preamp, 200 % volume.
- Smart silence skipper that accelerates quiet passages in lectures and podcasts.
- NV12, P010, YUV420P and 10-bit YUV420P straight to the GPU, with BT.601/709/2020 matrices and ACES filmic tone mapping for PQ and HLG sources.
- Picture controls, aspect and crop presets, zoom, pan, rotation, mirroring, deinterlacing and optional frame blending at `2.0x` and above.
- SRT, WebVTT and ASS subtitles (embedded or external), with live delay adjustment.
- A-B loop, lossless PNG snapshots, chapters on the scrubber, playlist with shuffle and repeat.
- A built-in procedural 4K 60 fps stream for testing without any media file.

## Quick start

### Requirements

- Rust 1.88 or newer.
- FFmpeg development libraries and `pkg-config` (developed against FFmpeg 7.1).
- A C++ toolchain and libclang (used to build the time-stretching library).

```sh
# macOS
brew install ffmpeg pkg-config

# Debian / Ubuntu
sudo apt install libavcodec-dev libavformat-dev libavutil-dev libavdevice-dev \
  libavfilter-dev libswscale-dev libswresample-dev pkg-config clang libasound2-dev
```

### Build and run

```sh
cargo run --release                       # welcome screen
cargo run --release -- --demo             # built-in 4K scrubbing demo
cargo run --release -- movie.mkv          # one or more files or URLs
cargo run --release -- https://example.com/live/index.m3u8
```

| Option | Effect |
| --- | --- |
| `--demo` | Start the procedural 4K 60 fps benchmark stream |
| `--mute` | Start with audio muted |
| `--paused` | Open media without starting playback |
| `--inspector` | Show the codec and performance inspector on launch |
| `--cache-mb <N>` | Memory budget of the frame ring buffer in MiB (default 3072) |

`VELOCE_CACHE_MB` sets the same budget from the environment and `VELOCE_HWACCEL=off` forces software decoding.

Files can also be dropped onto the window. Dropping media replaces the queue, or appends to it while the playlist drawer is open. Dropping an `.srt`, `.vtt` or `.ass` file loads it as a subtitle track. Subtitle files that share the media file name are picked up automatically.

## Keyboard map

| Key | Action |
| --- | --- |
| `Space` | Play / pause |
| `Left` / `Right` | Seek 5 s back / forward |
| `Shift` + `Left` / `Right` | Seek 30 s back / forward |
| `,` / `.` | Previous / next frame |
| `[` / `]` | Slower / faster (0.25x to 4.0x) |
| `=` | Reset speed to 1.0x |
| `Up` / `Down` | Volume up / down (to 200 %) |
| `M` | Mute |
| `F` | Fullscreen (also double-click) |
| `S` | Cycle subtitle tracks |
| `B` | Cycle audio tracks |
| `J` / `K` | Audio delay -25 ms / +25 ms |
| `G` / `H` | Subtitle delay -50 ms / +50 ms |
| `L` | A-B loop: set A, set B, clear |
| `N` / `P` | Next / previous playlist item |
| `Shift` + `N` / `P` | Next / previous chapter |
| `E` | Filters, equalizer and subtitle options |
| `Cmd/Ctrl` + `I` | Codec and performance inspector |
| `Cmd/Ctrl` + `L` | Playlist drawer |
| `Cmd/Ctrl` + `O` | Open file |
| `Cmd/Ctrl` + `U` | Open network stream (HTTP, HTTPS, HLS, RTSP) |
| `Cmd/Ctrl` + `S` | Save a PNG snapshot of the current frame |
| `Esc` | Close the open drawer, or leave fullscreen |

Snapshots are written to `~/Pictures/Veloce`. Right-clicking the audio or subtitle button in the control capsule opens the matching options tab.

## Architecture

```
                 +--------------------- decode thread ----------------------+
 file / URL ---> | Demuxer -> VideoDecoder (VideoToolbox, D3D11VA, NVDEC,   |
 or demo source  |            VAAPI, or threaded software) -> FrameConverter |
                 |         -> AudioDecoder -> resampler (stereo f32)        |
                 |         -> SubtitleDecoder                               |
                 +-------+----------------------+----------------+----------+
                         |                      |                |
                         v                      v                v
                   FrameCache             AudioTimeline     subtitle cues
              (frames around the       (PCM for the same    (to the player)
                   playhead)                window)
                         |                      |
     render thread       |                      |      DSP thread
  +----------------------+------+     +---------+----------------------+
  | PlaybackClock.tick()        |     | equalizer -> silence detector  |
  | FrameCache.frame_at(t)      |     | -> time stretcher -> SPSC ring |
  | wgpu: video quad + egui HUD |     +---------------+----------------+
  +-------------^---------------+                     |
                |                                     v
                |   clock anchor (seqlock)    audio callback (cpal)
                +-------------------------- gain, soft clip, timing
```

### Predictive ring buffer

`engine::FrameCache` holds a contiguous run of decoded frames in presentation order, targeting ten seconds behind and twenty-five seconds ahead of the playhead. Lookups take a read lock, binary search the run and clone an `Arc`. Audio for the same span lives in `engine::AudioTimeline`, indexed by sample, so an in-window seek repositions sound as cheaply as picture.

`engine::SessionShared::seek` decides hit or miss atomically:

- **Hit**: the playhead moves, a new epoch is issued, nothing else happens. The decoder keeps filling the window ahead.
- **Miss**: the request is written to a single pending slot. A request that is still waiting when the next one arrives is overwritten. A request that is already being decoded is abandoned as soon as a newer one is taken, so only the last target of a burst is decoded to completion.

On a miss the demuxer seeks to the keyframe at or before the target, using the container index read at open (refreshed after the first seek for containers that load it lazily). Frames between that keyframe and the target are decoded but not downloaded from the hardware surface. When the target lies ahead inside the current group of pictures the demuxer is not repositioned at all and decoding simply rolls forward.

The window is bounded by a memory budget because uncompressed frames are large:

| Content | Frame size | Window with the default 3 GiB |
| --- | --- | --- |
| 1080p 24 fps, 8-bit | 3.0 MiB | the full -10 s / +25 s |
| 1080p 60 fps, 8-bit | 3.0 MiB | about -4.9 s / +12.3 s |
| 2160p 30 fps, 10-bit | 23.7 MiB | about -1.2 s / +3.1 s |
| 2160p 60 fps, 8-bit | 11.9 MiB | about -1.2 s / +3.1 s |

The budget is split 2:5 between history and lookahead. At least 1.5 s of lookahead is always kept, even above the budget, so audio never starves at high speed.

### Clock and A/V sync

The audio callback publishes an anchor (media time, monotonic timestamp, rate) through a seqlock after every buffer. `engine::PlaybackClock` extrapolates from the latest anchor and filters callback jitter with a small proportional correction, snapping only when the error exceeds 40 ms. Every block the DSP thread queues carries the media time it represents, corrected for the time stretcher's latency at the current speed, so the clock stays truthful through speed changes, silence skipping and audio delay adjustments.

After a seek the clock holds the target until audio belonging to the new epoch is actually playing, and audio does not start until the target frame has been selected for display. Without an output device the same renderer is driven by a timer, so video-only playback uses the identical path.

### Audio chain

`equalizer` (ten peaking biquads plus preamp) -> `silence` (RMS detector with hold, attack and release) -> `stretcher` (Signalsmith Stretch) -> lock-free ring -> callback (gain ramp, soft clipper above unity, channel mapping). Volume and mute are applied in the callback, so they respond within one buffer.

### Rendering

`gpu::VideoPipeline` uploads decoder planes at their native stride into `R8`/`RG8` or `R16`/`RG16` textures and draws one quad. `shaders/video.wgsl` performs the matrix conversion with bit-depth aware quantization, PQ or HLG linearization, BT.2020 to BT.709 gamut mapping, ACES filmic tone mapping, grading (brightness, contrast, saturation, hue, gamma), unsharp masking, linear-blend deinterlacing and optional blending with the previous frame. The interface is drawn by `egui` in the same render pass. Snapshots re-render the current frame at source resolution into an offscreen target and encode it as PNG on a worker thread.

### Source layout

| Path | Responsibility |
| --- | --- |
| `src/media` | Frame, audio chunk, stream metadata and the `MediaSource` trait |
| `src/demo` | Procedural 4K 60 fps video and multi-tone audio generator |
| `src/engine` | Demuxer, decoders, hardware acceleration, ring buffer, audio timeline, clock, decode thread |
| `src/audio` | Device output, callback renderer, DSP thread, time stretcher, equalizer, silence detector |
| `src/gpu` | Device and surface, video pipeline, uniforms, geometry, snapshots, WGSL shader |
| `src/subtitles` | SRT, VTT and ASS parsing, time-indexed tracks, overlay renderer |
| `src/player` | Sessions, commands, transport, playlist, subtitle state, frame selection |
| `src/ui` | Control capsule, scrubber, drawers, inspector, toasts, theme |
| `src/app` | Window, event loop, keyboard map, file handling |

## Performance

Measured with `examples/seek_bench.rs`, a headless driver for the same engine the player uses. Release build, Apple M1 Max, 64 GB, macOS, FFmpeg 7.1, default 3 GiB budget. The sample clips were generated with FFmpeg (`testsrc2`, two-second keyframe interval).

| Measurement | Procedural 4K 60 | H.264 1080p 60 (MKV) | HEVC 10-bit 2160p 30 (MP4) |
| --- | --- | --- | --- |
| Decoder | generator | VideoToolbox | VideoToolbox |
| Window fill rate | 711 fps (11.8x) | 239 fps (4.0x) | 153 fps (5.1x) |
| In-window seek, median | 83 ns | 83 ns | 83 ns |
| In-window seek, p99 | 84 ns | 125 ns | 125 ns |
| Decoder flushes during 10,000 in-window seeks | 0 | 0 | 0 |
| Out-of-window seek to first frame, median | 0.6 ms | 213 ms | 79 ms |
| Out-of-window seek to first frame, slowest | 27 ms | 489 ms | 145 ms |
| 40 `+30s` seeks 15 ms apart: decoded to completion | 40 | 1 | 23 |

Reading the numbers:

- An in-window seek is a lock, a binary search and a reference count increment. The frame reaches the screen on the next refresh, at most 8.3 ms later at 120 Hz.
- An out-of-window seek costs a container seek plus decoding from the preceding keyframe. It scales with keyframe spacing and decoder throughput, not with file size.
- Holding a seek key on a slow source does not queue work. Thirty-nine of forty targets were abandoned before their frame was reached.
- Fill rate bounds sustained playback speed. The 1080p 60 clip decodes at 4.0x realtime on this machine, so `4.0x` playback of 60 fps material runs with no headroom.

Reproduce:

```sh
cargo run --release --example seek_bench -- --demo
cargo run --release --example seek_bench -- /path/to/file.mkv
```

The inspector (`Cmd/Ctrl` + `I`) shows the same counters live: window extent, memory in use, seek hit rate, resolve time, seek-to-frame latency, dropped frames and clock correction.

## Tests

```sh
cargo test
```

The suite is headless. It covers the ring buffer and seek coalescing, the clock, the audio timeline, pitch preservation across the speed range, marker alignment between the DSP thread and the callback, subtitle parsing, color matrices, offscreen GPU rendering including HDR10 tone mapping, an FFmpeg decode and seek round trip, the player end to end on the demo stream, and a layout pass over every interface surface. The player and interface tests open the audio device at zero volume; tests that need the `ffmpeg` binary or a GPU adapter skip themselves when it is missing.

Setting `VELOCE_UI_SNAPSHOT_DIR` while running `cargo test --test ui_smoke` writes a PNG of each interface surface to that directory.

## Limitations

- Decoded frames are downloaded from the hardware decoder and uploaded to the GPU, one copy each way. Direct surface sharing (IOSurface, DMA-BUF, D3D11 textures) is not implemented.
- The history side of the window is filled by playback. After a far seek, an immediate jump backwards is another out-of-window seek.
- Playback speed is limited by decode throughput. When the decoder cannot keep up, audio and video slow down together instead of dropping video.
- Bitmap subtitles (PGS, VobSub) are listed but not rendered, and ASS styling and positioning are reduced to plain text.
- Development and all measurements were done on macOS with VideoToolbox. The D3D11VA, NVDEC and VAAPI paths are selected at compile time and have not been exercised.
- The audio output follows the default device chosen at launch.
- Streams without a duration can only seek inside the buffered window.

## License

Veloce is released under the [MIT License](LICENSE).

The bundled typefaces, Inter and JetBrains Mono, are licensed under the SIL Open Font License 1.1. Their license texts are in `assets/fonts`.
