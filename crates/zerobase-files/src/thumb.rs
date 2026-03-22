//! Thumbnail generation for image files.
//!
//! Generates on-demand thumbnails for image files, caching them in storage
//! alongside the original file under a `thumbs/` subdirectory.
//!
//! # Supported formats
//!
//! Only raster image formats are supported: JPEG, PNG, GIF, WebP.
//! Non-image files return an error.
//!
//! # Thumb spec format
//!
//! Follows PocketBase's format: `WxH[mode]`
//!
//! - `100x100`  — center crop to exact 100×100
//! - `200x0`    — resize to width 200, auto height (preserve aspect ratio)
//! - `0x150`    — resize to height 150, auto width (preserve aspect ratio)
//! - `100x100t` — crop from top
//! - `100x100b` — crop from bottom
//! - `100x100f` — fit within bounds (preserve aspect ratio, no crop)
//!
//! # Thumbnail storage layout
//!
//! ```text
//! <collection_id>/<record_id>/thumbs/<spec>_<filename>
//! ```
//!
//! Example: `col123/rec456/thumbs/100x100_abc_photo.jpg`

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};

use zerobase_core::storage::{StorageError, ThumbMode, ThumbSize};

/// Check if a MIME type is a supported image format for thumbnailing.
pub fn is_thumbable(content_type: &str) -> bool {
    matches!(
        content_type,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    )
}

/// Build the storage key for a thumbnail.
pub fn thumb_key(collection_id: &str, record_id: &str, spec: &ThumbSize, filename: &str) -> String {
    format!("{collection_id}/{record_id}/thumbs/{spec}_{filename}")
}

/// Parse a thumbnail specification string into a [`ThumbSize`].
///
/// Accepts formats like `"100x100"`, `"200x0"`, `"0x150"`,
/// `"100x100t"`, `"100x100b"`, `"100x100f"`.
///
/// Returns `None` if the string is invalid.
pub fn parse_thumb_spec(spec: &str) -> Option<ThumbSize> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }

    // Detect optional mode suffix.
    let (dimensions, mode) = if spec.ends_with('t') {
        (&spec[..spec.len() - 1], ThumbMode::Top)
    } else if spec.ends_with('b') {
        (&spec[..spec.len() - 1], ThumbMode::Bottom)
    } else if spec.ends_with('f') {
        (&spec[..spec.len() - 1], ThumbMode::Fit)
    } else {
        (spec, ThumbMode::Center)
    };

    // Split on 'x' separator (case insensitive — also accept 'X').
    let parts: Vec<&str> = dimensions
        .splitn(2, |c: char| c == 'x' || c == 'X')
        .collect();
    if parts.len() != 2 {
        return None;
    }

    let width: u32 = parts[0].parse().ok()?;
    let height: u32 = parts[1].parse().ok()?;

    // At least one dimension must be > 0.
    if width == 0 && height == 0 {
        return None;
    }

    Some(ThumbSize {
        width,
        height,
        mode,
    })
}

