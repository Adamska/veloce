use anyhow::Context as _;
use winit::window::Icon;

use crate::ui::logo;

/// macOS takes the Dock icon from the bundle; other platforms read it from the window.
pub fn window_icon() -> Option<Icon> {
    let icon = logo::decode_rgba(logo::LOGO_128).and_then(|(pixels, width, height)| {
        Icon::from_rgba(pixels, width, height).context("invalid window icon")
    });
    icon.map_err(|error| log::warn!("window icon unavailable: {error:#}"))
        .ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_embedded_logo_decodes_to_a_square_icon() {
        let (pixels, width, height) =
            crate::ui::logo::decode_rgba(crate::ui::logo::LOGO_128).expect("logo decodes");
        assert_eq!((width, height), (128, 128));
        assert_eq!(pixels.len(), 128 * 128 * 4);
        assert!(super::window_icon().is_some());
    }
}
