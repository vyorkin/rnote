// Imports
use crate::pens::pensconfig::rulerconfig::RulerConfig;
use crate::store::chrono_comp::StrokeLayer;
use rand::{RngExt, SeedableRng};
use rnote_compose::Style;
use rnote_compose::builders::PenPathBuilderType;
use rnote_compose::style::PressureCurve;
use rnote_compose::style::smooth::{LineCap, LineStyle, SmoothOptions};
use rnote_compose::style::textured::TexturedOptions;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
    num_derive::FromPrimitive,
    num_derive::ToPrimitive,
)]
#[serde(rename = "brush_style")]
pub enum BrushStyle {
    #[serde(rename = "marker")]
    Marker = 0,
    #[serde(rename = "solid")]
    Solid,
    #[serde(rename = "textured")]
    Textured,
    #[serde(rename = "fountain")]
    Fountain,
}

impl Default for BrushStyle {
    fn default() -> Self {
        Self::Solid
    }
}

impl TryFrom<u32> for BrushStyle {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        num_traits::FromPrimitive::from_u32(value).ok_or_else(|| {
            anyhow::anyhow!("BrushStyle try_from::<u32>() for value {} failed", value)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "marker_options")]
pub struct MarkerOptions(SmoothOptions);

impl Default for MarkerOptions {
    fn default() -> Self {
        let mut options = SmoothOptions::default();
        options.pressure_curve = PressureCurve::Const;
        options.stroke_width = 12.0;

        Self(options)
    }
}

impl std::ops::Deref for MarkerOptions {
    type Target = SmoothOptions;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for MarkerOptions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "solid_options")]
pub struct SolidOptions(SmoothOptions);

impl Default for SolidOptions {
    fn default() -> Self {
        Self(SmoothOptions::default())
    }
}

impl std::ops::Deref for SolidOptions {
    type Target = SmoothOptions;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for SolidOptions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The options of the fountain pen.
///
/// The width of a fountain stroke varies with the pressure and the speed of the pen, and tapers
/// off at both ends. The resulting width is what the stroke stores per point, so the smooth style
/// that draws it applies a linear pressure curve to it, and the stroke width is the widest the
/// stroke can get.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "fountain_options")]
pub struct FountainOptions {
    #[serde(rename = "smooth_options")]
    pub smooth_options: SmoothOptions,
    /// How strongly the pen pressure widens the stroke, from 0 (not at all) to 1 (fully).
    #[serde(rename = "pressure_influence")]
    pub pressure_influence: f64,
    /// How strongly the pen speed narrows the stroke, from 0 (not at all) to 1 (fully).
    #[serde(rename = "speed_influence")]
    pub speed_influence: f64,
    /// The length of the tapered ends of a stroke, in document units.
    #[serde(rename = "taper_length")]
    pub taper_length: f64,
}

impl Default for FountainOptions {
    fn default() -> Self {
        let mut smooth_options = SmoothOptions::default();
        smooth_options.stroke_width = 6.0;
        smooth_options.pressure_curve = PressureCurve::Linear;

        Self {
            smooth_options,
            pressure_influence: 0.8,
            speed_influence: 0.6,
            taper_length: 8.0,
        }
    }
}

impl std::ops::Deref for FountainOptions {
    type Target = SmoothOptions;

    fn deref(&self) -> &Self::Target {
        &self.smooth_options
    }
}

impl std::ops::DerefMut for FountainOptions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.smooth_options
    }
}

impl FountainOptions {
    /// The pen speed (in surface units per second) at which the speed halves the stroke width.
    const SPEED_HALF_WIDTH: f64 = 700.0;
    /// The narrowest width factor a point of a fountain stroke gets.
    const MIN_WIDTH_FACTOR: f64 = 0.05;

