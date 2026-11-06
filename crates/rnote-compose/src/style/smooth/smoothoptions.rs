// Imports
use crate::Color;
use crate::style::PressureCurve;
use anyhow::Context;
use num_derive::{FromPrimitive, ToPrimitive};
use serde::{Deserialize, Serialize};
use std::{f64, ops::AddAssign};

/// Options for shapes that can be drawn in a smooth style. Ensure the precursor struct used in deserialization matches this one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename = "smooth_options")]
pub struct SmoothOptions {
    /// Stroke width.
    #[serde(rename = "stroke_width", with = "crate::serialize::f64_dp3")]
    pub stroke_width: f64,
    /// Stroke color. When set to None, the stroke outline is not drawn.
    #[serde(rename = "stroke_color")]
    pub stroke_color: Option<Color>,
    /// Fill color. When set to None, the fill is not drawn.
    #[serde(rename = "fill_color")]
    pub fill_color: Option<Color>,
    /// Pressure curve.
    #[serde(rename = "pressure_curve")]
    pub pressure_curve: PressureCurve,
    /// Line style.
    #[serde(rename = "line_style")]
    pub line_style: LineStyle,
    /// The length of a dash, in multiples of the stroke width.
    ///
    /// Unused by a dotted line, whose dashes are the round ends of the stroke itself.
    #[serde(rename = "dash_length", with = "crate::serialize::f64_dp3")]
    pub dash_length: f64,
    /// The space between two dashes or two dots, in multiples of the stroke width.
    #[serde(rename = "dash_gap", with = "crate::serialize::f64_dp3")]
    pub dash_gap: f64,
    /// Line cap.
    #[serde(rename = "line_cap")]
    pub line_cap: LineCap,
    /// The inner piet::StrokeStyle, computed using the stroke_width, line_style, and line_cap.
    #[serde(skip)]
    pub piet_stroke_style: piet::StrokeStyle,
}

impl Default for SmoothOptions {
    fn default() -> Self {
        let stroke_width: f64 = 2.0;
        let line_style = LineStyle::default();
        let dash_length = Self::DEFAULT_DASH_LENGTH;
        let dash_gap = Self::DEFAULT_DASH_GAP;
        let line_cap = LineCap::default();
        Self {
            stroke_width,
            stroke_color: Some(Color::BLACK),
            fill_color: None,
            pressure_curve: PressureCurve::default(),
            line_style,
            dash_length,
            dash_gap,
            line_cap,
            piet_stroke_style: Self::compute_piet_stroke_style(
                stroke_width,
                line_style,
                dash_length,
                dash_gap,
                line_cap,
            ),
        }
    }
}

impl SmoothOptions {
    /// The length of a dash of a line style that has not been adjusted, in stroke widths.
    pub const DEFAULT_DASH_LENGTH: f64 = 2.7;
    /// The space between dashes of a line style that has not been adjusted, in stroke widths.
    pub const DEFAULT_DASH_GAP: f64 = 2.7;
    /// The space a dotted line starts with, in stroke widths: the dots sit right next to each
    /// other, the cap of a dot is drawn around the width of the stroke.
    const DEFAULT_DOT_GAP: f64 = 0.0;

    fn compute_piet_stroke_style(
        stroke_width: f64,
        line_style: LineStyle,
        dash_length: f64,
        dash_gap: f64,
        line_cap: LineCap,
    ) -> piet::StrokeStyle {
        let mut dash_pattern = match line_style {
            LineStyle::Solid => Vec::new(),
            LineStyle::Dotted => vec![0.0, dash_gap.max(0.0) * stroke_width],
            LineStyle::Dashed => vec![
                dash_length.max(0.0) * stroke_width,
                dash_gap.max(0.0) * stroke_width,
            ],
        };
        if line_cap == LineCap::Rounded {
            // With a rounded line cap, a half disk with radius equal to the stroke width is
            // added both ends of a dash, this increases the length of each dash, and is not
            // taken into account by DashStroke, it has to be manually accounted for
            for gap in dash_pattern.iter_mut().skip(1).step_by(2) {
                gap.add_assign(2.0 * stroke_width)
            }
        }
        let mut stroke_style = piet::StrokeStyle::new();
        stroke_style.set_dash_pattern(dash_pattern);
        stroke_style.set_line_cap(line_cap.into());
        stroke_style
    }

    /// Updates the inner piet::Strokestyle
    pub fn update_piet_stroke_style(&mut self) {
        self.piet_stroke_style = Self::compute_piet_stroke_style(
            self.stroke_width,
            self.line_style,
            self.dash_length,
            self.dash_gap,
            self.line_cap,
        );
    }

