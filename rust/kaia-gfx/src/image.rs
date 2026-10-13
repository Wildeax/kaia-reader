//! Decoding PNG and JPEG, scaling to a box, and rendering SVG icons.
//!
//! Decoding checks the image size from the file header before any pixels are
//! decoded. A file above the limit is refused without allocating its pixels.
//! Scaling uses Lanczos3 on premultiplied alpha, so transparent edges do not
//! pick up dark fringes.

use std::fmt;
use std::io::Cursor;

use ::image::{DynamicImage, ImageFormat, ImageReader};
use resvg::{tiny_skia, usvg};

use crate::geom::Size;
use crate::pixmap::{PixelFormat, Pixmap};
use crate::rgba::AlphaPixmap;

/// Largest side accepted by [`render_svg`] in pixels.
pub const MAX_SVG_SIDE: u32 = 4096;

/// Largest image the decoder accepts, checked from the file header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    pub max_width: u32,
    pub max_height: u32,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_width: 4096,
            max_height: 4096,
        }
    }
}

/// Errors from decoding raster images.
#[derive(Debug)]
pub enum ImageError {
    /// The data is not PNG or JPEG.
    Unsupported,
    /// The image is larger than the limits. Found from the header, before decoding.
    TooLarge {
        width: u32,
        height: u32,
        limits: DecodeLimits,
    },
    /// The file is PNG or JPEG but could not be read.
    Decode(::image::ImageError),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::Unsupported => write!(f, "only PNG and JPEG images are supported"),
            ImageError::TooLarge {
                width,
                height,
                limits,
            } => write!(
                f,
                "image is {width}x{height}, the limit is {}x{}",
                limits.max_width, limits.max_height
            ),
            ImageError::Decode(e) => write!(f, "cannot decode image: {e}"),
        }
    }
}

impl std::error::Error for ImageError {}

/// Errors from rendering SVG.
#[derive(Debug)]
pub enum SvgError {
    /// The SVG could not be parsed.
    Parse(usvg::Error),
    /// The requested size is zero or above [`MAX_SVG_SIDE`].
    Size(Size),
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SvgError::Parse(e) => write!(f, "cannot parse SVG: {e}"),
            SvgError::Size(s) => write!(f, "unsupported SVG render size {}x{}", s.w, s.h),
        }
    }
}

impl std::error::Error for SvgError {}

/// Reads the format and size from the header and checks the limits.
fn check_header(bytes: &[u8], limits: DecodeLimits) -> Result<ImageFormat, ImageError> {
    let format = ::image::guess_format(bytes).map_err(|_| ImageError::Unsupported)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(ImageError::Unsupported);
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(ImageError::Decode)?;
    if width > limits.max_width || height > limits.max_height {
        return Err(ImageError::TooLarge {
            width,
            height,
            limits,
        });
    }
    Ok(format)
}

fn decode_dynamic(bytes: &[u8], limits: DecodeLimits) -> Result<DynamicImage, ImageError> {
    let format = check_header(bytes, limits)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut guard = ::image::Limits::default();
    guard.max_image_width = Some(limits.max_width);
    guard.max_image_height = Some(limits.max_height);
    reader.limits(guard);
    reader.decode().map_err(ImageError::Decode)
}

/// Decodes a PNG or JPEG into straight-alpha RGBA. JPEG gets alpha 255.
pub fn decode_rgba(bytes: &[u8], limits: DecodeLimits) -> Result<AlphaPixmap, ImageError> {
    let img = decode_dynamic(bytes, limits)?.to_rgba8();
    let (w, h) = img.dimensions();
    // The buffer length is width * height * 4 by construction.
    Ok(AlphaPixmap::from_data(w, h, img.into_raw()).expect("RGBA buffer matches its size"))
}

/// Decodes a PNG or JPEG into an Rgb24 pixmap. Transparent pixels are placed
/// over white.
pub fn decode_rgb(bytes: &[u8], limits: DecodeLimits) -> Result<Pixmap, ImageError> {
    let img = decode_dynamic(bytes, limits)?;
    let (w, h) = (img.width(), img.height());
    let data = if img.color().has_alpha() {
        let mut data = img.to_rgba8().into_raw();
        for p in data.as_chunks_mut::<4>().0 {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = mix(255, *c, a);
            }
        }
        data.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect()
    } else {
        img.to_rgb8().into_raw()
    };
    // The buffer length is width * height * 3 by construction.
    Ok(Pixmap::from_data(w, h, PixelFormat::Rgb24, data).expect("RGB buffer matches its size"))
}

/// Blends `src` over `dst` with alpha `a` (0 to 255), rounding to nearest.
fn mix(dst: u8, src: u8, a: u32) -> u8 {
    ((dst as u32 * (255 - a) + src as u32 * a + 127) / 255) as u8
}

