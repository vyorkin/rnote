// Imports
use crate::Drawable;
use anyhow::Context;
use core::fmt::Debug;
use image::ImageReader;
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::glamx::DAffine2;
use p2d::math::Vector2;
use piet::RenderContext;
use rayon::prelude::*;
use rnote_compose::Transformable;
use rnote_compose::ext::{AabbExt, DAffine2Ext};
use rnote_compose::shapes::{Rectangle, Shapeable};
use serde::{Deserialize, Serialize};
use std::io::{self, Cursor};

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum ImageMemoryFormat {
    R8g8b8a8Premultiplied,
}

impl Default for ImageMemoryFormat {
    fn default() -> Self {
        Self::R8g8b8a8Premultiplied
    }
}

#[cfg(feature = "ui")]
impl TryFrom<gtk4::gdk::MemoryFormat> for ImageMemoryFormat {
    type Error = anyhow::Error;
    fn try_from(value: gtk4::gdk::MemoryFormat) -> Result<Self, Self::Error> {
        match value {
            gtk4::gdk::MemoryFormat::R8g8b8a8Premultiplied => Ok(Self::R8g8b8a8Premultiplied),
            _ => Err(anyhow::anyhow!(
                "ImageMemoryFormat try_from() gdk::MemoryFormat failed, unsupported MemoryFormat `{:?}`",
                value
            )),
        }
    }
}

#[cfg(feature = "ui")]
impl From<ImageMemoryFormat> for gtk4::gdk::MemoryFormat {
    fn from(value: ImageMemoryFormat) -> Self {
        match value {
            ImageMemoryFormat::R8g8b8a8Premultiplied => {
                gtk4::gdk::MemoryFormat::R8g8b8a8Premultiplied
            }
        }
    }
}

impl From<ImageMemoryFormat> for piet::ImageFormat {
    fn from(value: ImageMemoryFormat) -> Self {
        match value {
            ImageMemoryFormat::R8g8b8a8Premultiplied => piet::ImageFormat::RgbaPremul,
        }
    }
}

/// A bitmap image.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename = "image")]
pub struct Image {
    /// The image data.
    ///
    /// Is (de)serialized with base64 encoding.
    #[serde(rename = "data", with = "crate::utils::glib_bytes_base64")]
    pub data: glib::Bytes,
    /// The target rect in the coordinate space of the document.
    #[serde(rename = "rectangle")]
    pub rectangle: Rectangle,
    /// Width of the image data.
    #[serde(rename = "pixel_width")]
    pub pixel_width: u32,
    /// Height of the image data.
    #[serde(rename = "pixel_height")]
    pub pixel_height: u32,
    /// Memory format.
    #[serde(rename = "memory_format")]
    pub memory_format: ImageMemoryFormat,
}

impl Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("data", &String::from("{.. no debug impl ..}"))
            .field("rect", &self.rectangle)
            .field("pixel_width", &self.pixel_width)
            .field("pixel_height", &self.pixel_height)
            .field("memory_format", &self.memory_format)
            .finish()
    }
}

impl Default for Image {
    fn default() -> Self {
        Self {
            data: glib::Bytes::from_owned(Vec::new()),
            rectangle: Rectangle::default(),
            pixel_width: 0,
            pixel_height: 0,
            memory_format: ImageMemoryFormat::default(),
        }
    }
}

impl From<image::DynamicImage> for Image {
    fn from(dynamic_image: image::DynamicImage) -> Self {
        let pixel_width = dynamic_image.width();
        let pixel_height = dynamic_image.height();
        let memory_format = ImageMemoryFormat::R8g8b8a8Premultiplied;
        let data = glib::Bytes::from_owned(dynamic_image.into_rgba8().to_vec());
        let bounds = Aabb::new(
            Vector2::ZERO,
            Vector2::new(pixel_width as f64, pixel_height as f64),
        );

        Self {
            data,
            rectangle: Rectangle::from_p2d_aabb(bounds),
            pixel_width,
            pixel_height,
            memory_format,
        }
    }
}

