//! Delimited LaTeX display formulas recognized in pasted and typed text.
//!
//! Recognition is strict: only balanced `$$…$$` and `\[…\]` with a non-empty body are formulas.
//! `$…$`, `\(…\)`, unbalanced delimiters and empty bodies stay literal text.

use rnote_compose::Color;
use serde::{Deserialize, Serialize};

/// A display formula recognized in text.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    /// The LaTeX source, trimmed.
    pub latex: String,
    /// The delimited text as it appeared, used when the source does not typeset.
    pub raw: String,
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
}

impl Default for FormulaSource {
    fn default() -> Self {
        Self {
            latex: String::default(),
            font_size: 32.0,
            color: Color::default(),
        }
    }
}

const DELIMITERS: [(&str, &str); 2] = [("\\[", "\\]"), ("$$", "$$")];

fn next_delimiter(text: &str, from: usize) -> Option<(usize, &'static str, &'static str)> {
    let mut best: Option<(usize, &'static str, &'static str)> = None;
    for (open, close) in DELIMITERS {
        if let Some(rel_start) = find_unescaped(text, from, open) {
            let start = from + rel_start;
            if best.is_none_or(|(best_start, _, _)| start < best_start) {
                best = Some((start, open, close));
            }
        }
    }
    best
}

/// Find `needle`, ignoring occurrences whose first byte is escaped by a preceding backslash.
///
/// Only `$$` can be escaped (`\$`); `\[` is a delimiter in its own right.
fn find_unescaped(text: &str, from: usize, needle: &str) -> Option<usize> {
    let mut search_from = from;
    while let Some(rel) = text[search_from..].find(needle) {
        let at = search_from + rel;
        let escaped = at > 0 && text.as_bytes()[at - 1] == b'\\';
        if !escaped {
            return Some(at - from);
        }
        search_from = at + needle.len();
    }
    None
}

/// Split text into plain text and recognized display formulas, preserving order.
pub fn split_text(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();

    // `cursor` is where the not-yet-emitted literal text starts, `search_from` where the next
    // opening delimiter is looked for; an empty formula skips its delimiters but keeps them literal.
    let mut cursor = 0;
    let mut search_from = 0;

    while let Some((start, open, close)) = next_delimiter(text, search_from) {
        let body_start = start + open.len();
        let Some(rel_close) = text[body_start..].find(close) else {
            // Unbalanced opening delimiter: the rest of the text stays literal.
            break;
        };
        let body = &text[body_start..body_start + rel_close];
        let end = body_start + rel_close + close.len();

        if body.trim().is_empty() {
            // An empty formula is literal text; keep scanning past the closing delimiter.
            search_from = end;
            continue;
        }

        if start > cursor {
            segments.push(Segment::Text(text[cursor..start].to_string()));
        }
        segments.push(Segment::Formula(Formula {
            latex: body.trim().to_string(),
            raw: text[start..end].to_string(),
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

/// Typeset a LaTeX formula to a standalone SVG document, in text document units.
///
/// The SVG is rendered with glyphs as paths, so `librsvg` (canvas rendering and PDF/SVG export)
/// needs no TeX fonts installed.
#[cfg(feature = "latex")]
pub fn render_svg(latex: &str, font_size: f64, color: Color) -> anyhow::Result<String> {
    use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
    use ratex_parser::parser::parse;
    use ratex_svg::{SvgColorSyntax, SvgOptions, render_to_svg_with_color_syntax};
    use ratex_types::color::Color as RatexColor;
    use ratex_types::math_style::MathStyle;

    // The generated document measures in `pt`; usvg (and therefore the vector image) measures in
    // `px`. Scale down so one document unit ends up as one SVG unit.
    const PT_PER_PX: f64 = 0.75;

    let ast = parse(latex)?;
    let layout_options = LayoutOptions::default()
        .with_style(MathStyle::Display)
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

    Ok(render_to_svg_with_color_syntax(
        &display_list,
        &svg_options,
        SvgColorSyntax::Rgba,
    ))
}

/// Typeset a LaTeX formula, or fail when built without the `latex` feature.
#[cfg(not(feature = "latex"))]
pub fn render_svg(_latex: &str, _font_size: f64, _color: Color) -> anyhow::Result<String> {
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
                    raw: "\\[x^2\\]".into()
                }),
                Segment::Text(" after".into()),
            ]
        );
    }

    #[test]
    fn double_dollar_is_a_formula() {
        let segments = split_text("$$\\alpha$$");
        assert_eq!(segments.len(), 1);
        assert!(matches!(&segments[0], Segment::Formula(f) if f.latex == "\\alpha"));
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
    fn single_dollar_and_round_brackets_stay_literal() {
        assert_eq!(
            split_text("$x$ and \\(y\\)"),
            vec![Segment::Text("$x$ and \\(y\\)".into())]
        );
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
}