/// How an image fits a box of a different aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitMode {
    /// The largest size with the same aspect ratio that fits inside the box.
    Contain,
    /// The smallest size with the same aspect ratio that covers the box. The
    /// overflow is cropped from the center, so the result is the box size.
    Cover,
}

/// The size of `src` scaled to fit `bound` with its aspect ratio kept. For
/// [`FitMode::Cover`] the result covers `bound` and may be larger than it.
/// A zero size on either side gives a zero size.
pub fn fit_size(src: Size, bound: Size, mode: FitMode) -> Size {
    if src.w == 0 || src.h == 0 || bound.w == 0 || bound.h == 0 {
        return Size::new(0, 0);
    }
    let sx = bound.w as f64 / src.w as f64;
    let sy = bound.h as f64 / src.h as f64;
    let s = match mode {
        FitMode::Contain => sx.min(sy),
        FitMode::Cover => sx.max(sy),
    };
    Size::new(
        ((src.w as f64 * s).round() as u32).max(1),
        ((src.h as f64 * s).round() as u32).max(1),
    )
}

/// Scales an RGBA image to `size` with Lanczos3 on premultiplied alpha.
pub fn scale_rgba(src: &AlphaPixmap, size: Size) -> AlphaPixmap {
    if size == src.size() {
        return src.clone();
    }
    if size.w == 0 || size.h == 0 || src.width() == 0 || src.height() == 0 {
        return AlphaPixmap::new(size.w, size.h);
    }
    let premultiplied: Vec<u8> = src
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = p[3] as u32;
            [mix(0, p[0], a), mix(0, p[1], a), mix(0, p[2], a), p[3]]
        })
        .collect();
    let img = ::image::RgbaImage::from_raw(src.width(), src.height(), premultiplied)
        .expect("RGBA buffer matches its size");
    let mut out =
        ::image::imageops::resize(&img, size.w, size.h, ::image::imageops::FilterType::Lanczos3).into_raw();
    for p in out.as_chunks_mut::<4>().0 {
        let a = p[3];
        for c in &mut p[..3] {
            *c = unpremultiply(*c, a);
        }
    }
    AlphaPixmap::from_data(size.w, size.h, out).expect("RGBA buffer matches its size")
}

/// Scales a Gray8 or Rgb24 pixmap to `size` with Lanczos3.
pub fn scale_pixmap(src: &Pixmap, size: Size) -> Pixmap {
    if size == src.size() {
        return src.clone();
    }
    let format = src.format();
    if size.w == 0 || size.h == 0 || src.width() == 0 || src.height() == 0 {
        return Pixmap::new(size.w, size.h, format);
    }
    let filter = ::image::imageops::FilterType::Lanczos3;
    let data = match format {
        PixelFormat::Gray8 => {
            let img = ::image::GrayImage::from_raw(src.width(), src.height(), src.data().to_vec())
                .expect("gray buffer matches its size");
            ::image::imageops::resize(&img, size.w, size.h, filter).into_raw()
        }
        PixelFormat::Rgb24 => {
            let img = ::image::RgbImage::from_raw(src.width(), src.height(), src.data().to_vec())
                .expect("RGB buffer matches its size");
            ::image::imageops::resize(&img, size.w, size.h, filter).into_raw()
        }
    };
    Pixmap::from_data(size.w, size.h, format, data).expect("scaled buffer matches its size")
}

/// Fits an RGBA image into `bound`. See [`FitMode`].
pub fn fit_rgba(src: &AlphaPixmap, bound: Size, mode: FitMode) -> AlphaPixmap {
    let scaled = scale_rgba(src, fit_size(src.size(), bound, mode));
    match mode {
        FitMode::Contain => scaled,
        FitMode::Cover => {
            let (cw, ch) = (bound.w.min(scaled.width()), bound.h.min(scaled.height()));
            let (x0, y0) = (
                (scaled.width() - cw) as usize / 2,
                (scaled.height() - ch) as usize / 2,
            );
            let data = crop_bytes(scaled.data(), scaled.width() as usize, 4, (x0, y0), (cw, ch));
            AlphaPixmap::from_data(cw, ch, data).expect("cropped buffer matches its size")
        }
    }
}

/// Fits a Gray8 or Rgb24 pixmap into `bound`. See [`FitMode`].
pub fn fit_pixmap(src: &Pixmap, bound: Size, mode: FitMode) -> Pixmap {
    let scaled = scale_pixmap(src, fit_size(src.size(), bound, mode));
    match mode {
        FitMode::Contain => scaled,
        FitMode::Cover => {
            let format = scaled.format();
            let (cw, ch) = (bound.w.min(scaled.width()), bound.h.min(scaled.height()));
            let (x0, y0) = (
                (scaled.width() - cw) as usize / 2,
                (scaled.height() - ch) as usize / 2,
            );
            let data = crop_bytes(
                scaled.data(),
                scaled.width() as usize,
                format.bytes_per_pixel(),
                (x0, y0),
                (cw, ch),
            );
            Pixmap::from_data(cw, ch, format, data).expect("cropped buffer matches its size")
        }
    }
}