/// Determine the output [`ImageFormat`] from a MIME type.
fn image_format_from_mime(content_type: &str) -> Option<ImageFormat> {
    match content_type {
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/png" => Some(ImageFormat::Png),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

/// Generate a thumbnail from raw image bytes.
///
/// Returns the thumbnail bytes in the same format as the original.
///
/// # Errors
///
/// Returns `StorageError::Io` if the image cannot be decoded or encoded.
pub fn generate_thumbnail(
    data: &[u8],
    content_type: &str,
    spec: &ThumbSize,
) -> Result<Vec<u8>, StorageError> {
    // Verify this is a thumbable format.
    if !is_thumbable(content_type) {
        return Err(StorageError::io(format!(
            "cannot generate thumbnail for MIME type: {content_type}"
        )));
    }

    let format = image_format_from_mime(content_type)
        .ok_or_else(|| StorageError::io(format!("unsupported image format: {content_type}")))?;

    // Decode the image.
    let img = image::load_from_memory(data)
        .map_err(|e| StorageError::io(format!("failed to decode image: {e}")))?;

    // Resize/crop according to spec.
    let thumb = resize_image(&img, spec);

    // Encode back to the same format.
    let mut buf = Cursor::new(Vec::new());
    thumb
        .write_to(&mut buf, format)
        .map_err(|e| StorageError::io(format!("failed to encode thumbnail: {e}")))?;

    Ok(buf.into_inner())
}

/// Resize or crop an image according to the given [`ThumbSize`].
fn resize_image(img: &DynamicImage, spec: &ThumbSize) -> DynamicImage {
    let (orig_w, orig_h) = (img.width(), img.height());

    match (spec.width, spec.height) {
        // Auto height: resize to target width, preserve aspect ratio.
        (w, 0) => {
            let h = (orig_h as f64 * w as f64 / orig_w as f64).round() as u32;
            img.resize_exact(w, h.max(1), FilterType::Lanczos3)
        }
        // Auto width: resize to target height, preserve aspect ratio.
        (0, h) => {
            let w = (orig_w as f64 * h as f64 / orig_h as f64).round() as u32;
            img.resize_exact(w.max(1), h, FilterType::Lanczos3)
        }
        // Both dimensions specified.
        (w, h) => match spec.mode {
            ThumbMode::Fit => {
                // Fit within bounds — no cropping, preserve aspect ratio.
                img.resize(w, h, FilterType::Lanczos3)
            }
            ThumbMode::Center => {
                // Resize to cover then center-crop.
                let resized = resize_to_cover(img, w, h);
                crop_center(&resized, w, h)
            }
            ThumbMode::Top => {
                // Resize to cover then crop from top.
                let resized = resize_to_cover(img, w, h);
                crop_top(&resized, w, h)
            }
            ThumbMode::Bottom => {
                // Resize to cover then crop from bottom.
                let resized = resize_to_cover(img, w, h);
                crop_bottom(&resized, w, h)
            }
        },
    }
}

/// Resize an image so it fully covers the target dimensions while
/// preserving aspect ratio. The result may be larger than target in
/// one dimension.
fn resize_to_cover(img: &DynamicImage, target_w: u32, target_h: u32) -> DynamicImage {
    let (orig_w, orig_h) = (img.width(), img.height());
    let scale_w = target_w as f64 / orig_w as f64;
    let scale_h = target_h as f64 / orig_h as f64;
    let scale = scale_w.max(scale_h);

    let new_w = (orig_w as f64 * scale).ceil() as u32;
    let new_h = (orig_h as f64 * scale).ceil() as u32;

    img.resize_exact(new_w.max(1), new_h.max(1), FilterType::Lanczos3)
}

/// Center-crop an image to the target dimensions.
fn crop_center(img: &DynamicImage, target_w: u32, target_h: u32) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let x = w.saturating_sub(target_w) / 2;
    let y = h.saturating_sub(target_h) / 2;
    img.crop_imm(x, y, target_w.min(w), target_h.min(h))
}

/// Crop from the top of an image.
fn crop_top(img: &DynamicImage, target_w: u32, target_h: u32) -> DynamicImage {
    let w = img.width();
    let x = w.saturating_sub(target_w) / 2;
    img.crop_imm(x, 0, target_w.min(w), target_h.min(img.height()))
}

/// Crop from the bottom of an image.
fn crop_bottom(img: &DynamicImage, target_w: u32, target_h: u32) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let x = w.saturating_sub(target_w) / 2;
    let y = h.saturating_sub(target_h);
    img.crop_imm(x, y, target_w.min(w), target_h.min(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_thumb_spec ─────────────────────────────────────────────────

    #[test]
    fn parse_basic_dimensions() {
        let spec = parse_thumb_spec("100x100").unwrap();
        assert_eq!(spec.width, 100);
        assert_eq!(spec.height, 100);
        assert_eq!(spec.mode, ThumbMode::Center);
    }

    #[test]
    fn parse_auto_height() {
        let spec = parse_thumb_spec("200x0").unwrap();
        assert_eq!(spec.width, 200);
        assert_eq!(spec.height, 0);
        assert_eq!(spec.mode, ThumbMode::Center);
    }

    #[test]
    fn parse_auto_width() {
        let spec = parse_thumb_spec("0x150").unwrap();
        assert_eq!(spec.width, 0);
        assert_eq!(spec.height, 150);
        assert_eq!(spec.mode, ThumbMode::Center);
    }

    #[test]
    fn parse_fit_mode() {
        let spec = parse_thumb_spec("300x200f").unwrap();
        assert_eq!(spec.width, 300);
        assert_eq!(spec.height, 200);
        assert_eq!(spec.mode, ThumbMode::Fit);
    }

    #[test]
    fn parse_top_mode() {
        let spec = parse_thumb_spec("100x100t").unwrap();
        assert_eq!(spec.width, 100);
        assert_eq!(spec.height, 100);
        assert_eq!(spec.mode, ThumbMode::Top);
    }

    #[test]
    fn parse_bottom_mode() {
        let spec = parse_thumb_spec("100x100b").unwrap();
        assert_eq!(spec.width, 100);
        assert_eq!(spec.height, 100);
        assert_eq!(spec.mode, ThumbMode::Bottom);
    }

    #[test]
    fn parse_uppercase_x() {
        let spec = parse_thumb_spec("100X200").unwrap();
        assert_eq!(spec.width, 100);
        assert_eq!(spec.height, 200);
    }

    #[test]
    fn parse_rejects_zero_zero() {
        assert!(parse_thumb_spec("0x0").is_none());
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(parse_thumb_spec("").is_none());
    }

    #[test]
    fn parse_rejects_invalid_format() {
        assert!(parse_thumb_spec("abc").is_none());
        assert!(parse_thumb_spec("100").is_none());
        assert!(parse_thumb_spec("100x").is_none());
        assert!(parse_thumb_spec("x100").is_none());
        assert!(parse_thumb_spec("axb").is_none());
    }

    #[test]
    fn parse_rejects_negative() {
        // u32 parse won't accept negative numbers
        assert!(parse_thumb_spec("-1x100").is_none());
        assert!(parse_thumb_spec("100x-1").is_none());
    }

    #[test]
    fn parse_with_whitespace() {
        let spec = parse_thumb_spec("  100x100  ").unwrap();
        assert_eq!(spec.width, 100);
        assert_eq!(spec.height, 100);
    }

    // ── is_thumbable ─────────────────────────────────────────────────────

    #[test]
    fn is_thumbable_accepts_image_types() {
        assert!(is_thumbable("image/jpeg"));
        assert!(is_thumbable("image/png"));
        assert!(is_thumbable("image/gif"));
        assert!(is_thumbable("image/webp"));
    }

    #[test]
    fn is_thumbable_rejects_non_image_types() {
        assert!(!is_thumbable("application/pdf"));
        assert!(!is_thumbable("text/plain"));
        assert!(!is_thumbable("image/svg+xml"));
        assert!(!is_thumbable("video/mp4"));
    }

    // ── thumb_key ────────────────────────────────────────────────────────

    #[test]
    fn thumb_key_builds_correct_path() {
        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Center,
        };
        let key = thumb_key("col1", "rec1", &spec, "photo.jpg");
        assert_eq!(key, "col1/rec1/thumbs/100x100_photo.jpg");
    }

    #[test]
    fn thumb_key_with_fit_mode() {
        let spec = ThumbSize {
            width: 200,
            height: 300,
            mode: ThumbMode::Fit,
        };
        let key = thumb_key("col1", "rec1", &spec, "photo.jpg");
        assert_eq!(key, "col1/rec1/thumbs/200x300f_photo.jpg");
    }

    // ── generate_thumbnail ───────────────────────────────────────────────

    /// Create a simple test PNG image of given dimensions.
    fn create_test_png(width: u32, height: u32) -> Vec<u8> {
        let img = DynamicImage::new_rgba8(width, height);
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    /// Create a simple test JPEG image of given dimensions.
    fn create_test_jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = DynamicImage::new_rgb8(width, height);
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Jpeg).unwrap();
        buf.into_inner()
    }

    #[test]
    fn generate_thumbnail_center_crop() {
        let data = create_test_png(400, 300);
        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Center,
        };

        let result = generate_thumbnail(&data, "image/png", &spec).unwrap();
        let thumb = image::load_from_memory(&result).unwrap();
        assert_eq!(thumb.width(), 100);
        assert_eq!(thumb.height(), 100);
    }

    #[test]
    fn generate_thumbnail_fit_mode() {
        let data = create_test_png(400, 200);
        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Fit,
        };

        let result = generate_thumbnail(&data, "image/png", &spec).unwrap();
        let thumb = image::load_from_memory(&result).unwrap();
        // 400x200 fitted into 100x100 → 100x50 (aspect ratio preserved).
        assert_eq!(thumb.width(), 100);
        assert_eq!(thumb.height(), 50);
    }

    #[test]
    fn generate_thumbnail_auto_height() {
        let data = create_test_png(400, 200);
        let spec = ThumbSize {
            width: 200,
            height: 0,
            mode: ThumbMode::Center,
        };

        let result = generate_thumbnail(&data, "image/png", &spec).unwrap();
        let thumb = image::load_from_memory(&result).unwrap();
        assert_eq!(thumb.width(), 200);
        assert_eq!(thumb.height(), 100); // 200 / (400/200) = 100
    }

    #[test]
    fn generate_thumbnail_auto_width() {
        let data = create_test_png(400, 200);
        let spec = ThumbSize {
            width: 0,
            height: 100,
            mode: ThumbMode::Center,
        };

        let result = generate_thumbnail(&data, "image/png", &spec).unwrap();
        let thumb = image::load_from_memory(&result).unwrap();
        assert_eq!(thumb.width(), 200); // 400 * (100/200) = 200
        assert_eq!(thumb.height(), 100);
    }

    #[test]
    fn generate_thumbnail_top_crop() {
        let data = create_test_png(200, 400);
        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Top,
        };

        let result = generate_thumbnail(&data, "image/png", &spec).unwrap();
        let thumb = image::load_from_memory(&result).unwrap();
        assert_eq!(thumb.width(), 100);
        assert_eq!(thumb.height(), 100);
    }

    #[test]
    fn generate_thumbnail_bottom_crop() {
        let data = create_test_png(200, 400);
        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Bottom,
        };

        let result = generate_thumbnail(&data, "image/png", &spec).unwrap();
        let thumb = image::load_from_memory(&result).unwrap();
        assert_eq!(thumb.width(), 100);
        assert_eq!(thumb.height(), 100);
    }

    #[test]
    fn generate_thumbnail_jpeg_format() {
        let data = create_test_jpeg(400, 300);
        let spec = ThumbSize {
            width: 50,
            height: 50,
            mode: ThumbMode::Center,
        };

        let result = generate_thumbnail(&data, "image/jpeg", &spec).unwrap();
        // Verify it's a valid JPEG by loading it.
        let thumb = image::load_from_memory(&result).unwrap();
        assert_eq!(thumb.width(), 50);
        assert_eq!(thumb.height(), 50);
    }

    #[test]
    fn generate_thumbnail_rejects_non_image() {
        let result = generate_thumbnail(
            b"not an image",
            "application/pdf",
            &ThumbSize {
                width: 100,
                height: 100,
                mode: ThumbMode::Center,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn generate_thumbnail_rejects_corrupt_data() {
        let result = generate_thumbnail(
            b"corrupt",
            "image/png",
            &ThumbSize {
                width: 100,
                height: 100,
                mode: ThumbMode::Center,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn generate_thumbnail_large_to_small() {
        let data = create_test_png(2000, 1500);
        let spec = ThumbSize {
            width: 50,
            height: 50,
            mode: ThumbMode::Center,
        };

        let result = generate_thumbnail(&data, "image/png", &spec).unwrap();
        let thumb = image::load_from_memory(&result).unwrap();
        assert_eq!(thumb.width(), 50);
        assert_eq!(thumb.height(), 50);
    }

    #[test]
    fn generate_thumbnail_same_size_as_original() {
        let data = create_test_png(100, 100);
        let spec = ThumbSize {
            width: 100,
            height: 100,
            mode: ThumbMode::Center,
        };

        let result = generate_thumbnail(&data, "image/png", &spec).unwrap();
        let thumb = image::load_from_memory(&result).unwrap();
        assert_eq!(thumb.width(), 100);
        assert_eq!(thumb.height(), 100);
    }

    // ── roundtrip: parse → generate ──────────────────────────────────────

    #[test]
    fn roundtrip_parse_and_generate() {
        let data = create_test_png(800, 600);

        for spec_str in &["100x100", "200x0", "0x150", "100x100f", "50x50t", "50x50b"] {
            let spec = parse_thumb_spec(spec_str).unwrap();
            let result = generate_thumbnail(&data, "image/png", &spec);
            assert!(
                result.is_ok(),
                "failed to generate thumbnail for spec '{spec_str}'"
            );

            let thumb = image::load_from_memory(&result.unwrap()).unwrap();
            // Width and height should be > 0.
            assert!(thumb.width() > 0, "thumb width is 0 for spec '{spec_str}'");
            assert!(
                thumb.height() > 0,
                "thumb height is 0 for spec '{spec_str}'"
            );
        }
    }
}