    /// Updates the stroke width, and the dash pattern that is measured in it.
    pub fn update_stroke_width(&mut self, stroke_width: f64) {
        self.stroke_width = stroke_width;
        self.update_piet_stroke_style();
    }

    /// Updates the length of a dash, in multiples of the stroke width.
    pub fn update_dash_length(&mut self, dash_length: f64) {
        self.dash_length = dash_length;
        self.update_piet_stroke_style();
    }

    /// Updates the space between two dashes or two dots, in multiples of the stroke width.
    pub fn update_dash_gap(&mut self, dash_gap: f64) {
        self.dash_gap = dash_gap;
        self.update_piet_stroke_style();
    }

    /// Updates the line cap
    pub fn update_line_cap(&mut self, line_cap: LineCap) {
        // Dotted style requires a round LineCap
        if self.line_style.is_dotted() && line_cap != LineCap::Rounded {
            self.line_style = LineStyle::Solid;
        }
        self.line_cap = line_cap;
        self.update_piet_stroke_style();
    }

    /// Updates the line style
    pub fn update_line_style(&mut self, line_style: LineStyle) {
        // Dotted style requires a round LineCap
        if line_style.is_dotted() {
            self.line_cap = LineCap::Rounded;
        }
        self.line_style = line_style;
        self.update_piet_stroke_style();
    }
}

impl<'de> Deserialize<'de> for SmoothOptions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(default, rename = "smooth_options")]
        struct SmoothOptionsPrecursor {
            #[serde(rename = "stroke_width", with = "crate::serialize::f64_dp3")]
            pub stroke_width: f64,
            #[serde(rename = "stroke_color")]
            pub stroke_color: Option<Color>,
            #[serde(rename = "fill_color")]
            pub fill_color: Option<Color>,
            #[serde(rename = "pressure_curve")]
            pub pressure_curve: PressureCurve,
            #[serde(rename = "line_style")]
            pub line_style: LineStyleName,
            /// Missing in files written before the dash pattern was adjustable, and not to be
            /// taken from the defaults of this struct, which carry the default lengths.
            #[serde(rename = "dash_length", default)]
            pub dash_length: Option<f64>,
            /// Missing in files written before the dash pattern was adjustable, and not to be
            /// taken from the defaults of this struct, which carry the default lengths.
            #[serde(rename = "dash_gap", default)]
            pub dash_gap: Option<f64>,
            #[serde(rename = "line_cap")]
            pub line_cap: LineCap,
        }

        impl From<SmoothOptions> for SmoothOptionsPrecursor {
            fn from(value: SmoothOptions) -> Self {
                Self {
                    stroke_width: value.stroke_width,
                    stroke_color: value.stroke_color,
                    fill_color: value.fill_color,
                    pressure_curve: value.pressure_curve,
                    line_style: value.line_style.into(),
                    dash_length: Some(value.dash_length),
                    dash_gap: Some(value.dash_gap),
                    line_cap: value.line_cap,
                }
            }
        }

        impl Default for SmoothOptionsPrecursor {
            fn default() -> Self {
                SmoothOptions::default().into()
            }
        }

        let precursor = SmoothOptionsPrecursor::deserialize(deserializer)?;
        let (line_style, dash_length, dash_gap) = precursor
            .line_style
            .dash_pattern(precursor.dash_length, precursor.dash_gap);

        Ok(SmoothOptions {
            stroke_width: precursor.stroke_width,
            stroke_color: precursor.stroke_color,
            fill_color: precursor.fill_color,
            pressure_curve: precursor.pressure_curve,
            line_style,
            dash_length,
            dash_gap,
            line_cap: precursor.line_cap,
            piet_stroke_style: Self::compute_piet_stroke_style(
                precursor.stroke_width,
                line_style,
                dash_length,
                dash_gap,
                precursor.line_cap,
            ),
        })
    }
}

/// Line cap present at the start and end of a line
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, FromPrimitive, ToPrimitive,
)]
#[serde(rename = "line_cap")]
pub enum LineCap {
    /// Straight line cap
    #[default]
    #[serde(rename = "straight")]
    Straight,
    /// Rounded line cap
    #[serde(rename = "rounded")]
    Rounded,
}

impl TryFrom<u32> for LineCap {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        num_traits::FromPrimitive::from_u32(value)
            .with_context(|| format!("LineCap try_from::<u32>() for value {value} failed"))
    }
}

impl From<LineCap> for piet::LineCap {
    fn from(value: LineCap) -> Self {
        match value {
            LineCap::Straight => piet::LineCap::Butt,
            LineCap::Rounded => piet::LineCap::Round,
        }
    }
}