/// Copies a `(cw, ch)` block starting at `(x0, y0)` out of a row-major buffer.
fn crop_bytes(
    data: &[u8],
    width: usize,
    bpp: usize,
    (x0, y0): (usize, usize),
    (cw, ch): (u32, u32),
) -> Vec<u8> {
    let row = cw as usize * bpp;
    let mut out = Vec::with_capacity(row * ch as usize);
    for y in y0..y0 + ch as usize {
        let start = (y * width + x0) * bpp;
        out.extend_from_slice(&data[start..start + row]);
    }
    out
}

/// Converts one premultiplied channel to straight alpha. Filters can overshoot,
/// so the result is clamped. A zero alpha gives 0.
fn unpremultiply(c: u8, a: u8) -> u8 {
    let a = a as u32;
    (c as u32 * 255 + a / 2)
        .checked_div(a)
        .map_or(0, |v| v.min(255) as u8)
}

/// Renders an SVG document into an RGBA image of exactly `size`. The drawing
/// keeps its aspect ratio and is centered. Unused space is transparent.
pub fn render_svg(svg: &[u8], size: Size) -> Result<AlphaPixmap, SvgError> {
    if size.w == 0 || size.h == 0 || size.w > MAX_SVG_SIDE || size.h > MAX_SVG_SIDE {
        return Err(SvgError::Size(size));
    }
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).map_err(SvgError::Parse)?;
    let (tw, th) = (tree.size().width(), tree.size().height());
    let scale = (size.w as f32 / tw).min(size.h as f32 / th);
    let dx = (size.w as f32 - tw * scale) / 2.0;
    let dy = (size.h as f32 - th * scale) / 2.0;
    let mut canvas = tiny_skia::Pixmap::new(size.w, size.h).ok_or(SvgError::Size(size))?;
    let transform = tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, dx, dy);
    resvg::render(&tree, transform, &mut canvas.as_mut());

    // tiny-skia stores premultiplied alpha. Convert to straight alpha.
    let mut data = canvas.data().to_vec();
    for p in data.as_chunks_mut::<4>().0 {
        let a = p[3];
        for c in &mut p[..3] {
            *c = unpremultiply(*c, a);
        }
    }
    Ok(AlphaPixmap::from_data(size.w, size.h, data).expect("canvas matches its size"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png::encode_rgba;

    const ICON_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/icons/mdlight");

    fn icon(name: &str) -> Vec<u8> {
        std::fs::read(format!("{ICON_DIR}/{name}")).expect("icon in resources/icons/mdlight")
    }

    fn solid_rgba(w: u32, h: u32, px: [u8; 4]) -> AlphaPixmap {
        let data = px.repeat(w as usize * h as usize);
        AlphaPixmap::from_data(w, h, data).unwrap()
    }

    #[test]
    fn png_round_trip_keeps_alpha() {
        let mut src = AlphaPixmap::new(3, 2);
        src.put(0, 0, [10, 20, 30, 255]);
        src.put(2, 1, [200, 100, 50, 128]);
        let bytes = encode_rgba(&src).unwrap();
        let back = decode_rgba(&bytes, DecodeLimits::default()).unwrap();
        assert_eq!(back, src);
    }

    #[test]
    fn jpeg_decodes_to_rgb_and_gray_like_source() {
        let (w, h) = (16u32, 16u32);
        let rgb: Vec<u8> = [200u8, 100, 50].repeat((w * h) as usize);
        let mut bytes = Vec::new();
        ::image::ImageEncoder::write_image(
            ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95),
            &rgb,
            w,
            h,
            ::image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        let pm = decode_rgb(&bytes, DecodeLimits::default()).unwrap();
        assert_eq!(pm.format(), PixelFormat::Rgb24);
        let c = pm.get(8, 8).unwrap();
        assert!(
            (c.r as i32 - 200).abs() <= 6 && (c.g as i32 - 100).abs() <= 6 && (c.b as i32 - 50).abs() <= 6
        );
        let rgba = decode_rgba(&bytes, DecodeLimits::default()).unwrap();
        assert_eq!(rgba.get(8, 8).unwrap()[3], 255);
    }

    #[test]
    fn oversize_is_refused_from_the_header_alone() {
        let mut bytes = Vec::new();
        let img = vec![0u8; 64 * 64 * 4];
        ::image::ImageEncoder::write_image(
            ::image::codecs::png::PngEncoder::new(&mut bytes),
            &img,
            64,
            64,
            ::image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        // Cut the file inside the IDAT chunk. The header parses, but the pixels cannot be
        // decoded, so only the header check can produce TooLarge.
        let idat = bytes.windows(4).position(|w| w == b"IDAT").expect("PNG has IDAT");
        let truncated = &bytes[..idat + 6];
        let limits = DecodeLimits {
            max_width: 32,
            max_height: 32,
        };
        match decode_rgba(truncated, limits) {
            Err(ImageError::TooLarge {
                width: 64,
                height: 64,
                ..
            }) => {}
            other => panic!("expected TooLarge from the header, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_formats_are_refused() {
        let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00;";
        assert!(matches!(
            decode_rgba(gif, DecodeLimits::default()),
            Err(ImageError::Unsupported)
        ));
        assert!(matches!(
            decode_rgba(b"not an image", DecodeLimits::default()),
            Err(ImageError::Unsupported)
        ));
    }

    #[test]
    fn fit_sizes_keep_aspect_ratio() {
        let src = Size::new(100, 50);
        let bound = Size::new(40, 40);
        assert_eq!(fit_size(src, bound, FitMode::Contain), Size::new(40, 20));
        assert_eq!(fit_size(src, bound, FitMode::Cover), Size::new(80, 40));
        assert_eq!(
            fit_size(Size::new(0, 5), bound, FitMode::Contain),
            Size::new(0, 0)
        );
    }

    #[test]
    fn scaling_a_flat_image_stays_flat() {
        let src = solid_rgba(10, 10, [200, 100, 50, 255]);
        let out = scale_rgba(&src, Size::new(4, 4));
        assert_eq!(out.size(), Size::new(4, 4));
        for y in 0..4 {
            for x in 0..4 {
                let p = out.get(x, y).unwrap();
                assert!(
                    (p[0] as i32 - 200).abs() <= 1 && (p[3] as i32 - 255).abs() <= 1,
                    "{p:?}"
                );
            }
        }
    }

    #[test]
    fn transparent_edges_do_not_darken_colors() {
        let mut src = solid_rgba(8, 8, [255, 255, 255, 255]);
        for y in 0..8 {
            for x in 0..4 {
                src.put(x, y, [0, 0, 0, 0]);
            }
        }
        let out = scale_rgba(&src, Size::new(4, 8));
        let p = out.get(1, 4).unwrap();
        assert!(p[0] > 240, "white stays white next to transparency, got {p:?}");
    }

    #[test]
    fn cover_fit_returns_the_box_size() {
        let src = solid_rgba(100, 50, [1, 2, 3, 255]);
        let out = fit_rgba(&src, Size::new(40, 40), FitMode::Cover);
        assert_eq!(out.size(), Size::new(40, 40));

        let gray = Pixmap::new(100, 50, PixelFormat::Gray8);
        let out = fit_pixmap(&gray, Size::new(40, 40), FitMode::Cover);
        assert_eq!(out.size(), Size::new(40, 40));
        assert_eq!(out.format(), PixelFormat::Gray8);
    }

    #[test]
    fn renders_repo_icons_at_requested_size() {
        let left = render_svg(&icon("align.left.svg"), Size::new(64, 64)).unwrap();
        let center = render_svg(&icon("align.center.svg"), Size::new(64, 64)).unwrap();
        assert_eq!(left.size(), Size::new(64, 64));
        assert_ne!(left, center, "the two icons differ");

        // The viewBox is 48 wide and the 64 pixel output scales it by 4/3. The line at
        // y=24 spans x=5..43 in the viewBox, which is x=7..57 in the output.
        let on_line = left.get(32, 32).unwrap();
        assert!(
            on_line[3] > 200 && on_line[0] < 60,
            "stroke is dark and opaque, got {on_line:?}"
        );
        assert_eq!(left.get(0, 0).unwrap()[3], 0, "corner is transparent");
    }

    #[test]
    fn svg_is_centered_when_the_box_is_not_square() {
        let out = render_svg(&icon("align.left.svg"), Size::new(96, 48)).unwrap();
        assert_eq!(out.get(0, 0).unwrap()[3], 0, "left margin is transparent");
        assert_eq!(out.get(95, 47).unwrap()[3], 0, "right margin is transparent");
    }

    #[test]
    fn svg_rejects_bad_sizes_and_input() {
        assert!(matches!(
            render_svg(&icon("align.left.svg"), Size::new(0, 10)),
            Err(SvgError::Size(_))
        ));
        assert!(matches!(
            render_svg(&icon("align.left.svg"), Size::new(MAX_SVG_SIDE + 1, 10)),
            Err(SvgError::Size(_))
        ));
        assert!(matches!(
            render_svg(b"<svg", Size::new(8, 8)),
            Err(SvgError::Parse(_))
        ));
    }
}
