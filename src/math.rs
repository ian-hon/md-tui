//! Typesets LaTeX math into an image whose pixel size is a whole number of terminal cells.
//!
//! The pipeline is the one TFormula uses (LaTeX -> vector glyphs -> transparent PNG -> terminal
//! graphics), with RaTeX as the engine: pure Rust, so no Node or JavaScript is needed.
//!
//! Eg: `\frac{a}{b} + \alpha^2` as `Kind::Inline` -> a 9x1 cell image, baseline on the text baseline.

use image::{DynamicImage, RgbaImage, imageops};
use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
use ratex_parser::parser::parse;
use ratex_render::{RenderOptions, render_to_png};
use ratex_types::{color::Color, math_style::MathStyle};

/// Where on the cell's height the text baseline sits (0 = top, 1 = bottom).
const BASELINE: f32 = 0.8;
/// Inline formulas taller than a row are shrunk to fit; below this scale they are not worth drawing.
const MIN_INLINE_SCALE: f32 = 0.6;
/// Transparent border so italic overhangs aren't clipped by the rasteriser.
const PAD_PX: u32 = 2;
const MAX_FORMULA_LEN: usize = 4_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// In the middle of a line: exactly one cell row high.
    Inline,
    /// On its own: as many rows as it needs, scaled down to fit `max_cols`.
    Display { max_cols: u16 },
}

/// Cell size in pixels and how the formula should look.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub cell_w: u16,
    pub cell_h: u16,
    /// Formula font size relative to the cell height; ~0.9 matches the terminal's text.
    pub scale: f32,
    pub color: [u8; 3],
}