    /// The width of a stroke point relative to the stroke width of the style, from pressure and speed.
    ///
    /// Both inputs only narrow the stroke, so that the full width is the width the style is set to.
    pub(crate) fn width_factor(&self, pressure: f64, speed: f64) -> f64 {
        let pressure = pressure.clamp(0.0, 1.0);
        let pressure_part = 1.0 - self.pressure_influence.clamp(0.0, 1.0) * (1.0 - pressure);
        let speed_part = 1.0
            - self.speed_influence.clamp(0.0, 1.0)
                * (1.0 - 1.0 / (1.0 + speed.max(0.0) / Self::SPEED_HALF_WIDTH));

        (pressure_part * speed_part).clamp(Self::MIN_WIDTH_FACTOR, 1.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename = "brush_config")]
pub struct BrushConfig {
    #[serde(rename = "builder_type")]
    pub builder_type: PenPathBuilderType,
    #[serde(rename = "style")]
    pub style: BrushStyle,
    #[serde(rename = "marker_options")]
    pub marker_options: MarkerOptions,
    #[serde(rename = "solid_options")]
    pub solid_options: SolidOptions,
    #[serde(rename = "textured_options")]
    pub textured_options: TexturedOptions,
    #[serde(rename = "fountain_options")]
    pub fountain_options: FountainOptions,
    #[serde(rename = "ruler_config")]
    pub ruler_config: RulerConfig,
}

impl Default for BrushConfig {
    fn default() -> Self {
        Self {
            builder_type: PenPathBuilderType::default(),
            style: BrushStyle::default(),
            marker_options: MarkerOptions::default(),
            solid_options: SolidOptions::default(),
            textured_options: TexturedOptions::default(),
            fountain_options: FountainOptions::default(),
            ruler_config: RulerConfig::default(),
        }
    }
}

impl BrushConfig {
    pub const STROKE_WIDTH_MIN: f64 = 0.1;
    pub const STROKE_WIDTH_MAX: f64 = 500.0;

    pub(crate) fn layer_for_current_options(&self) -> StrokeLayer {
        match &self.style {
            BrushStyle::Marker => StrokeLayer::Highlighter,
            BrushStyle::Solid | BrushStyle::Textured | BrushStyle::Fountain => {
                StrokeLayer::UserLayer(0)
            }
        }
    }

    /// A new seed for new shapes
    pub(crate) fn new_style_seeds(&mut self) {
        let seed = Some(rand_pcg::Pcg64::from_rng(&mut rand::rng()).random());
        self.textured_options.seed = seed;
    }

    pub(crate) fn style_for_current_options(&self) -> Style {
        match &self.style {
            BrushStyle::Marker => {
                let MarkerOptions(options) = self.marker_options.clone();

                Style::Smooth(options)
            }
            BrushStyle::Solid => {
                let SolidOptions(options) = self.solid_options.clone();

                Style::Smooth(options)
            }
            BrushStyle::Textured => {
                let options = self.textured_options.clone();

                Style::Textured(options)
            }
            BrushStyle::Fountain => {
                // The width of every point is already stored in its pressure, so the curve that
                // maps pressure to width must not reshape it.
                let mut options = self.fountain_options.smooth_options.clone();
                options.pressure_curve = PressureCurve::Linear;

                Style::Smooth(options)
            }
        }
    }

    /// The full width a stroke of the current brush style is drawn with.
    pub fn stroke_width(&self) -> f64 {
        match &self.style {
            BrushStyle::Marker => self.marker_options.stroke_width,
            BrushStyle::Solid => self.solid_options.stroke_width,
            BrushStyle::Textured => self.textured_options.stroke_width,
            BrushStyle::Fountain => self.fountain_options.smooth_options.stroke_width,
        }
    }

    /// The line style of the current brush style, if it has one.
    ///
    /// Textured strokes have none: they are drawn as a texture, not as a line.
    /// A line style only shows on the shape a stroke is recognized into, because a freehand
    /// stroke is drawn as a filled outline of varying width, which has no dashes.
    pub fn line_style(&self) -> Option<LineStyle> {
        match &self.style {
            BrushStyle::Marker => Some(self.marker_options.line_style),
            BrushStyle::Solid => Some(self.solid_options.line_style),
            BrushStyle::Textured => None,
            BrushStyle::Fountain => Some(self.fountain_options.smooth_options.line_style),
        }
    }

    /// Sets the line style of the current brush style.
    ///
    /// It takes effect when a stroke is recognized into a shape, see [`Self::line_style`].
    pub fn update_line_style(&mut self, line_style: LineStyle) {
        match &mut self.style {
            BrushStyle::Marker => self.marker_options.update_line_style(line_style),
            BrushStyle::Solid => self.solid_options.update_line_style(line_style),
            BrushStyle::Textured => (),
            BrushStyle::Fountain => self.fountain_options.update_line_style(line_style),
        }
    }

