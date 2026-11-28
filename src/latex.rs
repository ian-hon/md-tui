//! Turns the inside of a `$...$` span into plain Unicode text a terminal can show.
//!
//! Eg: `\frac{\alpha}{x^2} \leq \sqrt{\pi}` -> `α/x² ≤ √π`
//!
//! It covers the common cases (Greek, operators, super/subscripts, fractions, roots, accents).
//! Anything it doesn't know is left as written, so the source is still readable.

pub fn to_unicode(src: &str) -> String {
    let mut parser = Parser {
        chars: src.chars().collect(),
        pos: 0,
    };
    parser.expr(false)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    /// Reads atoms until the end, or until the `}` that closes a group.
    fn expr(&mut self, in_group: bool) -> String {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            match c {
                '}' if in_group => break,
                '^' | '_' => {
                    self.pos += 1;
                    let operand = self.arg();
                    out.push_str(&script(&operand, c == '^'));
                }
                _ => out.push_str(&self.atom()),
            }
        }
        out
    }

    /// One argument: a `{group}`, a `\command`, or a single character.
    fn arg(&mut self) -> String {
        self.skip_spaces();
        match self.peek() {
            Some('{') => self.group(),
            Some(_) => self.atom(),
            None => String::new(),
        }
    }

    fn group(&mut self) -> String {
        self.pos += 1;
        let inner = self.expr(true);
        if self.peek() == Some('}') {
            self.pos += 1;
        }
        inner
    }

    /// An optional `[...]` argument, as in `\sqrt[3]{x}`.
    fn optional_arg(&mut self) -> Option<String> {
        self.skip_spaces();
        if self.peek() != Some('[') {
            return None;
        }
        self.pos += 1;
        let start = self.pos;
        while self.peek().is_some_and(|c| c != ']') {
            self.pos += 1;
        }
        let raw: String = self.chars[start..self.pos].iter().collect();
        self.pos += 1;
        Some(to_unicode(&raw))
    }

    fn atom(&mut self) -> String {
        let Some(c) = self.peek() else {
            return String::new();
        };
        self.pos += 1;
        match c {
            '{' => {
                self.pos -= 1;
                self.group()
            }
            '\\' => self.command(),
            '~' => " ".to_string(),
            _ => c.to_string(),
        }
    }

    fn command(&mut self) -> String {
        let Some(first) = self.peek() else {
            return "\\".to_string();
        };
        if !first.is_ascii_alphabetic() {
            self.pos += 1;
            return match first {
                ',' | ';' | ':' | ' ' | '\\' => " ".to_string(),
                '!' => String::new(),
                _ => first.to_string(),
            };
        }
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        let name: String = self.chars[start..self.pos].iter().collect();

        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let num = self.arg();
                let den = self.arg();
                format!("{}/{}", wrap(&num), wrap(&den))
            }
            "sqrt" => {
                let index = self.optional_arg();
                let body = self.arg();
                let prefix = index.map(|n| script(&n, true)).unwrap_or_default();
                format!("{prefix}√{}", wrap(&body))
            }
            "text" | "textbf" | "textit" | "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathcal"
            | "operatorname" | "boldsymbol" | "bm" => self.arg(),
            "mathbb" => self.arg().chars().map(blackboard).collect(),
            "hat" => self.accent('\u{0302}'),
            "bar" | "overline" => self.accent('\u{0304}'),
            "vec" => self.accent('\u{20D7}'),
            "dot" => self.accent('\u{0307}'),
            "ddot" => self.accent('\u{0308}'),
            "tilde" => self.accent('\u{0303}'),
            "left" | "right" => {
                if self.peek() == Some('.') {
                    self.pos += 1;
                }
                String::new()
            }
            "begin" | "end" => {
                self.arg();
                String::new()
            }
            "quad" => "  ".to_string(),
            "qquad" => "    ".to_string(),
            _ if FUNCTIONS.contains(&name.as_str()) => name,
            _ => symbol(&name).map_or_else(|| format!("\\{name}"), str::to_string),
        }
    }

    fn accent(&mut self, mark: char) -> String {
        self.arg().chars().flat_map(|c| [c, mark]).collect()
    }
}

/// Operators such as `\sin` print as the plain word.
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "log", "ln", "exp", "lim", "max", "min", "det", "sup", "inf", "arcsin",
    "arccos", "arctan", "sinh", "cosh", "tanh", "gcd", "dim", "ker", "deg", "arg",
];

/// Adds parentheses unless the text is a single symbol or a number.
fn wrap(text: &str) -> String {
    let single = text.chars().filter(|c| !is_combining(*c)).count() <= 1;
    let root_of_one = text.starts_with('√') && text.chars().count() == 2;
    if single || root_of_one || text.chars().all(|c| c.is_ascii_digit()) {
        text.to_string()
    } else {
        format!("({text})")
    }
}

