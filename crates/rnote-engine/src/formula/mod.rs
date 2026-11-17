//! Delimited LaTeX formulas recognized in pasted and typed text.
//!
//! Recognition is strict: only balanced `$$…$$`, `\[…\]` (display) and `$…$`, `\(…\)` (inline)
//! with a non-empty body are formulas. Unbalanced delimiters and empty bodies stay literal text,
//! and an inline formula never spans a line break.

use rnote_compose::Color;
use serde::{Deserialize, Serialize};

/// A formula recognized in text.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    /// The LaTeX source, trimmed.
    pub latex: String,
    /// The delimited text as it appeared, used when the source does not typeset.
    pub raw: String,
    /// Whether the formula was set inline (`$…$`, `\(…\)`) instead of as a display block.
    pub inline: bool,
}

/// A piece of text, either plain text or a recognized formula.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Text(String),
    Formula(Formula),
}

/// A formula a vector image was rendered from, kept so it can be edited and re-rendered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FormulaSource {
    #[serde(rename = "latex")]
    pub latex: String,
    #[serde(rename = "font_size")]
    pub font_size: f64,
    #[serde(rename = "color")]
    pub color: Color,
    /// Whether the image is set inline with the text instead of as a display block.
    #[serde(rename = "inline")]
    pub inline: bool,
    /// The distance from the image top to the formula baseline, in document units.
    #[serde(rename = "baseline")]
    pub baseline: f64,
    /// The image height, in document units.
    #[serde(rename = "height")]
    pub height: f64,
}

