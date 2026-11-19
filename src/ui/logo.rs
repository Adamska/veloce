use std::io::Cursor;

use anyhow::{Context as _, Result, bail};

pub const LOGO_128: &[u8] = include_bytes!("../../assets/logo/veloce-128.png");
const LOGO_256: &[u8] = include_bytes!("../../assets/logo/veloce-256.png");

/// Decodes an 8-bit RGBA PNG into straight-alpha pixels.
pub fn decode_rgba(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().context("unreadable logo header")?;
    let size = reader.output_buffer_size().context("logo too large")?;
    let mut pixels = vec![0; size];
    let frame = reader
        .next_frame(&mut pixels)
        .context("unreadable logo pixels")?;
    if frame.color_type != png::ColorType::Rgba {
        bail!("logo must be RGBA, found {:?}", frame.color_type);
    }
    pixels.truncate(frame.buffer_size());
    Ok((pixels, frame.width, frame.height))
}

pub fn texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let (pixels, width, height) = decode_rgba(LOGO_256)
        .map_err(|error| log::warn!("logo unavailable: {error:#}"))
        .ok()?;
    let image =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &pixels);
    Some(ctx.load_texture("veloce logo", image, egui::TextureOptions::LINEAR))
}
