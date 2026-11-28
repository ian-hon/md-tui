//! Typeset math with terminal graphics on. Its own test binary, because the image protocol is
//! chosen once per process (from the environment) the first time it's needed.

use md_tui::nodes::root::{Component, ComponentRoot};
use md_tui::parser::parse_markdown;

fn parse(md: &str) -> ComponentRoot {
    // Safe here: no other thread reads the environment before the picker is first used.
    unsafe {
        std::env::set_var("MDT_IMAGE_PROTOCOL", "kitty");
        std::env::set_var("MDT_CELL_SIZE", "10x20");
        std::env::set_var("MDT_REMEMBER_POSITION", "false");
    }
    parse_markdown(None, md, 100)
}

fn images(root: &ComponentRoot) -> Vec<(u16, u16)> {
    root.children()
        .into_iter()
        .filter_map(|c| match c {
            Component::Image(i) => Some((i.x_offset(), i.height())),
            Component::TextComponent(_) => None,
        })
        .collect()
}

fn text(root: &ComponentRoot) -> String {
    root.words()
        .iter()
        .map(|w| w.content())
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn a_formula_alone_in_its_paragraph_is_a_centred_block() {
    let root = parse("$$\\int_0^\\infty e^{-x^2}dx$$\n");
    let blocks = images(&root);
    assert_eq!(blocks.len(), 1, "blocks: {blocks:?}\ntext: {}", text(&root));
    let (x_offset, rows) = blocks[0];
    assert!(
        x_offset > 10,
        "centred in ~99 columns, got offset {x_offset}"
    );
    assert!(rows >= 2, "display style is taller than a row, got {rows}");
}

#[test]
fn a_matrix_alone_is_a_block_not_text() {
    let root = parse("$$\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}$$\n");
    assert_eq!(images(&root).len(), 1, "text: {}", text(&root));
}

#[test]
fn a_formula_in_a_sentence_stays_inline() {
    let root = parse("Euler: $e^{i\\pi} + 1 = 0$ is neat.\n");
    assert!(images(&root).is_empty());
    let t = text(&root);
    assert!(t.starts_with("Euler: "), "{t:?}");
    assert!(t.ends_with(" is neat."), "{t:?}");
    assert!(
        t.chars().any(|c| ('\u{F0000}'..='\u{FFFFD}').contains(&c)),
        "has a placeholder: {t:?}"
    );
}

#[test]
fn a_multi_line_display_block_is_one_block() {
    let root = parse("Before\n\n$$\na = b\n  + c\n$$\n\nAfter\n");
    assert_eq!(images(&root).len(), 1, "text: {}", text(&root));
}

#[test]
fn dollar_prices_are_plain_text_and_do_not_swallow_the_next_block() {
    // "$10 later.\n\n$$" used to read as one formula that closed on the next block's opening $$.
    let root = parse("It costs $5 and $10 later.\n\n$$\\pi r^2$$\n");
    assert_eq!(images(&root).len(), 1, "text: {}", text(&root));
    assert!(
        text(&root).contains("It costs $5 and $10 later."),
        "{:?}",
        text(&root)
    );
}

#[test]
fn a_display_formula_wider_than_the_page_is_shrunk_to_fit() {
    let wide = (0..60)
        .map(|i| format!("x_{{{i}}}"))
        .collect::<Vec<_>>()
        .join(" + ");
    let root = parse_markdown(None, &format!("$${wide}$$\n"), 60);
    let blocks = images(&root);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].0, 0, "no room to centre: {blocks:?}");
}

#[test]
fn a_formula_too_wide_to_be_inline_falls_back_to_text() {
    let wide = (0..40)
        .map(|i| format!("x_{{{i}}}"))
        .collect::<Vec<_>>()
        .join(" + ");
    let root = parse(&format!("Look: ${wide}$ done.\n"));
    assert!(
        !text(&root)
            .chars()
            .any(|c| ('\u{F0000}'..='\u{FFFFD}').contains(&c))
    );
    assert!(text(&root).contains("x₀ + x₁"), "{:?}", text(&root));
}

#[test]
fn common_multi_line_forms_are_all_blocks() {
    let forms = [
        "\\[\na = b\n\\]\n",
        "\\[ a = b \\]\n",
        "\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n",
        "\\begin{equation}\nE = mc^2\n\\end{equation}\n",
        "\\begin{multline}\na + b \\\\\n+ c\n\\end{multline}\n",
        "$$\n\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}\n$$\n",
    ];
    for md in forms {
        let root = parse(md);
        assert_eq!(images(&root).len(), 1, "{md:?} -> text: {}", text(&root));
    }
}
