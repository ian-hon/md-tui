//! Typeset math in the document: formulas drawn as terminal images over reserved cells.
//!
//! Inline `$...$`: the paragraph gets a word of `cols` private-use characters (one per cell, all the
//! same character, which encodes the formula's id). The text layout then wraps and sizes it like any
//! word. After the frame's text is drawn, `overlay` finds those characters in the frame buffer and
//! draws the formula image over them. Finding them in the buffer, rather than in each text widget,
//! means lists, tables and quotes need no special code.
//!
//! A formula alone in its paragraph becomes a centred image block instead (`block`).
//!
//! Without terminal graphics (or with `math_images = false`) nothing here is used and the parser
//! falls back to Unicode text (see `latex.rs`).

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

use image::DynamicImage;
use ratatui::{Frame, layout::Rect};
use ratatui_image::{Image, Resize, picker::ProtocolType, protocol::Protocol};

use super::image::{IMAGE_PICKER, ImageComponent};
use crate::{
    math::{self, Kind, MathError, Style},
    util::general::GENERAL_CONFIG,
};

/// Supplementary Private Use Area-A: never in real text, and one cell wide.
const PLACEHOLDER_BASE: u32 = 0xF_0000;
/// A formula wider than this is not "inline"; it reads better as plain text.
const MAX_INLINE_COLS: u16 = 48;

struct Inline {
    cols: u16,
    image: Option<DynamicImage>,
    protocol: Option<Protocol>,
}

#[derive(Default)]
struct Registry {
    /// TeX source -> index in `entries`, or `None` if it can't be drawn inline.
    by_tex: HashMap<String, Option<usize>>,
    entries: Vec<Inline>,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Mutex::default);

/// Real terminal graphics are available and math images are not switched off.
#[must_use]
pub fn graphics_enabled() -> bool {
    GENERAL_CONFIG.math_images && IMAGE_PICKER.protocol_type() != ProtocolType::Halfblocks
}

fn style() -> Style {
    let cell = IMAGE_PICKER.font_size();
    Style {
        cell_w: cell.width.max(1),
        cell_h: cell.height.max(1),
        scale: GENERAL_CONFIG.math_scale,
        color: GENERAL_CONFIG.math_color,
    }
}

/// The text to put in a paragraph for the inline formula `tex`: one placeholder character per cell.
/// `None` when it can't be drawn as a one-row image, so the caller should use text instead.
#[must_use]
pub fn inline_placeholder(tex: &str) -> Option<String> {
    if !graphics_enabled() {
        return None;
    }
    let mut registry = REGISTRY.lock().ok()?;
    let id = match registry.by_tex.get(tex) {
        Some(known) => *known,
        None => {
            let made = math::render(tex, Kind::Inline, style())
                .ok()
                .filter(|m| m.cols <= MAX_INLINE_COLS)
                .map(|m| {
                    registry.entries.push(Inline {
                        cols: m.cols,
                        image: Some(m.image),
                        protocol: None,
                    });
                    registry.entries.len() - 1
                });
            registry.by_tex.insert(tex.to_owned(), made);
            made
        }
    }?;
    let marker = char::from_u32(PLACEHOLDER_BASE + u32::try_from(id).ok()?)?;
    Some(std::iter::repeat_n(marker, registry.entries[id].cols.into()).collect())
}

/// A formula that is alone in its paragraph, as a centred image block `width` cells wide at most.
#[must_use]
pub fn block(tex: &str, width: u16) -> Option<ImageComponent> {
    if !graphics_enabled() {
        return None;
    }
    let max_cols = width.saturating_sub(1).max(1);
    match math::render(tex, Kind::Display { max_cols }, style()) {
        Ok(m) => {
            let x_offset = max_cols.saturating_sub(m.cols) / 2;
            Some(ImageComponent::from_cells(
                m.image, m.cols, m.rows, x_offset, tex,
            ))
        }
        Err(MathError::Invalid(_) | MathError::TooTall) => None,
    }
}

fn placeholder_id(symbol: &str) -> Option<usize> {
    let mut chars = symbol.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let code = u32::from(c).checked_sub(PLACEHOLDER_BASE)?;
    (code < 0xFFFE).then_some(code as usize)
}

/// Draws every inline formula over its placeholder cells, and blanks any placeholder it can't draw.
///
/// Call once per frame, after all text has been rendered into the frame buffer.
pub fn overlay(f: &mut Frame, area: Rect) {
    let mut found: Vec<(Rect, usize)> = Vec::new();
    {
        let Ok(registry) = REGISTRY.lock() else {
            return;
        };
        let buf = f.buffer_mut();
        let area = area.intersection(buf.area);
        for y in area.top()..area.bottom() {
            let mut x = area.left();
            while x < area.right() {
                let Some(id) = placeholder_id(buf[(x, y)].symbol()) else {
                    x += 1;
                    continue;
                };
                let mut end = x + 1;
                while end < area.right() && placeholder_id(buf[(end, y)].symbol()) == Some(id) {
                    end += 1;
                }
                if let Some(entry) = registry.entries.get(id) {
                    let mut at = x;
                    while at + entry.cols <= end {
                        found.push((Rect::new(at, y, entry.cols, 1), id));
                        at += entry.cols;
                    }
                }
                for blank in x..end {
                    buf[(blank, y)].set_symbol(" ");
                }
                x = end;
            }
        }
    }

    let Ok(mut registry) = REGISTRY.lock() else {
        return;
    };
    for (rect, id) in found {
        let entry = &mut registry.entries[id];
        if entry.protocol.is_none() {
            let Some(image) = entry.image.take() else {
                continue;
            };
            entry.protocol = IMAGE_PICKER
                .new_protocol(image, rect.as_size(), Resize::Fit(None))
                .ok();
        }
        if let Some(protocol) = &entry.protocol {
            f.render_widget(Image::new(protocol), rect);
        }
    }
}

/// Whether `c` is one of our placeholder characters (and is one cell wide, as the layout assumes).
#[cfg(test)]
fn is_one_cell_placeholder(c: char) -> bool {
    use unicode_width::UnicodeWidthChar;
    placeholder_id(&c.to_string()).is_some() && UnicodeWidthChar::width(c) == Some(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_characters_are_one_cell_wide() {
        for id in [0, 1, 500, 0xFFFD] {
            let c = char::from_u32(PLACEHOLDER_BASE + id).unwrap();
            assert!(is_one_cell_placeholder(c), "id {id}");
            assert!(!c.is_whitespace(), "wrapping trims whitespace; id {id}");
        }
    }

    #[test]
    fn ordinary_text_is_not_a_placeholder() {
        assert_eq!(placeholder_id("a"), None);
        assert_eq!(placeholder_id(" "), None);
        assert_eq!(placeholder_id("é"), None);
        assert_eq!(placeholder_id("ab"), None);
    }
}
