# Veloce

Veloce is a native, GPU-accelerated media player written in Rust. It is built around one idea: scrubbing, frame stepping and speed changes should be answered from memory instead of from the decoder. A sliding window of decoded frames and audio stays resident around the playhead, so a `+5s` or `-5s` tap that lands inside the window is a lookup, not a demuxer seek.

It plays local files and network streams through FFmpeg, picks the faster of hardware and threaded software decoding for each file, renders video and interface in a single `wgpu` pass, and keeps dialogue at its natural pitch from `0.25x` to `4.0x`.

## Highlights

- Seeks inside the buffered window resolve in well under a microsecond and never flush the decoder.
- Repeated `5s` and `30s` hops are answered by speculative decoders that already hold the exact frame at the next few landing positions. No approximate or keyframe-only frame is ever shown.
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

`VELOCE_CACHE_MB` sets the same budget from the environment. `VELOCE_HWACCEL` selects the video decoder: unset picks automatically, `on` forces hardware decoding and `off` forces software decoding.

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
| `E` | Filters and equalizer |
| `Cmd/Ctrl` + `I` | Codec and performance inspector |
| `Cmd/Ctrl` + `L` | Playlist drawer |
| `Cmd/Ctrl` + `O` | Open file |
| `Cmd/Ctrl` + `U` | Open network stream (HTTP, HTTPS, HLS, RTSP) |
| `Cmd/Ctrl` + `S` | Save a PNG snapshot of the current frame |
| `Esc` | Close the open drawer, or leave fullscreen |

Snapshots are written to `~/Pictures/Veloce`.

The control capsule keeps the common controls visible. The subtitle button opens a floating, draggable subtitle window with track selection, file loading, delay and appearance. The A-B loop and audio track buttons appear after pressing the More button, which stays highlighted while a loop is active. Right-clicking the audio track button opens the audio options.

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

`engine::SessionShared` decides every seek atomically, in this order:

1. **The window covers the target**: the playhead moves, a new epoch is issued, nothing else happens.
2. **A speculative decoder holds the target**: its frames and audio are exchanged into the window in constant time and that decoder becomes the main one. It carries on decoding from where it is, so nothing is decoded twice.
3. **A speculative decoder is on its way to the target**: it is promoted as it is and finishes the job, instead of starting over.
4. **Otherwise**: the request goes to a single pending slot. A request still waiting when the next arrives is overwritten, and one already being decoded is abandoned as soon as a newer one is taken, so only the last target of a burst is decoded to completion.

On a cold seek the demuxer goes to the keyframe at or before the target, using the container index read at open (refreshed after the first seek for containers that load it lazily). Frames before the target are decoded but never downloaded from a hardware surface, and frames nothing else depends on are skipped inside the decoder. The frame shown is always the exact frame at the target.

### Speculative hops

Local files open four extra decoders (`engine::speculation`). They stay idle during ordinary playback and wake for five seconds after any seek, and while paused. Each one keeps a short pad of exact frames and audio at a predicted landing position:

- after a keyboard hop of stride `s`: `+s`, `+2s`, `+3s` and `-s` from the new position, which covers a held key and a change of mind;
- after any other seek: `+5s`, `-5s`, `+10s` and `+30s`.

Pads follow the playhead while playback continues. When a hop lands on one, step 2 above applies and the frame is on screen at the next refresh. The retired decoder is then pointed at the far end of the prediction, so a held key keeps finding prepared frames as long as the machine can decode one pad per key repeat.

Work is prioritized: the frame just requested comes first, the pad for the next hop second, the remaining pads third, and deep read-ahead last.

### Decoder selection

`VideoDecoder` chooses per file. On the test machine FFmpeg's frame-threaded software decoder measured about nine times faster than VideoToolbox for 1080p H.264 (1965 fps against 210 fps), so 8-bit H.264 up to 1440p uses software on machines with six or more cores. The same rule is applied, without having been measured, to MPEG-2, MPEG-4, VC-1, VP8 and VP9. Everything else (HEVC, 10-bit, 4K) uses VideoToolbox, D3D11VA, NVDEC or VAAPI, with software as the fallback.

### Memory

The window is bounded by a memory budget because uncompressed frames are large. Each speculative pad takes one sixteenth of the budget and the main window gets the rest (2304 MiB of the default 3 GiB for local files):

| Content | Frame size | Main window |
| --- | --- | --- |
| 1080p 24 fps, 8-bit | 3.0 MiB | about -9.2 s / +23.1 s |
| 1080p 60 fps, 8-bit | 3.0 MiB | about -6.0 s / +6.9 s |
| 2160p 30 fps, 10-bit | 23.7 MiB | about -1.6 s / +1.6 s |
| 2160p 60 fps, 8-bit | 11.9 MiB | about -1.6 s / +1.6 s |

