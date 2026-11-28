use std::sync::LazyLock;

use config::Config;
use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::{
    FilterType, FontSize, Resize,
    picker::{Picker, ProtocolType},
    sliced::SlicedProtocol,
};

use super::{root::ComponentProps, textcomponent::TextNode};
use crate::util::general::{GENERAL_CONFIG, settings};

pub(crate) static IMAGE_PICKER: LazyLock<Picker> = LazyLock::new(|| {
    let detected = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    with_overrides(detected, &settings())
});

/// Applies `image_protocol` (kitty, sixel, iterm2, halfblocks) and `cell_size` (e.g. "10x20", pixels)
/// from the settings, for terminals where detection fails or answers wrongly (often inside tmux).
fn with_overrides(detected: Picker, settings: &Config) -> Picker {
    let mut picker = detected;
    // tmux swallows the terminal's answer to the graphics probe, so the probe finds nothing there.
    // tmux knows the real terminal, though.
    if picker.protocol_type() == ProtocolType::Halfblocks
        && picker.tmux_detected()
        && tmux_client_speaks_kitty()
    {
        picker.set_protocol_type(ProtocolType::Kitty);
    }
    if let Some((w, h)) = settings
        .get::<String>("cell_size")
        .ok()
        .and_then(|size| parse_cell_size(&size))
    {
        #[allow(deprecated)]
        let sized = Picker::from_fontsize(FontSize::new(w, h));
        let protocol = picker.protocol_type();
        picker = sized;
        picker.set_protocol_type(protocol);
    }
    let protocol = settings
        .get::<String>("image_protocol")
        .ok()
        .and_then(|name| match name.to_lowercase().as_str() {
            "kitty" => Some(ProtocolType::Kitty),
            "sixel" => Some(ProtocolType::Sixel),
            "iterm2" => Some(ProtocolType::Iterm2),
            "halfblocks" => Some(ProtocolType::Halfblocks),
            _ => None,
        });
    if let Some(protocol) = protocol {
        picker.set_protocol_type(protocol);
    }
    picker
}

/// Asks tmux which terminal it is attached to, and whether that one draws Kitty graphics.
fn tmux_client_speaks_kitty() -> bool {
    std::process::Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "#{client_termname} #{client_termtype}",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| names_kitty_graphics_terminal(&String::from_utf8_lossy(&out.stdout)))
}

/// Terminals known to implement the Kitty graphics protocol (including Unicode placeholders).
fn names_kitty_graphics_terminal(names: &str) -> bool {
    let names = names.to_lowercase();
    names.contains("ghostty") || names.contains("kitty")
}

fn parse_cell_size(size: &str) -> Option<(u16, u16)> {
    let (w, h) = size.split_once('x')?;
    let (w, h) = (w.trim().parse::<u16>().ok()?, h.trim().parse::<u16>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

pub struct ImageComponent {
    _alt_text: String,
    y_offset: u16,
    x_offset: u16,
    height: u16,
    scroll_offset: u16,
    source: Option<DynamicImage>,
    image: Option<SlicedProtocol>,
    size: Size,
}

impl ImageComponent {
    pub fn new<T: ToString>(image: DynamicImage, width: u16, alt_text: T) -> Self {
        let resize = Resize::Fit(Some(FilterType::CatmullRom));
        // `parse_markdown` receives the text-wrap width, while the document
        // viewport reserves one additional column for its layout.
        let available = Size::new(width.saturating_sub(1), GENERAL_CONFIG.image_max_height);
        let size = resize.size_for(&image, IMAGE_PICKER.font_size(), available);

        Self {
            height: size.height,
            source: Some(image),
            image: None,
            size,
            _alt_text: alt_text.to_string(),
            scroll_offset: 0,
            y_offset: 0,
            x_offset: 0,
        }
    }

    /// An image that is already exactly `cols` x `rows` cells (typeset math), drawn `x_offset` cells in.
    pub fn from_cells<T: ToString>(
        image: DynamicImage,
        cols: u16,
        rows: u16,
        x_offset: u16,
        alt_text: T,
    ) -> Self {
        Self {
            height: rows,
            source: Some(image),
            image: None,
            size: Size::new(cols, rows),
            _alt_text: alt_text.to_string(),
            scroll_offset: 0,
            y_offset: 0,
            x_offset,
        }
    }

    pub fn image(&mut self) -> Option<&SlicedProtocol> {
        if self.image.is_none() {
            let source = self.source.take()?;
            let resize = Resize::Fit(Some(FilterType::CatmullRom));
            self.image =
                SlicedProtocol::new_with_resize(&IMAGE_PICKER, source, self.size, resize).ok();
        }
        self.image.as_ref()
    }

    pub fn set_scroll_offset(&mut self, offset: u16) {
        self.scroll_offset = offset;
    }

    #[must_use]
    pub fn scroll_offset(&self) -> u16 {
        self.scroll_offset
    }

    #[must_use]
    pub fn y_offset(&self) -> u16 {
        self.y_offset
    }

    #[must_use]
    pub fn x_offset(&self) -> u16 {
        self.x_offset
    }

    #[must_use]
    pub fn height(&self) -> u16 {
        self.height
    }
}

impl ComponentProps for ImageComponent {
    fn height(&self) -> u16 {
        self.height
    }

    fn set_y_offset(&mut self, y_offset: u16) {
        self.y_offset = y_offset;
    }

    fn set_scroll_offset(&mut self, scroll: u16) {
        self.scroll_offset = scroll;
    }

    fn kind(&self) -> TextNode {
        TextNode::Image
    }
}

#[cfg(test)]
mod tests {
    use super::{names_kitty_graphics_terminal, parse_cell_size};

    #[test]
    fn recognises_kitty_graphics_terminals_from_tmux_client_names() {
        assert!(names_kitty_graphics_terminal("xterm-ghostty ghostty 1.3.1"));
        assert!(names_kitty_graphics_terminal("xterm-kitty kitty(0.40)"));
        assert!(!names_kitty_graphics_terminal("xterm-256color XTerm(388)"));
        assert!(!names_kitty_graphics_terminal(""));
    }

    #[test]
    fn parses_cell_sizes() {
        assert_eq!(parse_cell_size("10x20"), Some((10, 20)));
        assert_eq!(parse_cell_size(" 9 x 18 "), Some((9, 18)));
        assert_eq!(parse_cell_size("0x20"), None);
        assert_eq!(parse_cell_size("wide"), None);
    }

    use image::DynamicImage;

    use super::ImageComponent;

    #[test]
    fn image_protocol_is_initialized_once_at_a_stable_size() {
        let source = DynamicImage::new_rgba8(1800, 1200);
        let mut component = ImageComponent::new(source, 120, "synthetic plot");
        let height = component.height;

        assert!(height > 0);
        assert!(component.source.is_some());
        assert!(component.image.is_none());

        let first = component.image().unwrap() as *const _;
        let second = component.image().unwrap() as *const _;

        assert_eq!(first, second);
        assert_eq!(component.height, height);
        assert!(component.source.is_none());
    }
}
