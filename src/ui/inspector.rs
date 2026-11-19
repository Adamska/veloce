use egui::{Align2, Area, Id, Order, RichText};

use super::{
    ViewContext, theme,
    widgets::{capsule, section, value_row},
};
use crate::media::{PixelLayout, Transfer};

const MEGABYTE: f64 = 1024.0 * 1024.0;

fn bit_rate(value: Option<u64>) -> String {
    match value {
        Some(bits) if bits >= 1_000_000 => format!("{:.2} Mb/s", bits as f64 / 1e6),
        Some(bits) => format!("{:.0} kb/s", bits as f64 / 1e3),
        None => "n/a".to_owned(),
    }
}

fn surface_format(layout: PixelLayout, transfer: Transfer) -> String {
    let planes = match layout {
        PixelLayout::Nv12 => "NV12 8-bit",
        PixelLayout::P010 => "P010 10-bit",
        PixelLayout::Yuv420p => "YUV420P 8-bit",
        PixelLayout::Yuv420p10 => "YUV420P 10-bit",
    };
    let range = match transfer {
        Transfer::Sdr => "SDR",
        Transfer::Pq => "HDR10 (PQ), ACES tone mapped",
        Transfer::Hlg => "HLG, ACES tone mapped",
    };
    format!("{planes}, {range}")
}

pub fn show(ctx: &egui::Context, view: &ViewContext<'_>) {
    let player = view.player;
    Area::new(Id::unique("inspector"))
        .order(Order::Foreground)
        .anchor(Align2::LEFT_TOP, [16.0, 92.0])
        .interactable(false)
        .show(ctx, |ui| {
            capsule(1.14).show(ui, |ui| {
                ui.set_width(330.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.spacing_mut().interact_size.y = 13.0;
                ui.label(
                    RichText::new("Codec and performance")
                        .font(theme::sans(13.0))
                        .color(theme::TEXT),
                );

                if let Some(info) = player.info() {
                    section(ui, "Media");
                    value_row(ui, "container", info.container.clone());
                    value_row(ui, "bitrate", bit_rate(info.bit_rate));
                    if let Some(video) = &info.video {
                        value_row(ui, "video codec", video.codec.clone());
                        value_row(
                            ui,
                            "resolution",
                            format!(
                                "{}x{} @ {:.3} fps",
                                video.width, video.height, video.frame_rate
                            ),
                        );
                        value_row(ui, "video bitrate", bit_rate(video.bit_rate));
                    }
                    if let Some(frame) = view.displayed {
                        value_row(
                            ui,
                            "surface",
                            surface_format(frame.layout, frame.color.transfer),
                        );
                    }
                    if let Some(track) = player.audio_track().and_then(|i| info.audio_tracks.get(i))
                    {
                        value_row(
                            ui,
                            "audio",
                            format!(
                                "{} {} ch {} Hz",
                                track.codec, track.channels, track.sample_rate
                            ),
                        );
                    }
                }

                let telemetry = player.telemetry().unwrap_or_default();
                section(ui, "Decode");
                value_row(ui, "decoder", telemetry.backend.clone());
                value_row(ui, "renderer", view.stats.adapter.to_owned());
                value_row(ui, "frames decoded", telemetry.decoded_frames.to_string());
                value_row(ui, "frames dropped", telemetry.dropped_frames.to_string());
                value_row(ui, "decode errors", telemetry.decode_errors.to_string());

                section(ui, "Ring buffer");
                let limits = player.cache_limits();
                match player.cache_span() {
                    Some(span) => {
                        value_row(
                            ui,
                            "window",
                            format!(
                                "{:+.1} s / {:+.1} s",
                                (span.start - player.position()) as f64 / 1e6,
                                (span.end - player.position()) as f64 / 1e6
                            ),
                        );
                        value_row(
                            ui,
                            "frames in memory",
                            format!(
                                "{} ({:.0} / {:.0} MiB)",
                                span.frames,
                                span.bytes as f64 / MEGABYTE,
                                limits.budget_bytes as f64 / MEGABYTE
                            ),
                        );
                    }
                    None => value_row(ui, "window", "empty"),
                }
                let hit_rate = telemetry
                    .hit_rate()
                    .map_or_else(|| "n/a".to_owned(), |rate| format!("{:.1} %", rate * 100.0));
                value_row(ui, "seek hit rate", hit_rate);
                value_row(
                    ui,
                    "hits / misses",
                    format!("{} / {}", telemetry.seeks_hit, telemetry.seeks_missed),
                );
                value_row(
                    ui,
                    "merged / preempted",
                    format!(
                        "{} / {}",
                        telemetry.seeks_coalesced, telemetry.seeks_superseded
                    ),
                );
                value_row(
                    ui,
                    "seek resolve",
                    format!("{:.2} us", telemetry.resolve_micros),
                );
                value_row(
                    ui,
                    "seek to frame",
                    format!("{:.1} ms", telemetry.present_millis),
                );

                section(ui, "Sync and output");
                let (device, rate) = player.audio_device();
                value_row(ui, "audio device", format!("{device} {rate} Hz"));
                value_row(ui, "speed", format!("{:.2}x", player.effective_speed()));
                value_row(
                    ui,
                    "clock correction",
                    format!("{:+.2} ms", player.clock_correction() as f64 / 1e3),
                );
                value_row(
                    ui,
                    "silence skipped",
                    format!("{:.1} s", telemetry.silence_skipped_seconds),
                );
                value_row(
                    ui,
                    "render",
                    format!(
                        "{:.0} fps, {:.2} ms",
                        view.stats.fps, view.stats.frame_millis
                    ),
                );
                value_row(ui, "texture uploads", view.stats.uploads.to_string());
            });
        });
}