/// Per-image adjustments.
///
/// The original image data is never modified, the adjusted image is
/// derived from it whenever it is needed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename = "image_adjustments")]
pub struct ImageAdjustments {
    /// Invert the rgb channels, keeping the alpha channel.
    #[serde(rename = "invert_colors")]
    pub invert_colors: bool,
    /// Make the background (the color sampled from the image border) transparent.
    #[serde(rename = "remove_background")]
    pub remove_background: Option<BackgroundRemoval>,
}

impl Default for ImageAdjustments {
    fn default() -> Self {
        Self {
            invert_colors: false,
            remove_background: None,
        }
    }
}

impl ImageAdjustments {
    /// True if no adjustment is enabled.
    pub fn is_default(&self) -> bool {
        !self.invert_colors && self.remove_background.is_none()
    }

    /// Applies the adjustments to the given image.
    ///
    /// The background is removed first, then the colors are inverted.
    pub fn apply(&self, image: &Image) -> Image {
        let mut adjusted = image.clone();
        if let Some(background_removal) = self.remove_background {
            adjusted = adjusted.with_transparent_background(background_removal);
        }
        if self.invert_colors {
            adjusted = adjusted.with_inverted_colors();
        }
        adjusted
    }
}

/// Options for making the background of an image transparent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename = "background_removal")]
pub struct BackgroundRemoval {
    /// Max-channel rgb distance (0..=1) that still counts as background.
    #[serde(rename = "tolerance")]
    pub tolerance: f64,
    /// Width of the alpha ramp (0..=1) after the tolerance.
    #[serde(rename = "feather")]
    pub feather: f64,
}

impl Default for BackgroundRemoval {
    fn default() -> Self {
        Self {
            tolerance: BACKGROUND_TOLERANCE_DEFAULT,
            feather: BACKGROUND_FEATHER_DEFAULT,
        }
    }
}

/// Background noise (distance of the border pixels from their median) above which
/// the background is considered to be not uniform and is not removed.
const BACKGROUND_NOISE_MAX: f64 = 0.10;
const BACKGROUND_TOLERANCE_MIN: f64 = 0.04;
const BACKGROUND_TOLERANCE_MAX: f64 = 0.20;
const BACKGROUND_TOLERANCE_DEFAULT: f64 = 0.07;
const BACKGROUND_FEATHER_DEFAULT: f64 = 0.04;

/// Returns true if the rgb channels of any pixel exceed its alpha channel.
///
/// Such data is straight alpha, even though all images are declared to be
/// premultiplied (images decoded from encoded image files are not premultiplied).
fn is_straight_alpha(data: &[u8]) -> bool {
    data.as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[3] != 255 && (px[0] > px[3] || px[1] > px[3] || px[2] > px[3]))
}

