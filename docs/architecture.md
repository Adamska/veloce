# Architecture

How Veloce turns a seek into a frame on screen. The [README](../README.md) has the overview; this page has the detail.

## Threads and data flow

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

## Predictive ring buffer

`engine::FrameCache` holds a contiguous run of decoded frames in presentation order, targeting ten seconds behind and twenty-five seconds ahead of the playhead. Lookups take a read lock, binary search the run and clone an `Arc`. Audio for the same span lives in `engine::AudioTimeline`, indexed by sample, so an in-window seek repositions sound as cheaply as picture.

`engine::SessionShared` decides every seek atomically, in this order:

1. **The window covers the target**: the playhead moves, a new epoch is issued, nothing else happens.
2. **A speculative decoder holds the target**: its frames and audio are exchanged into the window in constant time and that decoder becomes the main one. It carries on decoding from where it is, so nothing is decoded twice.
3. **A speculative decoder is on its way to the target**: it is promoted as it is and finishes the job, instead of starting over.
4. **Otherwise**: the request goes to a single pending slot. A request still waiting when the next arrives is overwritten, and one already being decoded is abandoned as soon as a newer one is taken, so only the last target of a burst is decoded to completion.

On a cold seek the demuxer goes to the keyframe at or before the target, using the container index read at open (refreshed after the first seek for containers that load it lazily). Frames before the target are decoded but never downloaded from a hardware surface, and frames nothing else depends on are skipped inside the decoder. The frame shown is always the exact frame at the target.

## Speculative hops

Local files open four extra decoders (`engine::speculation`). They stay idle during ordinary playback and wake for five seconds after any seek, and while paused. Each one keeps a short pad of exact frames and audio at a predicted landing position:

- after a keyboard hop of stride `s`: `+s`, `+2s`, `+3s` and `-s` from the new position, which covers a held key and a change of mind;
- after any other seek: `+5s`, `-5s`, `+10s` and `+30s`.

Pads follow the playhead while playback continues. When a hop lands on one, step 2 above applies and the frame is on screen at the next refresh. The retired decoder is then pointed at the far end of the prediction, so a held key keeps finding prepared frames as long as the machine can decode one pad per key repeat.

Work is prioritized: the frame just requested comes first, the pad for the next hop second, the remaining pads third, and deep read-ahead last.

## Decoder selection

`VideoDecoder` chooses per file. On the test machine FFmpeg's frame-threaded software decoder measured about nine times faster than VideoToolbox for 1080p H.264 (1965 fps against 210 fps), so 8-bit H.264 up to 1440p uses software on machines with six or more cores. The same rule is applied, without having been measured, to MPEG-2, MPEG-4, VC-1, VP8 and VP9. Everything else (HEVC, 10-bit, 4K) uses VideoToolbox, D3D11VA, NVDEC or VAAPI, with software as the fallback.

## Memory

The window is bounded by a memory budget because uncompressed frames are large. Each speculative pad takes one sixteenth of the budget and the main window gets the rest (2304 MiB of the default 3 GiB for local files):

| Content | Frame size | Main window |
| --- | --- | --- |
| 1080p 24 fps, 8-bit | 3.0 MiB | about -9.2 s / +23.1 s |
| 1080p 60 fps, 8-bit | 3.0 MiB | about -6.0 s / +6.9 s |
| 2160p 30 fps, 10-bit | 23.7 MiB | about -1.6 s / +1.6 s |
| 2160p 60 fps, 8-bit | 11.9 MiB | about -1.6 s / +1.6 s |

The budget is shared evenly between history and lookahead until each side holds six seconds, so a `5s` hop in either direction stays inside the window whenever memory allows; the remainder goes to lookahead. At least 1.5 s of lookahead is always kept, and pads keep at least 0.25 s, even above the budget, so audio never starves. At 4K this can exceed the budget by a few hundred MiB while the speculative decoders are awake.

## Clock and A/V sync

The audio callback publishes an anchor (media time, monotonic timestamp, rate) through a seqlock after every buffer. `engine::PlaybackClock` extrapolates from the latest anchor and filters callback jitter with a small proportional correction, snapping only when the error exceeds 40 ms. Every block the DSP thread queues carries the media time it represents, corrected for the time stretcher's latency at the current speed, so the clock stays truthful through speed changes, silence skipping and audio delay adjustments.

After a seek the clock holds the target until audio belonging to the new epoch is actually playing, and audio does not start until the target frame has been selected for display. Without an output device the same renderer is driven by a timer, so video-only playback uses the identical path.

## Audio chain

`equalizer` (ten peaking biquads plus preamp) -> `silence` (RMS detector with hold, attack and release) -> `stretcher` (Signalsmith Stretch) -> lock-free ring -> callback (gain ramp, soft clipper above unity, channel mapping). Volume and mute are applied in the callback, so they respond within one buffer.

## Rendering

`gpu::VideoPipeline` uploads decoder planes at their native stride into `R8`/`RG8` or `R16`/`RG16` textures and draws one quad. `shaders/video.wgsl` performs the matrix conversion with bit-depth aware quantization, PQ or HLG linearization, BT.2020 to BT.709 gamut mapping, ACES filmic tone mapping, grading (brightness, contrast, saturation, hue, gamma), unsharp masking, linear-blend deinterlacing and optional blending with the previous frame. The interface is drawn by `egui` in the same render pass. Snapshots re-render the current frame at source resolution into an offscreen target and encode it as PNG on a worker thread.