    /// The width a stroke of the current brush style is drawn with at the given pen pressure.
    ///
    /// This is what the stroke is made of: it is as wide as this at every point, for the pressure
    /// the pen had there. Input without pressure reports the default pressure, which with most
    /// curves is a good deal less than the full width of the style.
    pub(crate) fn stroke_width_for_pressure(&self, pressure: f64) -> f64 {
        match &self.style {
            BrushStyle::Marker => self
                .marker_options
                .pressure_curve
                .apply(self.marker_options.stroke_width, pressure),
            BrushStyle::Solid => self
                .solid_options
                .pressure_curve
                .apply(self.solid_options.stroke_width, pressure),
            BrushStyle::Textured => self
                .textured_options
                .pressure_curve
                .apply(self.textured_options.stroke_width, pressure),
            // A fountain stroke stores its relative width in the pressure of every point, which
            // the linear pressure curve of the style maps onto the stroke width.
            BrushStyle::Fountain => {
                self.fountain_options.smooth_options.stroke_width * pressure.clamp(0.0, 1.0)
            }
        }
    }

    /// The style for a shape that replaces a stroke, with the given stroke width.
    ///
    /// Always a smooth style, because shapes can't be composed with a textured style.
    /// A shape has one width all the way round. It is given the width the stroke it replaces had
    /// on average, which is what `stroke_width_for_pressure` gives for each point of it, so that
    /// the shape looks like the stroke did: as thick as the pen pressed it, and not as thick as
    /// the style would draw it at full pressure.
    pub(crate) fn style_for_recognized_shape(&self, stroke_width: f64) -> Style {
        let mut options = match &self.style {
            BrushStyle::Marker => {
                let MarkerOptions(options) = self.marker_options.clone();

                options
            }
            BrushStyle::Solid => {
                let SolidOptions(options) = self.solid_options.clone();

                options
            }
            BrushStyle::Textured => {
                let mut options = SmoothOptions::default();
                options.stroke_color = self.textured_options.stroke_color;
                options.pressure_curve = PressureCurve::Const;

                options
            }
            BrushStyle::Fountain => {
                let mut options = self.fountain_options.smooth_options.clone();
                options.pressure_curve = PressureCurve::Const;

                options
            }
        };
        options.stroke_width = stroke_width.max(Self::STROKE_WIDTH_MIN);
        // The dashes of a line style are measured in widths, so the stroke style follows the width,
        // which setting the line cap does.
        options.update_line_cap(LineCap::Rounded);

        Style::Smooth(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rnote_compose::penpath::Element;

    fn solid_config(curve: PressureCurve) -> BrushConfig {
        let mut config = BrushConfig {
            style: BrushStyle::Solid,
            ..BrushConfig::default()
        };
        config.solid_options.pressure_curve = curve;

        config
    }

    #[test]
    fn a_shape_is_as_wide_as_the_stroke_it_replaces_was() {
        let config = solid_config(PressureCurve::Linear);

        for pressure in [0.2, 0.5, 0.8, 1.0] {
            let drawn = config.stroke_width_for_pressure(pressure);
            let shape = config.style_for_recognized_shape(drawn).stroke_width();

            assert!(
                (shape - drawn).abs() < 1e-9,
                "pressure {pressure}: drawn {drawn}, shape {shape}"
            );
        }
    }

    #[test]
    fn a_stroke_is_thinner_than_the_style_unless_the_pen_presses_fully() {
        // The width of a stroke is the width of the style times the pressure, so one drawn with
        // the default pressure of input that has none is half as wide as the style, and the shape
        // that replaces it must not come out as wide as the style.
        let config = solid_config(PressureCurve::Linear);

        let drawn = config.stroke_width_for_pressure(Element::PRESSURE_DEFAULT);

        assert!((drawn - config.solid_options.stroke_width * 0.5).abs() < 1e-9);
        assert!(drawn < config.solid_options.stroke_width);
    }

    #[test]
    fn a_brush_that_ignores_pressure_draws_the_width_of_its_style() {
        let config = solid_config(PressureCurve::Const);

        for pressure in [0.1, 0.5, 1.0] {
            let width = config.stroke_width_for_pressure(pressure);
            assert!((width - config.solid_options.stroke_width).abs() < 1e-9);
        }
    }

    #[test]
    fn the_textured_brush_follows_its_own_pressure_curve() {
        let mut config = BrushConfig {
            style: BrushStyle::Textured,
            ..BrushConfig::default()
        };
        config.textured_options.pressure_curve = PressureCurve::Linear;

        let hard = config.stroke_width_for_pressure(1.0);
        let soft = config.stroke_width_for_pressure(0.2);

        assert!(hard > soft, "hard {hard}, soft {soft}");
        assert!((hard - config.textured_options.stroke_width).abs() < 1e-9);
    }

    #[test]
    fn the_marker_follows_its_own_pressure_curve() {
        let mut config = BrushConfig {
            style: BrushStyle::Marker,
            ..BrushConfig::default()
        };
        config.marker_options.pressure_curve = PressureCurve::Linear;

        assert!(config.stroke_width_for_pressure(1.0) > config.stroke_width_for_pressure(0.3));
    }

    #[test]
    fn a_shape_never_gets_thinner_than_the_least_a_stroke_can_be() {
        let config = solid_config(PressureCurve::Linear);

        let width = config.style_for_recognized_shape(0.0).stroke_width();

        assert_eq!(width, BrushConfig::STROKE_WIDTH_MIN);
    }

    #[test]
    fn the_shape_keeps_the_round_ends_of_the_strokes_it_replaces() {
        let config = solid_config(PressureCurve::Linear);

        let Style::Smooth(options) = config.style_for_recognized_shape(2.0) else {
            panic!("expected smooth options");
        };

        assert_eq!(options.line_cap, LineCap::Rounded);
    }

    #[test]
    fn a_recognized_shape_takes_the_line_style_of_the_brush() {
        let mut config = solid_config(PressureCurve::Const);
        config.update_line_style(LineStyle::Dotted);

        let Style::Smooth(options) = config.style_for_recognized_shape(2.0) else {
            panic!("expected smooth options");
        };

        assert_eq!(options.line_style, LineStyle::Dotted);
    }

    #[test]
    fn a_line_style_belongs_to_the_brush_style_that_is_current() {
        let mut config = solid_config(PressureCurve::Const);
        config.update_line_style(LineStyle::DashedEquidistant);

        assert_eq!(config.line_style(), Some(LineStyle::DashedEquidistant));
        assert_eq!(
            config.marker_options.line_style,
            LineStyle::Solid,
            "setting the line style of the solid brush must leave the marker alone"
        );

        config.style = BrushStyle::Fountain;
        config.update_line_style(LineStyle::Dotted);

        assert_eq!(
            config.fountain_options.smooth_options.line_style,
            LineStyle::Dotted
        );
        assert_eq!(config.solid_options.line_style, LineStyle::DashedEquidistant);

        // A textured stroke is a texture, not a line, so it has no line style to set.
        config.style = BrushStyle::Textured;
        config.update_line_style(LineStyle::DashedWide);

        assert_eq!(config.line_style(), None);
    }

    fn fountain_config() -> BrushConfig {
        BrushConfig {
            style: BrushStyle::Fountain,
            ..BrushConfig::default()
        }
    }

    #[test]
    fn the_fountain_pen_narrows_with_speed_and_widens_with_pressure() {
        let options = FountainOptions::default();

        let slow = options.width_factor(0.5, 0.0);
        let fast = options.width_factor(0.5, 2000.0);
        assert!(fast < slow, "fast {fast} is not thinner than slow {slow}");

        let lightly = options.width_factor(0.2, 500.0);
        let hard = options.width_factor(1.0, 500.0);
        assert!(
            hard > lightly,
            "hard {hard} is not thicker than light {lightly}"
        );

        // A rested pen at full pressure draws the full width of the style.
        assert!((options.width_factor(1.0, 0.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_fountain_pen_draws_the_width_stored_in_the_point_pressure() {
        let config = fountain_config();
        let width = config.fountain_options.stroke_width;

        for stored_factor in [0.05, 0.3, 0.75, 1.0] {
            let drawn = config.stroke_width_for_pressure(stored_factor);
            assert!((drawn - width * stored_factor).abs() < 1e-9);
        }
    }

    #[test]
    fn the_fountain_style_maps_the_stored_width_factor_linearly() {
        let config = fountain_config();

        let Style::Smooth(options) = config.style_for_current_options() else {
            panic!("expected smooth options");
        };

        assert!(matches!(options.pressure_curve, PressureCurve::Linear));
        assert!((options.stroke_width - config.fountain_options.stroke_width).abs() < 1e-9);
    }

    #[test]
    fn the_fountain_shape_is_as_wide_as_the_stroke_it_replaces_was() {
        let config = fountain_config();

        for stored_factor in [0.1, 0.5, 1.0] {
            let drawn = config.stroke_width_for_pressure(stored_factor);
            let shape = config.style_for_recognized_shape(drawn).stroke_width();

            assert!(
                (shape - drawn).abs() < 1e-9,
                "factor {stored_factor}: drawn {drawn}, shape {shape}"
            );
        }
    }
}