/// The rgb channels of a pixel, not premultiplied by its alpha channel.
fn straight_rgb(px: &[u8], is_straight: bool) -> [u8; 3] {
    let a = px[3];
    if is_straight || a == 0 || a == 255 {
        [px[0], px[1], px[2]]
    } else {
        let unpremultiply =
            |c: u8| -> u8 { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        [
            unpremultiply(px[0]),
            unpremultiply(px[1]),
            unpremultiply(px[2]),
        ]
    }
}

/// The max-channel distance of two rgb colors.
fn max_channel_distance(a: [u8; 3], b: [u8; 3]) -> u8 {
    let distance = |x: u8, y: u8| (x as i16 - y as i16).unsigned_abs() as u8;
    distance(a[0], b[0])
        .max(distance(a[1], b[1]))
        .max(distance(a[2], b[2]))
}

/// The texture of an [Image] together with the local bounds it covers.
///
/// A texture is bound to the pixels it was created from, and can be drawn at any affine of those
/// pixels. So transforming an image only needs a new render node around the same texture: handing
/// the pixels to gdk again is what creating a texture costs, and a translate or scale of a
/// rendering must not pay it per step.
#[cfg(feature = "ui")]
#[derive(Clone, Debug)]
pub struct ImageTexture {
    texture: gtk4::gdk::MemoryTexture,
    local_rect: gtk4::graphene::Rect,
}

#[cfg(feature = "ui")]
impl ImageTexture {
    pub fn new(image: &Image) -> Result<Self, anyhow::Error> {
        use crate::ext::GrapheneRectExt;

        image.assert_valid()?;

        Ok(Self {
            texture: gtk4::gdk::MemoryTexture::new(
                image.pixel_width as i32,
                image.pixel_height as i32,
                image.memory_format.into(),
                &image.data,
                (image.pixel_width * 4) as usize,
            ),
            local_rect: gtk4::graphene::Rect::from_p2d_aabb(image.rectangle.cuboid.local_aabb()),
        })
    }

    /// Draw the texture at the given affine of the image it was created from.
    pub fn to_rendernode(&self, affine: &DAffine2) -> gtk4::gsk::RenderNode {
        use gtk4::{gsk, prelude::*};

        let texture_node = gsk::TextureNode::new(&self.texture, &self.local_rect);
        gsk::TransformNode::new(&texture_node, Some(&crate::utils::affine_to_gsk(affine))).upcast()
    }

    /// The texture the node of this image draws.
    pub fn texture(&self) -> &gtk4::gdk::MemoryTexture {
        &self.texture
    }
}

impl Drawable for Image {
    /// Draw itself on a [piet::RenderContext].
    ///
    /// Expects image to be in rgba8-premultiplied format, else drawing will fail.
    ///
    /// `image_scale` has no meaning here, because the bitmap is already provided.
    fn draw(&self, cx: &mut impl piet::RenderContext, _image_scale: f64) -> anyhow::Result<()> {
        let piet_image_format = piet::ImageFormat::from(self.memory_format);

        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let piet_image = cx
            .make_image(
                self.pixel_width as usize,
                self.pixel_height as usize,
                &self.data,
                piet_image_format,
            )
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        cx.transform(self.rectangle.affine.to_kurbo());
        cx.draw_image(
            &piet_image,
            self.rectangle.cuboid.local_aabb().to_kurbo_rect(),
            piet::InterpolationMode::Bilinear,
        );
        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

impl Transformable for Image {
    fn translate(&mut self, offset: Vector2) {
        self.rectangle.translate(offset)
    }

    fn rotate(&mut self, angle: f64, center: Vector2) {
        self.rectangle.rotate(angle, center)
    }

    fn scale(&mut self, scale: Vector2) {
        self.rectangle.scale(scale)
    }
}

impl Image {
    pub fn assert_valid(&self) -> anyhow::Result<()> {
        self.rectangle.bounds().assert_valid()?;

        if self.pixel_width == 0
            || self.pixel_height == 0
            || self.data.len() as u32 != 4 * self.pixel_width * self.pixel_height
        {
            Err(anyhow::anyhow!(
                "Asserting image validity failed, invalid size or data."
            ))
        } else {
            Ok(())
        }
    }

    /// Samples the background color from the image border.
    ///
    /// Returns the sampled color and the border noise (0..=1), or `None` if the
    /// border is mostly transparent or not uniform.
    fn background_key(&self) -> Option<([u8; 3], f64)> {
        if self.assert_valid().is_err() {
            return None;
        }

        let width = self.pixel_width as usize;
        let height = self.pixel_height as usize;
        let data = self.data.as_ref();
        let is_straight = is_straight_alpha(data);

        let pixel_at = |x: usize, y: usize| -> [u8; 4] {
            let i = (y * width + x) * 4;
            let px = &data[i..i + 4];
            let [r, g, b] = straight_rgb(px, is_straight);
            [r, g, b, px[3]]
        };

        let mut border = Vec::with_capacity(2 * (width + height));
        for x in 0..width {
            border.push(pixel_at(x, 0));
            if height > 1 {
                border.push(pixel_at(x, height - 1));
            }
        }
        for y in 1..height.saturating_sub(1) {
            border.push(pixel_at(0, y));
            if width > 1 {
                border.push(pixel_at(width - 1, y));
            }
        }

        let border_len = border.len();
        let opaque: Vec<[u8; 4]> = border.into_iter().filter(|px| px[3] != 0).collect();
        // A mostly transparent border means the background is already transparent.
        if opaque.is_empty() || 2 * opaque.len() < border_len {
            return None;
        }

        let median = |channel: usize| -> u8 {
            let mut values: Vec<u8> = opaque.iter().map(|px| px[channel]).collect();
            values.sort_unstable();
            values[values.len() / 2]
        };
        let key = [median(0), median(1), median(2)];

        let mut distances: Vec<u8> = opaque
            .iter()
            .map(|px| max_channel_distance([px[0], px[1], px[2]], key))
            .collect();
        distances.sort_unstable();
        let noise = distances[distances.len() * 3 / 4] as f64 / 255.0;

        if noise > BACKGROUND_NOISE_MAX {
            return None;
        }

        Some((key, noise))
    }

    /// Detects whether the background can be made transparent and returns the
    /// detected options, or `None` if it can not.
    pub fn detect_background(&self) -> Option<BackgroundRemoval> {
        let (_, noise) = self.background_key()?;

        Some(BackgroundRemoval {
            tolerance: (noise * 2.0 + 5.0 / 255.0)
                .clamp(BACKGROUND_TOLERANCE_MIN, BACKGROUND_TOLERANCE_MAX),
            feather: BACKGROUND_FEATHER_DEFAULT,
        })
    }

    /// Returns a copy of the image with the background made transparent.
    ///
    /// The background color is sampled from the image border, pixels within the
    /// tolerance become fully transparent and the ones just outside it are faded
    /// out over the feather width.
    pub fn with_transparent_background(&self, options: BackgroundRemoval) -> Image {
        let Some((key, _)) = self.background_key() else {
            return self.clone();
        };

        let is_straight = is_straight_alpha(self.data.as_ref());
        let tolerance = (options.tolerance.clamp(0.0, 1.0) * 255.0).round() as i32;
        let feather = ((options.feather.clamp(0.0, 1.0) * 255.0).round() as i32).max(1);

        let mut data = self.data.as_ref().to_vec();
        data.par_chunks_exact_mut(4).for_each(|px| {
            let alpha = px[3];
            if alpha == 0 {
                return;
            }

            let distance = max_channel_distance(straight_rgb(px, is_straight), key) as i32;
            let factor = if distance <= tolerance {
                0.0f32
            } else if distance >= tolerance + feather {
                return;
            } else {
                (distance - tolerance) as f32 / feather as f32
            };

            let scale = |channel: u8| (channel as f32 * factor).round().clamp(0.0, 255.0) as u8;
            px[0] = scale(px[0]);
            px[1] = scale(px[1]);
            px[2] = scale(px[2]);
            px[3] = scale(alpha);
        });

        Image {
            data: glib::Bytes::from_owned(data),
            ..self.clone()
        }
    }

    /// Returns a copy of the image with the rgb channels inverted.
    ///
    /// The alpha channel is kept, transparent pixels stay transparent.
    pub fn with_inverted_colors(&self) -> Image {
        if self.assert_valid().is_err() {
            return self.clone();
        }

        let is_straight = is_straight_alpha(self.data.as_ref());
        let mut data = self.data.as_ref().to_vec();
        data.par_chunks_exact_mut(4).for_each(|px| {
            if is_straight {
                if px[3] == 0 {
                    return;
                }
                px[0] = 255 - px[0];
                px[1] = 255 - px[1];
                px[2] = 255 - px[2];
            } else {
                // Premultiplied: (255 - straight_color) * alpha = alpha - channel.
                px[0] = px[3].saturating_sub(px[0]);
                px[1] = px[3].saturating_sub(px[1]);
                px[2] = px[3].saturating_sub(px[2]);
            }
        });

        Image {
            data: glib::Bytes::from_owned(data),
            ..self.clone()
        }
    }

    pub fn try_from_encoded_bytes(bytes: &[u8]) -> Result<Self, anyhow::Error> {
        let reader = ImageReader::new(io::Cursor::new(bytes)).with_guessed_format()?;
        Ok(Image::from(reader.decode()?))
    }

    pub fn try_from_cairo_surface(
        mut surface: cairo::ImageSurface,
        bounds: Aabb,
    ) -> anyhow::Result<Self> {
        let width = surface.width() as u32;
        let height = surface.height() as u32;
        let data = surface.data()?.to_vec();

        Ok(Image {
            data: glib::Bytes::from_owned(convert_image_bgra_to_rgba(width, height, data)),
            rectangle: Rectangle::from_p2d_aabb(bounds),
            pixel_width: width,
            pixel_height: height,
            // cairo renders to bgra8-premultiplied, but we convert it to rgba8-premultiplied
            memory_format: ImageMemoryFormat::R8g8b8a8Premultiplied,
        })
    }

    pub fn into_imgbuf(
        self,
    ) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, anyhow::Error> {
        self.assert_valid()?;

        match self.memory_format {
            ImageMemoryFormat::R8g8b8a8Premultiplied => image::RgbaImage::from_vec(
                self.pixel_width,
                self.pixel_height,
                self.data.to_vec(),
            )
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Creating RgbaImage from data failed for image with memory-format {:?}.",
                    self.memory_format
                )
            }),
        }
    }

    /// Encodes the image into the provided format.
    ///
    /// When the format is `Jpeg`, the quality should be provided, but falls back to 93 if it is None.
    pub fn into_encoded_bytes(
        self,
        format: image::ImageFormat,
        quality: Option<u8>,
    ) -> Result<Vec<u8>, anyhow::Error> {
        const QUALITY_FALLBACK: u8 = 93;

        self.assert_valid()?;
        let mut bytes_buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let dynamic_image = image::DynamicImage::ImageRgba8(
            self.into_imgbuf()
                .context("Converting image to image::ImageBuffer failed.")?,
        );
        match format {
            image::ImageFormat::Jpeg => {
                image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut bytes_buf,
                    quality.map(|q| q.clamp(0, 100)).unwrap_or(QUALITY_FALLBACK),
                )
                .encode_image(&dynamic_image)
                .context("Encode dynamic image to jpeg failed.")?;
            }
            format => {
                dynamic_image
                    .write_to(&mut bytes_buf, format)
                    .context("Encode dynamic image to format '{format}' failed.")?;
            }
        }

        Ok(bytes_buf.into_inner())
    }

    #[cfg(feature = "ui")]
    pub fn to_memtexture(&self) -> Result<gtk4::gdk::MemoryTexture, anyhow::Error> {
        self.assert_valid()?;

        Ok(gtk4::gdk::MemoryTexture::new(
            self.pixel_width as i32,
            self.pixel_height as i32,
            self.memory_format.into(),
            &self.data,
            (self.pixel_width * 4) as usize,
        ))
    }

    #[cfg(feature = "ui")]
    pub fn to_rendernode(&self) -> Result<gtk4::gsk::RenderNode, anyhow::Error> {
        Ok(ImageTexture::new(self)?.to_rendernode(&self.rectangle.affine))
    }

    #[cfg(feature = "ui")]
    pub fn images_to_textures<'a>(
        images: impl IntoIterator<Item = &'a Self>,
    ) -> Result<Vec<ImageTexture>, anyhow::Error> {
        images.into_iter().map(ImageTexture::new).collect()
    }

    #[cfg(feature = "ui")]
    pub fn images_to_rendernodes<'a>(
        images: impl IntoIterator<Item = &'a Self>,
    ) -> Result<Vec<gtk4::gsk::RenderNode>, anyhow::Error> {
        images.into_iter().map(|img| img.to_rendernode()).collect()
    }

    /// Generates an image with a provided closure that draws onto a [cairo::Context].
    pub fn gen_with_cairo<F>(
        draw_func: F,
        mut bounds: Aabb,
        image_scale: f64,
    ) -> anyhow::Result<Self>
    where
        F: FnOnce(&cairo::Context) -> anyhow::Result<()>,
    {
        bounds.ensure_positive();
        bounds.loosen(1.0);
        bounds.assert_valid()?;

        let width_scaled = ((bounds.extents()[0]) * image_scale).round() as u32;
        let height_scaled = ((bounds.extents()[1]) * image_scale).round() as u32;

        let mut image_surface = cairo::ImageSurface::create(
            cairo::Format::ARgb32,
            width_scaled as i32,
            height_scaled as i32,
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "creating image surface with dimensions ({}, {}) failed, Err: {e:?}",
                width_scaled,
                height_scaled,
            )
        })?;

        {
            let cairo_cx = cairo::Context::new(&image_surface)?;
            cairo_cx.scale(image_scale, image_scale);
            cairo_cx.translate(-bounds.mins[0], -bounds.mins[1]);
            // Apply the draw function
            draw_func(&cairo_cx)?;
        }

        let data = image_surface
            .data()
            .map_err(|e| anyhow::anyhow!("accessing image surface data failed, Err: {e:?}"))?
            .to_vec();

        Ok(Image {
            data: glib::Bytes::from_owned(convert_image_bgra_to_rgba(
                width_scaled,
                height_scaled,
                data,
            )),
            rectangle: Rectangle::from_p2d_aabb(bounds),
            pixel_width: width_scaled,
            pixel_height: height_scaled,
            // cairo renders to bgra8-premultiplied, but we convert it to rgba8-premultiplied
            memory_format: ImageMemoryFormat::R8g8b8a8Premultiplied,
        })
    }

    /// Generates an image with a provided closure that draws onto a [piet_cairo::CairoRenderContext].
    pub fn gen_with_piet<F>(draw_func: F, bounds: Aabb, image_scale: f64) -> anyhow::Result<Self>
    where
        F: FnOnce(&mut piet_cairo::CairoRenderContext) -> anyhow::Result<()>,
    {
        let cairo_draw_fn = move |cairo_cx: &cairo::Context| -> anyhow::Result<()> {
            let mut piet_cx = piet_cairo::CairoRenderContext::new(cairo_cx);
            // Apply the draw function
            draw_func(&mut piet_cx)?;
            piet_cx
                .finish()
                .map_err(|e| anyhow::anyhow!("finishing piet context failed, Err: {e:?}"))?;
            Ok(())
        };

        Self::gen_with_cairo(cairo_draw_fn, bounds, image_scale)
    }
}