impl Default for FormulaSource {
    fn default() -> Self {
        Self {
            latex: String::default(),
            font_size: 32.0,
            color: Color::default(),
            inline: false,
            baseline: 0.0,
            height: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Delimiter {
    open: &'static str,
    close: &'static str,
    inline: bool,
}

const DELIMITERS: [Delimiter; 4] = [
    Delimiter {
        open: "\\[",
        close: "\\]",
        inline: false,
    },
    Delimiter {
        open: "$$",
        close: "$$",
        inline: false,
    },
    Delimiter {
        open: "\\(",
        close: "\\)",
        inline: true,
    },
    Delimiter {
        open: "$",
        close: "$",
        inline: true,
    },
];

fn next_delimiter(text: &str, from: usize) -> Option<(usize, Delimiter)> {
    let bytes = text.as_bytes();
    let mut at = from;
    while at < text.len() {
        if !text.is_char_boundary(at) {
            at += 1;
            continue;
        }
        for delimiter in DELIMITERS {
            if text[at..].starts_with(delimiter.open) {
                let escaped = delimiter.open.starts_with('$') && at > 0 && bytes[at - 1] == b'\\';
                if !escaped {
                    return Some((at, delimiter));
                }
            }
        }
        at += text[at..].chars().next().map_or(1, |c| c.len_utf8());
    }
    None
}

/// Find the closing delimiter, ignoring escaped `$` and, inline, a doubled `$$`.
fn find_close(text: &str, from: usize, delimiter: Delimiter) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut at = from;
    while at < text.len() {
        if !text.is_char_boundary(at) {
            at += 1;
            continue;
        }
        if text[at..].starts_with(delimiter.close) {
            let escaped = delimiter.close.starts_with('$') && at > 0 && bytes[at - 1] == b'\\';
            if !escaped {
                if delimiter.close == "$" && text[at..].starts_with("$$") {
                    at += 2;
                    continue;
                }
                return Some(at);
            }
        }
        at += text[at..].chars().next().map_or(1, |c| c.len_utf8());
    }
    None
}

/// Split text into plain text and recognized formulas, preserving order.
pub fn split_text(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();

    // `cursor` is where the not-yet-emitted literal text starts, `search_from` where the next
    // opening delimiter is looked for; an empty formula skips its delimiters but keeps them literal.
    let mut cursor = 0;
    let mut search_from = 0;

    while let Some((start, delimiter)) = next_delimiter(text, search_from) {
        let body_start = start + delimiter.open.len();
        let Some(close) = find_close(text, body_start, delimiter) else {
            // Unbalanced opening delimiter: the rest of the text stays literal.
            break;
        };
        let body = &text[body_start..close];
        let end = close + delimiter.close.len();

        if body.trim().is_empty() || (delimiter.inline && body.contains('\n')) {
            // Not a formula: keep the delimiter literal and keep scanning after the opening one.
            search_from = body_start;
            continue;
        }

        if start > cursor {
            segments.push(Segment::Text(text[cursor..start].to_string()));
        }
        segments.push(Segment::Formula(Formula {
            latex: body.trim().to_string(),
            raw: text[start..end].to_string(),
            inline: delimiter.inline,
        }));
        cursor = end;
        search_from = end;
    }

    if cursor < text.len() {
        segments.push(Segment::Text(text[cursor..].to_string()));
    } else if segments.is_empty() && text.is_empty() {
        return segments;
    }

    segments
}

/// A typeset formula: its SVG document plus the metrics needed to align it with text.
#[derive(Debug, Clone)]
pub struct RenderedFormula {
    /// The standalone SVG document.
    pub svg: String,
    /// The distance from the image top to the formula baseline, in document units.
    pub baseline: f64,
    /// The image height, in document units.
    pub height: f64,
}

/// Typeset a LaTeX display formula to a standalone SVG document, in text document units.
///
/// The SVG is rendered with glyphs as paths, so `librsvg` (canvas rendering and PDF/SVG export)
/// needs no TeX fonts installed.
#[cfg(feature = "latex")]
pub fn render_svg(latex: &str, font_size: f64, color: Color) -> anyhow::Result<String> {
    Ok(render_formula(
        latex,
        font_size,
        color,
        ratex_types::math_style::MathStyle::Display,
    )?
    .svg)
}

/// Typeset a LaTeX inline formula, with the metrics needed to sit it on a text baseline.
#[cfg(feature = "latex")]
pub fn render_inline_svg(
    latex: &str,
    font_size: f64,
    color: Color,
) -> anyhow::Result<RenderedFormula> {
    render_formula(
        latex,
        font_size,
        color,
        ratex_types::math_style::MathStyle::Text,
    )
}

#[cfg(feature = "latex")]
fn render_formula(
    latex: &str,
    font_size: f64,
    color: Color,
    style: ratex_types::math_style::MathStyle,
) -> anyhow::Result<RenderedFormula> {
    use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
    use ratex_parser::parser::parse;
    use ratex_svg::{SvgColorSyntax, SvgOptions, render_to_svg_with_color_syntax};
    use ratex_types::color::Color as RatexColor;

    // The generated document measures in `pt`; usvg (and therefore the vector image) measures in
    // `px`. Scale down so one document unit ends up as one SVG unit.
    const PT_PER_PX: f64 = 0.75;

    let ast = parse(latex)?;
    let layout_options = LayoutOptions::default()
        .with_style(style)
        .with_color(RatexColor::new(
            color.r as f32,
            color.g as f32,
            color.b as f32,
            color.a as f32,
        ));
    let lbox = layout(&ast, &layout_options);
    validate_layout(&lbox).map_err(|err| anyhow::anyhow!("{err}"))?;
    let display_list = to_display_list(&lbox);

    let svg_options = SvgOptions {
        font_size: font_size * PT_PER_PX,
        padding: 0.0,
        stroke_width: 1.5 * PT_PER_PX,
        embed_glyphs: true,
        font_dir: String::new(),
    };

    Ok(RenderedFormula {
        svg: render_to_svg_with_color_syntax(&display_list, &svg_options, SvgColorSyntax::Rgba),
        baseline: lbox.height * font_size,
        height: (lbox.height + lbox.depth) * font_size,
    })
}

/// Typeset a LaTeX display formula, or fail when built without the `latex` feature.
#[cfg(not(feature = "latex"))]
pub fn render_svg(_latex: &str, _font_size: f64, _color: Color) -> anyhow::Result<String> {
    anyhow::bail!("rnote was built without the `latex` feature")
}

/// Typeset a LaTeX inline formula, or fail when built without the `latex` feature.
#[cfg(not(feature = "latex"))]
pub fn render_inline_svg(
    _latex: &str,
    _font_size: f64,
    _color: Color,
) -> anyhow::Result<RenderedFormula> {
    anyhow::bail!("rnote was built without the `latex` feature")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(segments: &[Segment]) -> Vec<&str> {
        segments
            .iter()
            .map(|s| match s {
                Segment::Text(t) => t.as_str(),
                Segment::Formula(f) => f.raw.as_str(),
            })
            .collect()
    }

    #[test]
    fn plain_text_stays_one_segment() {
        assert_eq!(
            split_text("no math here"),
            vec![Segment::Text("no math here".into())]
        );
    }

    #[test]
    fn display_brackets_split_into_three_segments() {
        let segments = split_text("before \\[x^2\\] after");
        assert_eq!(
            segments,
            vec![
                Segment::Text("before ".into()),
                Segment::Formula(Formula {
                    latex: "x^2".into(),
                    raw: "\\[x^2\\]".into(),
                    inline: false,
                }),
                Segment::Text(" after".into()),
            ]
        );
    }

    #[test]
    fn round_brackets_are_an_inline_formula() {
        let segments = split_text("вектор \\(w\\) проверен");
        assert_eq!(
            segments,
            vec![
                Segment::Text("вектор ".into()),
                Segment::Formula(Formula {
                    latex: "w".into(),
                    raw: "\\(w\\)".into(),
                    inline: true,
                }),
                Segment::Text(" проверен".into()),
            ]
        );
    }

    #[test]
    fn double_dollar_is_a_display_formula() {
        let segments = split_text("$$\\alpha$$");
        assert_eq!(segments.len(), 1);
        assert!(matches!(&segments[0], Segment::Formula(f) if f.latex == "\\alpha" && !f.inline));
    }

    #[test]
    fn dollars_are_an_inline_formula() {
        let segments = split_text("x $\\alpha$ y");
        assert_eq!(texts(&segments), vec!["x ", "$\\alpha$", " y"]);
        assert!(matches!(&segments[1], Segment::Formula(f) if f.inline && f.latex == "\\alpha"));
    }

    #[test]
    fn double_dollars_are_not_split_as_inline() {
        let segments = split_text("a $$x$$ b");
        assert_eq!(texts(&segments), vec!["a ", "$$x$$", " b"]);
        assert!(matches!(&segments[1], Segment::Formula(f) if !f.inline));
    }

    #[test]
    fn multiple_formulas_keep_order() {
        let segments = split_text("a \\[1\\] b $$2$$ c");
        assert_eq!(
            texts(&segments),
            vec!["a ", "\\[1\\]", " b ", "$$2$$", " c"]
        );
    }

    #[test]
    fn inline_formula_does_not_span_a_line_break() {
        assert_eq!(
            split_text("\\(x\ny\\)"),
            vec![Segment::Text("\\(x\ny\\)".into())]
        );
    }

    #[test]
    fn whitespace_inline_formula_stays_literal() {
        assert_eq!(split_text("$ $"), vec![Segment::Text("$ $".into())]);
    }

    #[test]
    fn unbalanced_opening_stays_literal() {
        assert_eq!(
            split_text("before \\[x^2"),
            vec![Segment::Text("before \\[x^2".into())]
        );
    }

    #[test]
    fn empty_formula_stays_literal() {
        assert_eq!(
            split_text("\\[  \\]"),
            vec![Segment::Text("\\[  \\]".into())]
        );
    }

    #[test]
    fn escaped_dollars_stay_literal() {
        assert_eq!(
            split_text("costs \\$5 here"),
            vec![Segment::Text("costs \\$5 here".into())]
        );
    }

    #[cfg(feature = "latex")]
    #[test]
    fn rendered_svg_uses_paths() {
        let svg = render_svg("\\alpha(1,0)+\\beta(0,1)", 32.0, Color::BLACK).unwrap();
        assert!(svg.starts_with("<svg"), "unexpected svg: {svg}");
        assert!(svg.contains("<path"), "svg has no glyph paths: {svg}");
        assert!(
            !svg.contains("<text"),
            "svg still uses text elements: {svg}"
        );
    }

    #[cfg(feature = "latex")]
    #[test]
    fn inline_render_reports_a_baseline_inside_the_image() {
        let rendered = render_inline_svg("y_p", 32.0, Color::BLACK).unwrap();
        assert!(rendered.svg.contains("<path"));
        assert!(rendered.baseline > 0.0);
        assert!(
            rendered.height > rendered.baseline,
            "the subscript descends below the baseline, height {} baseline {}",
            rendered.height,
            rendered.baseline
        );
    }
}