## Source layout

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
| `src/app` | Window, event loop, keyboard map, file handling, macOS document events |

## Recent files, settings, resume and logs

The welcome screen lists the files played recently, newest first, and hides any that no longer exist on disk. `player::RecentFiles` keeps the list in memory and, in the app only, in a plain text file: `~/Library/Application Support/Veloce/recent` on macOS, `%APPDATA%\Veloce\recent` on Windows and `~/.local/share/veloce/recent` on Linux. Whether each file still exists is checked on a background thread, so an unreachable network volume never blocks the interface; the screen repaints when the check finishes. Tests use the in-memory list and never touch the user's history.

Playback speed, volume and the subtitle choice are saved by `player::Settings` to a `settings` file next to `recent`, one `key value` pair per line. A change is written at once, and later changes at most every half second (dragging the volume slider changes it every frame); the last change is written when the app exits, including through the macOS application menu, which ends the process without dropping the player. Tests keep these settings in memory too.

`player::Resume` keeps where playback stopped, per file, in a `resume` file next to `settings`, along with the queue that was open and whether it was paused. A file closed within 5 s of its start or 10 s of its end is forgotten, so finished films start over. The player saves the open file's position when it closes or switches files, every 5 s while it runs (a crash loses at most that) and on exit. On a launch without files, `Player::restore_session` reopens that queue; whichever way a file is opened, `Session::open` gets its saved position as the start target, so the first frame shown is the exact one where playback stopped, never a nearby keyframe.

`app::logging` sends every record to stderr and to `veloce.log` (`~/Library/Logs/Veloce` on macOS, `$XDG_STATE_HOME/veloce` or `~/.local/state/veloce` on Linux, `%LOCALAPPDATA%\Veloce\Logs` on Windows), with UTC timestamps; a log over 4 MiB is moved to `veloce.old.log` at launch. A panic hook writes the panicking thread, location and backtrace. Each run also creates and locks a `running-<pid>` marker that it deletes on a clean exit: at launch, a marker that can be locked belonged to a run that crashed, was force quit or lost power, which is logged and shown as a warning. Markers of other instances still running stay locked and are left alone.

The subtitle choice carries over from one file to the next. `subtitles::TrackSignature` describes a track by language, title, flags (forced, SDH, signs, commentary) and position. ISO 639 codes and language names are normalized, so `fre`, `fr` and `French` match, and the language of a sidecar file is read from its name after the media stem (`Movie.fr.forced.srt`). When a file opens, the track with the best score is selected, across embedded tracks and sidecar files. Turning subtitles off is remembered too, and a choice the viewer makes while the file is loading is never overridden.

## macOS integration

```sh
scripts/macos/bundle.sh             # builds target/release/bundle/Veloce.app
scripts/macos/bundle.sh --install   # also installs to /Applications and registers it with Finder
```

The installed app appears in Launchpad and Spotlight, and in Finder's **Open With** menu for video and audio files, including MKV, WebM, AVI, FLV and FLAC. Files can be dropped on the Dock icon. Opening a file while Veloce is running plays it in the existing window.

To make Veloce the default player for a format, select a file of that type in Finder, choose **File > Get Info**, pick Veloce under **Open with** and click **Change All**.

The app icon and every logo size in `assets/logo` come from `assets/logo/veloce.png`. After replacing that image, run `scripts/render-logo.sh` to trim it and regenerate the sizes. The bundle script compiles `assets/macos/AppIcon.icon`, an Icon Composer document whose logo layer the render script refreshes, into `Assets.car` with Xcode's `actool`. That is the only format macOS 26 and later draw natively, including the tinted and clear appearances; a plain `.icns` is shown shrunk inside a grey tile. Without Xcode the bundle falls back to the legacy `assets/macos/AppIcon.icns`. The render script needs ImageMagick.

The bundle links the FFmpeg libraries installed by Homebrew and is signed ad hoc, so it is meant for the machine that built it. Running the script again replaces the installed copy; quit Veloce first. To uninstall, delete `/Applications/Veloce.app`.

## Testing

```sh
cargo test
```

The suite is headless. It covers the ring buffer, seek coalescing and prepared hops in both directions, the clock, the audio timeline, pitch preservation across the speed range, marker alignment between the DSP thread and the callback, subtitle parsing and the carried-over track choice, color matrices, offscreen GPU rendering including HDR10 tone mapping, an FFmpeg decode and seek round trip, the player end to end on the demo stream, and a layout pass over every interface surface. The player and interface tests open the audio device at zero volume; tests that need the `ffmpeg` binary or a GPU adapter skip themselves when it is missing.

Setting `VELOCE_UI_SNAPSHOT_DIR` while running `cargo test --test ui_smoke` writes a PNG of each interface surface to that directory.

The README media are produced the same way. `scripts/readme-media.sh` fetches three minutes of the open movie Sintel, and the ignored test `tests/readme_media.rs` drives the real player and interface offscreen: it takes the screenshots, then plays the clip and presses the hops on a schedule while streaming the rendered frames to FFmpeg in real time. The script captions the key presses and writes the recording to `target/readme-media/veloce-hops.mp4`, which is hosted as a GitHub attachment rather than committed. `scripts/readme-charts.py` draws the benchmark figure from the numbers in [performance.md](performance.md).

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