pub(super) fn convert_image_bgra_to_rgba(_width: u32, _height: u32, mut bytes: Vec<u8>) -> Vec<u8> {
    for src in bytes.as_chunks_mut::<4>().0 {
        let (blue, green, red, alpha) = (src[0], src[1], src[2], src[3]);
        src[0] = red;
        src[1] = green;
        src[2] = blue;
        src[3] = alpha;
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_image(pixels: &[[u8; 4]], width: u32) -> Image {
        assert!(width > 0 && pixels.len() % width as usize == 0);
        let height = pixels.len() as u32 / width;
        let data = pixels.iter().flatten().copied().collect::<Vec<u8>>();

        Image {
            data: glib::Bytes::from_owned(data),
            rectangle: Rectangle::from_p2d_aabb(Aabb::new(
                Vector2::ZERO,
                Vector2::new(width as f64, height as f64),
            )),
            pixel_width: width,
            pixel_height: height,
            memory_format: ImageMemoryFormat::R8g8b8a8Premultiplied,
        }
    }

    fn image_pixels(image: &Image) -> Vec<[u8; 4]> {
        image
            .data
            .chunks_exact(4)
            .map(|px| [px[0], px[1], px[2], px[3]])
            .collect()
    }

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];

    #[test]
    fn invert_opaque_colors() {
        let image = test_image(
            &[
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [10, 20, 30, 255],
            ],
            2,
        );

        assert_eq!(
            image_pixels(&image.with_inverted_colors()),
            vec![
                [0, 255, 255, 255],
                [255, 0, 255, 255],
                [255, 255, 0, 255],
                [245, 235, 225, 255]
            ]
        );
    }

    #[test]
    fn invert_premultiplied_colors_keeps_alpha_invariant() {
        let image = test_image(&[[128, 64, 32, 128]], 1);

        assert_eq!(
            image_pixels(&image.with_inverted_colors()),
            vec![[0, 64, 96, 128]]
        );
    }

    #[test]
    fn invert_transparent_pixels_stays_transparent() {
        let image = test_image(&[[0, 0, 0, 0]], 1);

        assert_eq!(
            image_pixels(&image.with_inverted_colors()),
            vec![[0, 0, 0, 0]]
        );
    }

    #[test]
    fn invert_twice_is_identity() {
        let image = test_image(&[[128, 64, 32, 128], [255, 0, 0, 255]], 2);
        let original = image_pixels(&image);

        let twice = image.with_inverted_colors().with_inverted_colors();

        assert_eq!(image_pixels(&twice), original);
    }

    #[test]
    fn invert_straight_alpha_colors() {
        // Straight alpha data (the rgb channel exceeds the alpha channel).
        let image = test_image(&[[255, 0, 0, 128]], 1);

        assert_eq!(
            image_pixels(&image.with_inverted_colors()),
            vec![[0, 255, 255, 128]]
        );
    }

    #[test]
    fn detect_background_of_uniform_border() {
        let image = test_image(
            &[WHITE, WHITE, WHITE, WHITE, RED, WHITE, WHITE, WHITE, WHITE],
            3,
        );

        let detected = image
            .detect_background()
            .expect("background should be detected");

        assert!(detected.tolerance >= BACKGROUND_TOLERANCE_MIN);
        assert!(detected.tolerance <= BACKGROUND_TOLERANCE_MAX);
    }

    #[test]
    fn remove_background_makes_uniform_border_transparent() {
        let image = test_image(
            &[WHITE, WHITE, WHITE, WHITE, RED, WHITE, WHITE, WHITE, WHITE],
            3,
        );
        let options = image.detect_background().unwrap();

        let pixels = image_pixels(&image.with_transparent_background(options));

        assert_eq!(
            pixels,
            vec![
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                RED,
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0]
            ]
        );
    }

    #[test]
    fn remove_background_skips_already_transparent_border() {
        let image = test_image(
            &[
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                RED,
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
            ],
            3,
        );

        assert_eq!(image.detect_background(), None);
    }

    #[test]
    fn remove_background_skips_non_uniform_border() {
        let image = test_image(
            &[[255, 255, 255, 255], [0, 0, 0, 255], [0, 0, 0, 255], WHITE],
            2,
        );

        assert_eq!(image.detect_background(), None);
    }

    #[test]
    fn adjustments_apply_background_before_inverting() {
        let image = test_image(
            &[WHITE, WHITE, WHITE, WHITE, RED, WHITE, WHITE, WHITE, WHITE],
            3,
        );
        let adjustments = ImageAdjustments {
            invert_colors: true,
            remove_background: image.detect_background(),
        };

        let pixels = image_pixels(&adjustments.apply(&image));

        assert_eq!(pixels[0], [0, 0, 0, 0]);
        assert_eq!(pixels[4], [0, 255, 255, 255]);
        // The original image is not modified.
        assert_eq!(image_pixels(&image)[4], RED);
    }

    #[test]
    fn default_adjustments_keep_the_image() {
        let image = test_image(&[[10, 20, 30, 255]], 1);
        let adjustments = ImageAdjustments::default();

        assert!(adjustments.is_default());
        assert_eq!(
            image_pixels(&adjustments.apply(&image)),
            image_pixels(&image)
        );
    }
}
