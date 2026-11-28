//! Math without terminal graphics: Unicode text. Its own test binary, because the image protocol
//! is chosen once per process (and would otherwise follow whatever terminal the tests run in).

use md_tui::parser::parse_markdown;

fn text(md: &str) -> String {
    // Safe here: no other thread reads the environment before the picker is first used.
    unsafe {
        std::env::set_var("MDT_IMAGE_PROTOCOL", "halfblocks");
        std::env::set_var("MDT_REMEMBER_POSITION", "false");
    }
    let root = parse_markdown(None, md, 80);
    root.words()
        .iter()
        .map(|w| w.content())
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn latex_becomes_unicode_and_keeps_the_space_before_it() {
    let t = text("Inline $E = mc^2$ and $\\frac{a}{b} + \\alpha^2$ here.\n");
    assert!(t.contains("Inline E = mc² and a/b + α² here."), "got: {t}");
}

#[test]
fn a_formula_alone_in_its_paragraph_is_text_too() {
    let t = text("$$\\sqrt{\\pi}$$\n");
    assert!(t.contains("√π"), "got: {t}");
}
