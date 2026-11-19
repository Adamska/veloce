use std::io::Cursor;

use anyhow::{Context as _, Result, bail};
use winit::window::Icon;

/// macOS takes the Dock icon from the bundle; other platforms read it from the window.
const LOGO: &[u8] = include_bytes!("../../assets/logo/veloce-128.png");

fn decode(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
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

pub fn window_icon() -> Option<Icon> {
    let icon = decode(LOGO).and_then(|(pixels, width, height)| {
        Icon::from_rgba(pixels, width, height).context("invalid window icon")
    });
    icon.map_err(|error| log::warn!("window icon unavailable: {error:#}"))
        .ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_embedded_logo_decodes_to_a_square_icon() {
        let (pixels, width, height) = super::decode(super::LOGO).expect("logo decodes");
        assert_eq!((width, height), (128, 128));
        assert_eq!(pixels.len(), 128 * 128 * 4);
        assert!(super::window_icon().is_some());
    }
}
