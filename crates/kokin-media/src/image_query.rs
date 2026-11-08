//! Reproducible query preparation. Coordinates address the orientation-corrected
//! original image (or the worker's selected PDF-page raster), in thousandths.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
impl Rect {
    pub fn valid(self) -> bool {
        self.width > 0
            && self.height > 0
            && u32::from(self.x) + u32::from(self.width) <= 1000
            && u32::from(self.y) + u32::from(self.height) <= 1000
    }
    pub fn pixels(self, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
        if !self.valid() || w == 0 || h == 0 {
            return None;
        }
        let x = (u64::from(self.x) * u64::from(w) / 1000) as u32;
        let y = (u64::from(self.y) * u64::from(h) / 1000) as u32;
        let right = (u64::from(self.x + self.width) * u64::from(w))
            .div_ceil(1000)
            .min(u64::from(w)) as u32;
        let bottom = (u64::from(self.y + self.height) * u64::from(h))
            .div_ceil(1000)
            .min(u64::from(h)) as u32;
        (right > x && bottom > y).then_some((x, y, right - x, bottom - y))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    pub crop: Option<Rect>,
    pub quarter_turns: u8,
    pub mirror_horizontal: bool,
    pub mirror_vertical: bool,
    pub brightness: i16,
    pub contrast: i16,
    pub red: i16,
    pub green: i16,
    pub blue: i16,
    pub obscure: Vec<Rect>,
}
impl Recipe {
    pub fn validate(&self) -> std::result::Result<(), crate::Error> {
        if self.version != 1
            || self.quarter_turns > 3
            || self.crop.is_some_and(|r| !r.valid())
            || self.obscure.len() > 64
            || self.obscure.iter().any(|r| !r.valid())
            || [
                self.brightness,
                self.contrast,
                self.red,
                self.green,
                self.blue,
            ]
            .iter()
            .any(|v| !(-100..=100).contains(v))
        {
            return Err(crate::Error::Limit);
        }
        Ok(())
    }
}
#[cfg(feature = "engine")]
pub fn prepare(
    mut image: image::DynamicImage,
    recipe: &Recipe,
) -> std::result::Result<image::DynamicImage, crate::Error> {
    recipe.validate()?;
    // Solid opaque redaction precedes crop/rotation/color. Blur is not a reliable
    // exclusion mechanism; no promise of complete anonymization is made.
    let mut rgba = image.to_rgba8();
    for rect in &recipe.obscure {
        let (x, y, w, h) = rect
            .pixels(rgba.width(), rgba.height())
            .ok_or(crate::Error::Limit)?;
        for yy in y..y + h {
            for xx in x..x + w {
                rgba.put_pixel(xx, yy, image::Rgba([0, 0, 0, 255]));
            }
        }
    }
    image = image::DynamicImage::ImageRgba8(rgba);
    if let Some(rect) = recipe.crop {
        let (x, y, w, h) = rect
            .pixels(image.width(), image.height())
            .ok_or(crate::Error::Limit)?;
        image = image.crop_imm(x, y, w, h);
    }
    image = match recipe.quarter_turns {
        1 => image.rotate90(),
        2 => image.rotate180(),
        3 => image.rotate270(),
        _ => image,
    };
    if recipe.mirror_horizontal {
        image = image.fliph();
    }
    if recipe.mirror_vertical {
        image = image.flipv();
    }
    if recipe.brightness != 0 {
        image = image.brighten(i32::from(recipe.brightness) * 255 / 100);
    }
    if recipe.contrast != 0 {
        image = image.adjust_contrast(f32::from(recipe.contrast));
    }
    let mut pixels = image.to_rgba8();
    for pixel in pixels.pixels_mut() {
        for (i, offset) in [recipe.red, recipe.green, recipe.blue].iter().enumerate() {
            pixel.0[i] =
                (i32::from(pixel.0[i]) + i32::from(*offset) * 255 / 100).clamp(0, 255) as u8;
        }
    }
    Ok(image::DynamicImage::ImageRgba8(pixels))
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn rectangles_and_recipes_are_bounded() {
        let r = Rect {
            x: 999,
            y: 0,
            width: 1,
            height: 1000,
        };
        assert_eq!(r.pixels(3, 2), Some((2, 0, 1, 2)));
        assert!(!Rect { x: 1000, ..r }.valid());
        assert!(Recipe {
            version: 1,
            ..Default::default()
        }
        .validate()
        .is_ok());
        assert!(Recipe {
            version: 1,
            red: 101,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(Recipe::default().validate().is_err());
    }
    #[cfg(feature = "engine")]
    #[test]
    fn preparation_preserves_original_and_records_real_pixel_changes() {
        let original = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            8,
            4,
            image::Rgba([120, 40, 30, 255]),
        ));
        let recipe = Recipe {
            version: 1,
            crop: Some(Rect {
                x: 0,
                y: 0,
                width: 500,
                height: 1000,
            }),
            quarter_turns: 1,
            mirror_horizontal: true,
            obscure: vec![Rect {
                x: 0,
                y: 0,
                width: 1000,
                height: 1000,
            }],
            ..Default::default()
        };
        let prepared = prepare(original.clone(), &recipe).unwrap();
        assert_eq!((prepared.width(), prepared.height()), (4, 4));
        assert!(prepared.to_rgba8().pixels().all(|p| p.0 == [0, 0, 0, 255]));
        assert_eq!(original.to_rgba8().get_pixel(0, 0).0, [120, 40, 30, 255]);
    }
    #[cfg(feature = "engine")]
    #[test]
    fn rotation_mirror_and_color_channels_change_the_expected_pixels() {
        let mut pixels = image::RgbaImage::new(3, 2);
        for (n, pixel) in pixels.pixels_mut().enumerate() {
            *pixel = image::Rgba([20 * (n as u8 + 1), 40, 30, 255]);
        }
        let original = image::DynamicImage::ImageRgba8(pixels);
        let transformed = prepare(
            original.clone(),
            &Recipe {
                version: 1,
                quarter_turns: 1,
                mirror_horizontal: true,
                ..Default::default()
            },
        )
        .unwrap()
        .to_rgba8();
        assert_eq!(transformed.dimensions(), (2, 3));
        for (x, y, expected) in [(0, 0, 20), (1, 0, 80), (0, 1, 40), (1, 2, 120)] {
            assert_eq!(transformed.get_pixel(x, y).0[0], expected);
        }
        let adjusted = prepare(
            original,
            &Recipe {
                version: 1,
                brightness: 10,
                red: 20,
                blue: -20,
                ..Default::default()
            },
        )
        .unwrap()
        .to_rgba8();
        let pixel = adjusted.get_pixel(0, 0).0;
        assert!(
            pixel[0].abs_diff(96) <= 1 && pixel[1].abs_diff(65) <= 1 && pixel[2].abs_diff(4) <= 1
        );
        assert_eq!(pixel[3], 255);
    }
}