/// The overall style of the line
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, FromPrimitive, ToPrimitive,
)]
#[serde(rename = "line_style")]
pub enum LineStyle {
    /// Solid line style
    #[default]
    #[serde(rename = "solid")]
    Solid,
    /// Dotted line style, the dots are equidistant
    #[serde(rename = "dotted")]
    Dotted,
    /// Dashed line style, the length of a dash and the space after it are adjustable
    #[serde(rename = "dashed")]
    Dashed,
}

impl LineStyle {
    /// Indicates whether or not the LineStyle is dotted
    pub fn is_dotted(&self) -> bool {
        matches!(self, Self::Dotted)
    }
}

/// A line style as a file writes it: the three styles of this version, and the dash patterns
/// earlier versions chose from.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename = "line_style")]
enum LineStyleName {
    #[serde(rename = "solid")]
    Solid,
    #[serde(rename = "dotted")]
    Dotted,
    #[serde(rename = "dashed")]
    Dashed,
    /// Dash patterns earlier versions had no lengths for: the dashes have less space between
    /// them.
    #[serde(rename = "dashed_narrow")]
    DashedNarrow,
    /// Dash patterns earlier versions had no lengths for: the dashes are equidistant.
    #[serde(rename = "dashed_equidistant")]
    DashedEquidistant,
    /// Dash patterns earlier versions had no lengths for: the dashes have more space between
    /// them.
    #[serde(rename = "dashed_wide")]
    DashedWide,
}

impl From<LineStyle> for LineStyleName {
    fn from(value: LineStyle) -> Self {
        match value {
            LineStyle::Solid => Self::Solid,
            LineStyle::Dotted => Self::Dotted,
            LineStyle::Dashed => Self::Dashed,
        }
    }
}

impl LineStyleName {
    /// The line style this name stands for, with the length of a dash and the space after it.
    ///
    /// A file written before the dash pattern became adjustable carries no lengths of its own,
    /// and one written before the dash styles became adjustable at all only carries the name of
    /// the pattern it chose.
    fn dash_pattern(
        &self,
        dash_length: Option<f64>,
        dash_gap: Option<f64>,
    ) -> (LineStyle, f64, f64) {
        let dash_length = dash_length.map(|value| value.max(0.0));
        let dash_gap = dash_gap.map(|value| value.max(0.0));

        match self {
            Self::Solid => (
                LineStyle::Solid,
                SmoothOptions::DEFAULT_DASH_LENGTH,
                SmoothOptions::DEFAULT_DASH_GAP,
            ),
            Self::Dashed => (
                LineStyle::Dashed,
                dash_length.unwrap_or(SmoothOptions::DEFAULT_DASH_LENGTH),
                dash_gap.unwrap_or(SmoothOptions::DEFAULT_DASH_GAP),
            ),
            // A dotted line draws no dashes, so it keeps the length it would draw them with,
            // should the style be switched back to a dashed one.
            Self::Dotted => (
                LineStyle::Dotted,
                dash_length.unwrap_or(SmoothOptions::DEFAULT_DASH_LENGTH),
                dash_gap.unwrap_or(SmoothOptions::DEFAULT_DOT_GAP),
            ),
            // The presets of earlier versions: a dash was `e` stroke widths long, and the space
            // after it was `e` times 0.618, 1.0 and 1.618 stroke widths long.
            Self::DashedNarrow => (LineStyle::Dashed, 2.7, 1.7),
            Self::DashedEquidistant => (LineStyle::Dashed, 2.7, 2.7),
            Self::DashedWide => (LineStyle::Dashed, 2.7, 4.4),
        }
    }
}