fn is_combining(c: char) -> bool {
    ('\u{0300}'..='\u{036F}').contains(&c) || c == '\u{20D7}'
}

/// `x^2` -> `x²`; when a character has no superscript form, `x^(a b)`.
fn script(operand: &str, sup: bool) -> String {
    let mapped: Option<String> = operand
        .chars()
        .map(|c| if sup { superscript(c) } else { subscript(c) })
        .collect();
    match mapped {
        Some(text) if !text.is_empty() => text,
        _ => format!("{}{}", if sup { '^' } else { '_' }, wrap(operand)),
    }
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' | '−' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'i' => 'ⁱ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'n' => 'ⁿ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' | '−' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        _ => return None,
    })
}

fn blackboard(c: char) -> char {
    match c {
        'R' => 'ℝ',
        'N' => 'ℕ',
        'Z' => 'ℤ',
        'Q' => 'ℚ',
        'C' => 'ℂ',
        'P' => 'ℙ',
        'H' => 'ℍ',
        _ => c,
    }
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" | "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" | "vartheta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" | "varpi" => "π",
        "rho" | "varrho" => "ρ",
        "sigma" | "varsigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" | "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "int" => "∫",
        "iint" => "∬",
        "oint" => "∮",
        "sum" => "∑",
        "prod" => "∏",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "pm" => "±",
        "mp" => "∓",
        "times" => "×",
        "div" => "÷",
        "cdot" => "·",
        "ast" | "star" => "∗",
        "circ" => "∘",
        "bullet" => "•",
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "propto" => "∝",
        "ll" => "≪",
        "gg" => "≫",
        "to" | "rightarrow" => "→",
        "leftarrow" | "gets" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" | "implies" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" | "iff" => "⇔",
        "mapsto" => "↦",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "setminus" => "∖",
        "emptyset" | "varnothing" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "nexists" => "∄",
        "neg" | "lnot" => "¬",
        "land" | "wedge" => "∧",
        "lor" | "vee" => "∨",
        "oplus" => "⊕",
        "otimes" => "⊗",
        "perp" => "⊥",
        "parallel" => "∥",
        "angle" => "∠",
        "degree" => "°",
        "ldots" | "dots" | "cdots" => "…",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "hbar" => "ℏ",
        "ell" => "ℓ",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "aleph" => "ℵ",
        "prime" => "′",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "lbrace" => "{",
        "rbrace" => "}",
        "lbrack" => "[",
        "rbrack" => "]",
        "vert" | "mid" => "|",
        "Vert" | "|" => "‖",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::to_unicode;

    #[test]
    fn greek_and_operators() {
        assert_eq!(to_unicode(r"\alpha + \beta \leq \infty"), "α + β ≤ ∞");
    }

    #[test]
    fn scripts_use_unicode_when_possible() {
        assert_eq!(to_unicode("E = mc^2"), "E = mc²");
        assert_eq!(to_unicode("x_{i+1}"), "xᵢ₊₁");
        assert_eq!(to_unicode(r"\sum_{i=0}^{n}"), "∑ᵢ₌₀ⁿ");
    }

    #[test]
    fn scripts_fall_back_to_parentheses() {
        assert_eq!(to_unicode("e^{-x^2}"), "e^(-x²)");
        assert_eq!(to_unicode(r"x^\infty"), "x^∞");
    }

    #[test]
    fn fractions_and_roots() {
        assert_eq!(to_unicode(r"\frac{a}{b}"), "a/b");
        assert_eq!(to_unicode(r"\frac{x+1}{2}"), "(x+1)/2");
        assert_eq!(to_unicode(r"\sqrt{\pi}"), "√π");
        assert_eq!(to_unicode(r"\sqrt{x+1}"), "√(x+1)");
        assert_eq!(to_unicode(r"\sqrt[3]{x}"), "³√x");
    }

    #[test]
    fn display_example() {
        assert_eq!(
            to_unicode(r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}"),
            "∫₀^∞ e^(-x²) dx = √π/2"
        );
    }

    #[test]
    fn wrappers_accents_and_unknown_commands() {
        let words = |src: &str| to_unicode(src).split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(words(r"\mathbb{R}^n \text{ and } \mathbf{v}"), "ℝⁿ and v");
        assert_eq!(to_unicode(r"\sin x"), "sin x");
        assert_eq!(to_unicode(r"\hat{x}"), "x\u{0302}");
        assert_eq!(to_unicode(r"\left( x \right)"), "( x )");
        assert_eq!(to_unicode(r"\foo + 1"), "\\foo + 1");
    }
}