The budget is shared evenly between history and lookahead until each side holds six seconds, so a `5s` hop in either direction stays inside the window whenever memory allows; the remainder goes to lookahead. At least 1.5 s of lookahead is always kept, and pads keep at least 0.25 s, even above the budget, so audio never starves. At 4K this can exceed the budget by a few hundred MiB while the speculative decoders are awake.

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
| `src/engine` | Demuxer, decoders, hardware acceleration, ring buffer, audio timeline, clock, decode lanes and speculation |
| `src/audio` | Device output, callback renderer, DSP thread, time stretcher, equalizer, silence detector |
| `src/gpu` | Device and surface, video pipeline, uniforms, geometry, snapshots, WGSL shader |
| `src/subtitles` | SRT, VTT and ASS parsing, time-indexed tracks, overlay renderer |
| `src/player` | Sessions, commands, transport, playlist, subtitle state, frame selection |
| `src/ui` | Control capsule, scrubber, drawers, inspector, toasts, theme |
| `src/app` | Window, event loop, keyboard map, file handling |

## Performance

Measured with `examples/seek_bench.rs`, a headless driver for the same engine the player uses. Release build, Apple M1 Max (10 cores), 64 GB, macOS, FFmpeg 7.1, default 3 GiB budget. The clips were generated with FFmpeg from `testsrc2`: a film-like x264 encode with B-frames and ten-second keyframe spacing, a 60 fps encode with two-second spacing, and a 10-bit HEVC 4K encode with two-second spacing.

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

For comparison, the same film clip with the earlier behaviour (hardware decoding forced, no speculative decoders):

| Measurement | Before | Now |
| --- | --- | --- |
| Cold random seek, median | 412 ms | 63 ms |
| `+5s` x40 every 90 ms: frames never shown before the next press | 35 | 0 |
| `-5s` x40 every 90 ms: frames never shown before the next press | 37 | 1 |

Reading the numbers:

- An in-window seek is a lock, a binary search and a reference count increment. The frame reaches the screen on the next refresh, at most 8.3 ms later at 120 Hz.
- A cold seek costs a container seek plus decoding the dependency chain from the preceding keyframe. That chain cannot be skipped without showing an inexact frame, so it sets the floor for the first press of a new pattern.
- A held key is sustained as long as one pad can be decoded per key repeat. At a 33 ms repeat that is beyond this machine for long keyframe intervals; presses that cannot be prepared in time are still decoded exactly, never approximated.
- Fill rate bounds sustained playback speed.

Reproduce:

```sh
cargo run --release --example seek_bench -- --demo
cargo run --release --example seek_bench -- /path/to/file.mkv
cargo run --release --example seek_bench -- --lanes 0 /path/to/file.mkv   # without speculation
```

The inspector (`Cmd/Ctrl` + `I`) shows the same counters live: window extent, memory in use, seek hits (and how many were prepared), resolve time, seek-to-frame latency, dropped frames and clock correction.

## Tests

```sh
cargo test
```

The suite is headless. It covers the ring buffer, seek coalescing and prepared hops in both directions, the clock, the audio timeline, pitch preservation across the speed range, marker alignment between the DSP thread and the callback, subtitle parsing, color matrices, offscreen GPU rendering including HDR10 tone mapping, an FFmpeg decode and seek round trip, the player end to end on the demo stream, and a layout pass over every interface surface. The player and interface tests open the audio device at zero volume; tests that need the `ffmpeg` binary or a GPU adapter skip themselves when it is missing.

Setting `VELOCE_UI_SNAPSHOT_DIR` while running `cargo test --test ui_smoke` writes a PNG of each interface surface to that directory.

## Limitations

- Decoded frames are downloaded from the hardware decoder and uploaded to the GPU, one copy each way. Direct surface sharing (IOSurface, DMA-BUF, D3D11 textures) is not implemented.
- The first press of a new pattern can be a cold seek: speculative decoders are idle during ordinary playback, so at 4K, where the window holds under two seconds each way, the first `5s` hop after a while costs one decode. The presses that follow are prepared.
- Speculation opens four extra decoders per local file and is not used for network streams.
- Playback speed is limited by decode throughput. When the decoder cannot keep up, audio and video slow down together instead of dropping video.
- Bitmap subtitles (PGS, VobSub) are listed but not rendered, and ASS styling and positioning are reduced to plain text.
- The software-versus-hardware choice is a fixed rule tuned on one machine, not a measurement taken at run time.
- Development and all measurements were done on macOS with VideoToolbox. The D3D11VA, NVDEC and VAAPI paths are selected at compile time and have not been exercised.
- The audio output follows the default device chosen at launch.
- Streams without a duration can only seek inside the buffered window.

## License

Veloce is released under the [MIT License](LICENSE).

The bundled typefaces, Inter and JetBrains Mono, are licensed under the SIL Open Font License 1.1. Their license texts are in `assets/fonts`.