impl TryFrom<u32> for LineStyle {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        num_traits::FromPrimitive::from_u32(value)
            .with_context(|| format!("LineStyle try_from::<u32>() for value {value} failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options_from(json: &str) -> SmoothOptions {
        serde_json::from_str(json).expect("SmoothOptions from json")
    }

    #[test]
    fn a_dashed_line_written_before_the_dashes_had_lengths_keeps_its_look() {
        // The dash of every preset of earlier versions was `e` stroke widths long, the space
        // after it `e * 0.618`, `e` and `e * 1.618` widths long.
        let narrow = options_from(r#"{"line_style": "dashed_narrow"}"#);
        assert_eq!(narrow.line_style, LineStyle::Dashed);
        assert!((narrow.dash_length - 2.7).abs() < 1e-9);
        assert!((narrow.dash_gap - 1.7).abs() < 1e-9);

        let equidistant = options_from(r#"{"line_style": "dashed_equidistant"}"#);
        assert_eq!(equidistant.line_style, LineStyle::Dashed);
        assert!((equidistant.dash_gap - 2.7).abs() < 1e-9);

        let wide = options_from(r#"{"line_style": "dashed_wide"}"#);
        assert_eq!(wide.line_style, LineStyle::Dashed);
        assert!((wide.dash_gap - 4.4).abs() < 1e-9);
    }

    #[test]
    fn a_line_written_after_the_dashes_got_lengths_keeps_them() {
        let dashed =
            options_from(r#"{"line_style": "dashed", "dash_length": 4.0, "dash_gap": 1.5}"#);
        assert_eq!(dashed.line_style, LineStyle::Dashed);
        assert!((dashed.dash_length - 4.0).abs() < 1e-9);
        assert!((dashed.dash_gap - 1.5).abs() < 1e-9);

        // The dots sit right next to each other unless a gap is given.
        let dotted = options_from(r#"{"line_style": "dotted"}"#);
        assert_eq!(dotted.line_style, LineStyle::Dotted);
        assert!((dotted.dash_gap - SmoothOptions::DEFAULT_DOT_GAP).abs() < 1e-9);

        let spaced_dots = options_from(r#"{"line_style": "dotted", "dash_gap": 1.0}"#);
        assert!((spaced_dots.dash_gap - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_written_line_reads_back_as_it_was() {
        let mut written = SmoothOptions::default();
        written.update_line_style(LineStyle::Dashed);
        written.update_dash_length(3.4);
        written.update_dash_gap(1.2);
        let json = serde_json::to_string(&written).expect("SmoothOptions to json");
        let read = serde_json::from_str::<SmoothOptions>(&json).expect("SmoothOptions from json");

        assert_eq!(read.line_style, written.line_style);
        assert!((read.dash_length - written.dash_length).abs() < 1e-3);
        assert!((read.dash_gap - written.dash_gap).abs() < 1e-3);
        // The dash pattern is derived from the lengths, and is not part of the file.
        assert_eq!(
            read.piet_stroke_style.dash_pattern.to_vec(),
            written.piet_stroke_style.dash_pattern.to_vec()
        );
    }

    #[test]
    fn a_dashed_line_draws_dashes_as_long_as_it_says() {
        let options = options_from(
            r#"{"line_style": "dashed", "stroke_width": 2.0, "dash_length": 4.0, "dash_gap": 1.0, "line_cap": "straight"}"#,
        );

        assert_eq!(
            options.piet_stroke_style.dash_pattern.to_vec(),
            vec![8.0, 2.0]
        );
    }

    #[test]
    fn a_rounded_cap_lengthens_the_space_so_the_dashes_do_not_grow() {
        // A rounded cap adds a half disk of half the stroke width at either end of a dash, so the
        // space after it has to grow by the width of the stroke.
        let options = options_from(
            r#"{"line_style": "dashed", "stroke_width": 2.0, "dash_length": 4.0, "dash_gap": 1.0, "line_cap": "rounded"}"#,
        );

        assert_eq!(
            options.piet_stroke_style.dash_pattern.to_vec(),
            vec![8.0, 6.0]
        );
    }

    #[test]
    fn dots_are_the_round_ends_of_a_zero_length_dash() {
        let options = options_from(
            r#"{"line_style": "dotted", "stroke_width": 2.0, "dash_gap": 1.0, "line_cap": "rounded"}"#,
        );

        // A dot is no longer than the round cap that draws it, and the space between two dots is
        // the gap plus the stroke width that the caps at either end take up.
        assert_eq!(
            options.piet_stroke_style.dash_pattern.to_vec(),
            vec![0.0, 6.0]
        );
    }

    #[test]
    fn a_solid_line_has_no_dash_pattern() {
        let options = options_from(r#"{"line_style": "solid", "stroke_width": 2.0}"#);

        assert!(options.piet_stroke_style.dash_pattern.is_empty());
    }

    #[test]
    fn a_line_style_change_keeps_the_dash_pattern_in_step() {
        let mut options = options_from(
            r#"{"line_style": "dashed", "stroke_width": 2.0, "dash_length": 4.0, "dash_gap": 1.0, "line_cap": "straight"}"#,
        );

        options.update_dash_gap(3.0);
        assert_eq!(
            options.piet_stroke_style.dash_pattern.to_vec(),
            vec![8.0, 6.0]
        );

        options.update_stroke_width(1.0);
        assert_eq!(
            options.piet_stroke_style.dash_pattern.to_vec(),
            vec![4.0, 3.0]
        );

        options.update_line_style(LineStyle::Solid);
        assert!(options.piet_stroke_style.dash_pattern.is_empty());
    }
}