pub struct MathImage {
    pub image: DynamicImage,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MathError {
    Invalid(String),
    /// The formula needs more than one row and shrinking it to one would make it unreadable.
    TooTall,
}

/// Typesets `tex`. The image is `cols * cell_w` by `rows * cell_h` pixels, so it maps 1:1 to cells.
pub fn render(tex: &str, kind: Kind, style: Style) -> Result<MathImage, MathError> {
    if tex.trim().is_empty() || tex.len() > MAX_FORMULA_LEN {
        return Err(MathError::Invalid("empty or too long".into()));
    }
    let (cell_w, cell_h) = (
        u32::from(style.cell_w.max(1)),
        u32::from(style.cell_h.max(1)),
    );
    let em_px = (f32::from(style.cell_h) * style.scale).max(4.0);

    let math_style = match kind {
        Kind::Inline => MathStyle::Text,
        Kind::Display { .. } => MathStyle::Display,
    };
    let color = Color::parse(&format!(
        "#{:02x}{:02x}{:02x}",
        style.color[0], style.color[1], style.color[2]
    ))
    .ok_or_else(|| MathError::Invalid("bad colour".into()))?;

    let ast = parse(tex).map_err(|e| MathError::Invalid(e.to_string()))?;
    let options = LayoutOptions::default()
        .with_style(math_style)
        .with_color(color);
    let laid_out = layout(&ast, &options);
    validate_layout(&laid_out).map_err(MathError::Invalid)?;
    let list = to_display_list(&laid_out);

    let png = render_to_png(
        &list,
        &RenderOptions {
            font_size: em_px,
            padding: PAD_PX as f32,
            background_color: Color::parse("#00000000").expect("transparent is valid"),
            font_dir: String::new(),
            device_pixel_ratio: 1.0,
        },
    )
    .map_err(MathError::Invalid)?;
    let decoded = image::load_from_memory(&png)
        .map_err(|e| MathError::Invalid(e.to_string()))?
        .to_rgba8();
    // Baseline of the formula inside the image, in pixels from its top.
    let mut baseline = PAD_PX as f32 + list.height as f32 * em_px;
    // The rasteriser's side padding would look like an extra space next to the text.
    let mut ink = trim_sides(&decoded);

    match kind {
        Kind::Inline => {
            let mut shrink = 1.0_f32;
            if ink.height() > cell_h {
                shrink = cell_h as f32 / ink.height() as f32;
                if shrink < MIN_INLINE_SCALE {
                    return Err(MathError::TooTall);
                }
            }
            if shrink < 1.0 {
                ink = scaled(&ink, shrink);
                baseline *= shrink;
            }
            let cols = ink.width().div_ceil(cell_w).max(1);
            // Put the formula's baseline on the text baseline unless that pushes it out of the row.
            let wanted = (cell_h as f32 * BASELINE - baseline).round() as i64;
            let top = wanted.clamp(0, i64::from(cell_h) - i64::from(ink.height()));
            // Centre it in its cells so the slack is split between both sides.
            let left = i64::from((cols * cell_w - ink.width()) / 2);
            Ok(MathImage {
                image: place(&ink, cols * cell_w, cell_h, left, top),
                cols: cols as u16,
                rows: 1,
            })
        }
        Kind::Display { max_cols } => {
            let limit = u32::from(max_cols.max(1)) * cell_w;
            if ink.width() > limit {
                ink = scaled(&ink, limit as f32 / ink.width() as f32);
            }
            let cols = ink.width().div_ceil(cell_w).max(1);
            let rows = ink.height().div_ceil(cell_h).max(1);
            let top = i64::from((rows * cell_h - ink.height()) / 2);
            Ok(MathImage {
                image: place(&ink, cols * cell_w, rows * cell_h, 0, top),
                cols: cols as u16,
                rows: rows as u16,
            })
        }
    }
}

/// `image` without its fully transparent left and right columns.
fn trim_sides(image: &RgbaImage) -> RgbaImage {
    let has_ink = |x: u32| (0..image.height()).any(|y| image.get_pixel(x, y).0[3] > 0);
    let Some(first) = (0..image.width()).find(|&x| has_ink(x)) else {
        return image.clone();
    };
    let last = (first..image.width())
        .rev()
        .find(|&x| has_ink(x))
        .unwrap_or(first);
    imageops::crop_imm(image, first, 0, last - first + 1, image.height()).to_image()
}

fn scaled(image: &RgbaImage, factor: f32) -> RgbaImage {
    let w = ((image.width() as f32 * factor).round() as u32).max(1);
    let h = ((image.height() as f32 * factor).round() as u32).max(1);
    imageops::resize(image, w, h, imageops::FilterType::Lanczos3)
}

/// A transparent canvas with `ink` drawn at (`x`, `top`).
fn place(ink: &RgbaImage, width: u32, height: u32, x: i64, top: i64) -> DynamicImage {
    let mut canvas = RgbaImage::new(width, height);
    imageops::overlay(&mut canvas, ink, x, top);
    DynamicImage::ImageRgba8(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE: Style = Style {
        cell_w: 10,
        cell_h: 20,
        scale: 0.9,
        color: [255, 255, 255],
    };

    fn has_ink(image: &DynamicImage) -> bool {
        image.to_rgba8().pixels().any(|p| p.0[3] > 0)
    }

    #[test]
    fn inline_formula_is_one_row_of_whole_cells() {
        let m = render(r"E = mc^2", Kind::Inline, STYLE).unwrap();
        assert_eq!(m.rows, 1);
        assert_eq!(m.image.height(), 20);
        assert_eq!(m.image.width(), u32::from(m.cols) * 10);
        assert!(has_ink(&m.image));
    }

    #[test]
    fn inline_formula_uses_the_requested_colour() {
        let m = render(
            r"x",
            Kind::Inline,
            Style {
                color: [255, 0, 0],
                ..STYLE
            },
        )
        .unwrap();
        let strongest = m
            .image
            .to_rgba8()
            .pixels()
            .max_by_key(|p| p.0[3])
            .copied()
            .unwrap();
        assert_eq!(strongest.0, [255, 0, 0, strongest.0[3]]);
    }

    #[test]
    fn inline_formula_that_is_too_tall_is_refused() {
        let tall = r"\begin{pmatrix} a \\ b \\ c \\ d \end{pmatrix}";
        assert_eq!(
            render(tall, Kind::Inline, STYLE).err(),
            Some(MathError::TooTall)
        );
    }

    #[test]
    fn display_formula_takes_as_many_rows_as_it_needs() {
        let m = render(
            r"\sum_{n=1}^{\infty}\frac{1}{n^2}",
            Kind::Display { max_cols: 80 },
            STYLE,
        )
        .unwrap();
        assert!(m.rows >= 2, "rows = {}", m.rows);
        assert_eq!(m.image.height(), u32::from(m.rows) * 20);
        assert_eq!(m.image.width(), u32::from(m.cols) * 10);
    }

    #[test]
    fn display_formula_is_shrunk_to_the_available_width() {
        let long = r"a + b + c + d + e + f + g + h + i + j + k + l + m + n + o + p";
        let m = render(long, Kind::Display { max_cols: 12 }, STYLE).unwrap();
        assert!(m.cols <= 12, "cols = {}", m.cols);
    }

    #[test]
    fn unknown_commands_and_empty_input_are_errors() {
        assert!(matches!(
            render(r"\notacommand", Kind::Inline, STYLE),
            Err(MathError::Invalid(_))
        ));
        assert!(matches!(
            render("  ", Kind::Inline, STYLE),
            Err(MathError::Invalid(_))
        ));
    }
}
