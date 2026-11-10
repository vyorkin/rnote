//! Gestures that act on the canvas without changing the active tool.
//!
//! The gesture settings live outside the brush configuration: they apply to any pen that
//! draws ink, and are shared by all of them.

pub mod circle_lasso;

use rnote_compose::scribblerecognition::ScribbleRecognitionOptions;
use rnote_compose::shaperecognition::{
    BRACKET_CORNER_RADIUS_RATIO_DEFAULT, BraceStyle, ParenthesisStyle,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Settings for gestures drawn with a pen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "gesture_config")]
pub struct GestureConfig {
    /// Whether scribbling over ink erases it.
    #[serde(rename = "scribble_erase_enabled")]
    pub scribble_erase_enabled: bool,
    /// The thresholds a stroke has to pass to be recognized as a scribble.
    #[serde(rename = "scribble_recognition_options")]
    pub scribble_recognition_options: ScribbleRecognitionOptions,
    /// Whether holding the pen still at the end of a stroke replaces it with a shape.
    #[serde(rename = "shape_recognition_enabled")]
    pub shape_recognition_enabled: bool,
    /// The duration the pen has to be held still to recognize a shape.
    #[serde(rename = "shape_recognition_delay")]
    pub shape_recognition_delay: Duration,
    /// Whether a held open stroke is replaced with a smooth curve.
    #[serde(rename = "curve_recognition_enabled")]
    pub curve_recognition_enabled: bool,
    /// How a held stroke that is a round parenthesis is drawn.
    #[serde(rename = "bracket_style")]
    pub bracket_style: ParenthesisStyle,
    /// How a held stroke that is a curly brace is drawn.
    #[serde(rename = "brace_style")]
    pub brace_style: BraceStyle,
    /// The corner radius of a rounded parenthesis, relative to its height.
    #[serde(rename = "bracket_corner_radius_ratio")]
    pub bracket_corner_radius_ratio: f64,
    /// Whether a closed loop around ink selects it and switches to the selector.
    #[serde(rename = "circle_lasso_enabled")]
    pub circle_lasso_enabled: bool,
    /// Whether two-finger double taps undo and three-finger double taps redo.
    #[serde(rename = "multi_touch_undo_redo_enabled")]
    pub multi_touch_undo_redo_enabled: bool,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            scribble_erase_enabled: true,
            scribble_recognition_options: ScribbleRecognitionOptions::default(),
            shape_recognition_enabled: true,
            shape_recognition_delay: Self::SHAPE_RECOGNITION_DELAY_DEFAULT,
            curve_recognition_enabled: false,
            bracket_style: ParenthesisStyle::Rounded,
            brace_style: BraceStyle::Curly,
            bracket_corner_radius_ratio: BRACKET_CORNER_RADIUS_RATIO_DEFAULT,
            circle_lasso_enabled: true,
            multi_touch_undo_redo_enabled: true,
        }
    }
}

impl GestureConfig {
    pub const SHAPE_RECOGNITION_DELAY_MIN: Duration = Duration::from_millis(100);
    pub const SHAPE_RECOGNITION_DELAY_MAX: Duration = Duration::from_millis(2000);
    pub const SHAPE_RECOGNITION_DELAY_DEFAULT: Duration = Duration::from_millis(200);
}
