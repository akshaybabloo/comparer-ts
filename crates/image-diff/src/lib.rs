//! Pixel-by-pixel image comparison with a perceptual tolerance.
//!
//! Both images are split into horizontal bands of whole rows. A band is a contiguous
//! run of the RGBA buffer, so it is compared with a straight scan, and the diff image
//! splits into matching bands that are written without any locking. Natively the bands
//! run in parallel on rayon, one band per thread; on WebAssembly, which has no threads,
//! the whole image is a single band.
//!
//! Pixels are compared by their difference in the YIQ colour space, the measure
//! [pixelmatch](https://github.com/mapbox/pixelmatch) uses (Kotsarenko and Ramos,
//! "Measuring perceived color difference using YIQ NTSC transmission color space in
//! mobile applications"). It weights brightness over hue, roughly as the eye does.

use std::fmt;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};

/// The largest squared YIQ difference two pixels can have.
const MAX_YIQ_DELTA: f64 = 35215.0;

/// What a differing pixel is painted in the diff image.
const DIFF_COLOUR: [u8; 4] = [255, 0, 0, 255];

/// How strongly an unchanged pixel shows through in the diff image, from 0 (white) to 1.
const FADE: f64 = 0.1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DiffOptions {
    /// How much colour difference to tolerate, from 0 to 100.
    ///
    /// At 0 any change to a pixel's bytes counts, alpha included. Above 0 a pixel
    /// counts when its YIQ difference is over `(tolerance / 100)²` of the largest
    /// possible one, so 100 tolerates every change.
    pub tolerance: f64,
    /// Paint a diff image: differing pixels red, the rest the left image faded to grey.
    pub diff_image: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageDiff {
    pub width: u32,
    pub height: u32,
    pub different_pixels: u64,
    /// The diff image as RGBA, row by row, when [`DiffOptions::diff_image`] is set.
    pub diff_image: Option<Vec<u8>>,
}

impl ImageDiff {
    pub fn total_pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    pub fn identical(&self) -> bool {
        self.different_pixels == 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DiffError {
    /// An encoded image is not a PNG, JPEG, WebP, GIF or BMP, or is corrupt.
    Decode {
        side: &'static str,
        reason: String,
    },
    DimensionMismatch {
        left: (u32, u32),
        right: (u32, u32),
    },
    /// The tolerance is outside 0 to 100, or not a number.
    InvalidTolerance(f64),
    /// A pixel buffer is not the length its dimensions call for.
    BufferSize {
        side: &'static str,
        expected: usize,
        actual: usize,
    },
    /// The image is too large to address in memory.
    TooLarge {
        width: u32,
        height: u32,
    },
    Encode(String),
}

impl fmt::Display for DiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiffError::Decode { side, reason } => {
                write!(f, "{side} image could not be decoded: {reason}")
            }
            DiffError::DimensionMismatch { left, right } => write!(
                f,
                "images differ in size: left is {}x{}, right is {}x{}",
                left.0, left.1, right.0, right.1
            ),
            DiffError::InvalidTolerance(tolerance) => {
                write!(f, "tolerance must be between 0 and 100, got {tolerance}")
            }
            DiffError::BufferSize {
                side,
                expected,
                actual,
            } => write!(f, "{side} buffer is {actual} bytes, expected {expected}"),
            DiffError::TooLarge { width, height } => {
                write!(f, "a {width}x{height} image is too large")
            }
            DiffError::Encode(reason) => write!(f, "diff image could not be encoded: {reason}"),
        }
    }
}

impl std::error::Error for DiffError {}

/// When two pixels count as different.
#[derive(Clone, Copy, Debug)]
enum Threshold {
    /// Any byte differs.
    Exact,
    /// Their squared YIQ difference is over this.
    Yiq(f64),
}

impl Threshold {
    fn new(tolerance: f64) -> Result<Threshold, DiffError> {
        // `contains` is false for NaN too.
        if !(0.0..=100.0).contains(&tolerance) {
            return Err(DiffError::InvalidTolerance(tolerance));
        }
        if tolerance == 0.0 {
            return Ok(Threshold::Exact);
        }
        let fraction = tolerance / 100.0;
        Ok(Threshold::Yiq(MAX_YIQ_DELTA * fraction * fraction))
    }
}

/// Decodes two PNG, JPEG, WebP, GIF or BMP images and compares them.
///
/// The images may be in different formats: both are converted to 8-bit RGBA first,
/// so a 16-bit image is compared at 8-bit precision.
pub fn diff_encoded(
    left: &[u8],
    right: &[u8],
    options: &DiffOptions,
) -> Result<ImageDiff, DiffError> {
    // Checked up front, so a bad tolerance does not wait for two decodes to be reported.
    Threshold::new(options.tolerance)?;

    let decode = |side: &'static str, bytes: &[u8]| {
        image::load_from_memory(bytes)
            .map(|image| image.into_rgba8())
            .map_err(|error| DiffError::Decode {
                side,
                reason: error.to_string(),
            })
    };
    let left = decode("left", left)?;
    let right = decode("right", right)?;

    if left.dimensions() != right.dimensions() {
        return Err(DiffError::DimensionMismatch {
            left: left.dimensions(),
            right: right.dimensions(),
        });
    }
    let (width, height) = left.dimensions();
    diff_rgba(left.as_raw(), right.as_raw(), width, height, options)
}

/// Compares two images given as raw RGBA pixels, row by row, such as a canvas's
/// `ImageData`.
pub fn diff_rgba(
    left: &[u8],
    right: &[u8],
    width: u32,
    height: u32,
    options: &DiffOptions,
) -> Result<ImageDiff, DiffError> {
    let threshold = Threshold::new(options.tolerance)?;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(DiffError::TooLarge { width, height })?;
    check_len("left", left, expected)?;
    check_len("right", right, expected)?;

    let mut diff_image = options.diff_image.then(|| vec![0; expected]);
    let different_pixels = if expected == 0 {
        0
    } else {
        let diff_image = diff_image.as_deref_mut();
        diff_bands(left, right, diff_image, width, height, threshold)
    };

    Ok(ImageDiff {
        width,
        height,
        different_pixels,
        diff_image,
    })
}

/// Compares one band of RGBA pixels and returns how many differ, painting `diff_image`
/// if given.
///
/// Splitting two images into bands of whole pixels and adding up the counts gives the
/// same result as [`diff_rgba`], so bands can be handed to separate workers where
/// threads are not available.
pub fn diff_band(
    left: &[u8],
    right: &[u8],
    diff_image: Option<&mut [u8]>,
    tolerance: f64,
) -> Result<u64, DiffError> {
    let threshold = Threshold::new(tolerance)?;
    let expected = left.len() - left.len() % 4;
    check_len("left", left, expected)?;
    check_len("right", right, expected)?;
    if let Some(diff_image) = &diff_image {
        check_len("diff", diff_image, expected)?;
    }
    Ok(diff_pixels(left, right, diff_image, threshold))
}

/// Encodes an RGBA image, such as [`ImageDiff::diff_image`], as PNG.
///
/// Tuned for speed: a diff image is mostly flat grey, and on a 4K screenshot the
/// `Sub` filter came out as small as `Adaptive` in a third of the time.
pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, DiffError> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
        .write_image(rgba, width, height, ExtendedColorType::Rgba8)
        .map_err(|error| DiffError::Encode(error.to_string()))?;
    Ok(png)
}

fn check_len(side: &'static str, buffer: &[u8], expected: usize) -> Result<(), DiffError> {
    if buffer.len() == expected {
        Ok(())
    } else {
        Err(DiffError::BufferSize {
            side,
            expected,
            actual: buffer.len(),
        })
    }
}

/// Compares the images in bands of whole rows, one band per thread of the current rayon
/// pool, so every thread has work and no band waits for a free thread.
#[cfg(not(target_arch = "wasm32"))]
fn diff_bands(
    left: &[u8],
    right: &[u8],
    diff_image: Option<&mut [u8]>,
    width: u32,
    height: u32,
    threshold: Threshold,
) -> u64 {
    use rayon::prelude::*;

    let rows_per_band = (height as usize)
        .div_ceil(rayon::current_num_threads())
        .max(1);
    let band_len = rows_per_band * width as usize * 4;
    let bands = left.par_chunks(band_len).zip(right.par_chunks(band_len));
    match diff_image {
        Some(diff_image) => bands
            .zip(diff_image.par_chunks_mut(band_len))
            .map(|((left, right), out)| diff_pixels(left, right, Some(out), threshold))
            .sum(),
        None => bands
            .map(|(left, right)| diff_pixels(left, right, None, threshold))
            .sum(),
    }
}

/// A single scan: there is only the one thread to run it on.
#[cfg(target_arch = "wasm32")]
fn diff_bands(
    left: &[u8],
    right: &[u8],
    diff_image: Option<&mut [u8]>,
    _width: u32,
    _height: u32,
    threshold: Threshold,
) -> u64 {
    diff_pixels(left, right, diff_image, threshold)
}

/// Compares two equally long runs of RGBA pixels.
fn diff_pixels(
    left: &[u8],
    right: &[u8],
    diff_image: Option<&mut [u8]>,
    threshold: Threshold,
) -> u64 {
    let pixels = left.as_chunks::<4>().0.iter().zip(right.as_chunks::<4>().0);
    match diff_image {
        // Kept apart from the painting loop, so counting alone writes nothing.
        None => pixels
            .filter(|(left, right)| differs(left, right, threshold))
            .count() as u64,
        Some(diff_image) => pixels
            .zip(diff_image.as_chunks_mut::<4>().0)
            .map(|((left, right), out)| {
                let differs = differs(left, right, threshold);
                *out = if differs { DIFF_COLOUR } else { faded(left) };
                u64::from(differs)
            })
            .sum(),
    }
}

fn differs(left: &[u8; 4], right: &[u8; 4], threshold: Threshold) -> bool {
    // Most pixels of a typical comparison are identical; this skips the colour maths.
    if left == right {
        return false;
    }
    match threshold {
        Threshold::Exact => true,
        Threshold::Yiq(max) => yiq_delta(left, right) > max,
    }
}

/// The squared YIQ difference of two pixels, after blending each onto white by its alpha.
fn yiq_delta(left: &[u8; 4], right: &[u8; 4]) -> f64 {
    let [r1, g1, b1] = blend_onto_white(left);
    let [r2, g2, b2] = blend_onto_white(right);
    let (r, g, b) = (r1 - r2, g1 - g2, b1 - b2);

    // YIQ is a linear transform of RGB, so the difference transforms directly.
    let y = luma(r, g, b);
    let i = r * 0.595_977_99 - g * 0.274_176_10 - b * 0.321_801_89;
    let q = r * 0.211_470_17 - g * 0.522_617_11 + b * 0.311_146_94;
    0.5053 * y * y + 0.299 * i * i + 0.1957 * q * q
}

fn luma(r: f64, g: f64, b: f64) -> f64 {
    r * 0.298_895_31 + g * 0.586_622_47 + b * 0.114_482_23
}

fn blend_onto_white([r, g, b, a]: &[u8; 4]) -> [f64; 3] {
    let alpha = f64::from(*a) / 255.0;
    let blend = |channel: u8| 255.0 + (f64::from(channel) - 255.0) * alpha;
    [blend(*r), blend(*g), blend(*b)]
}

/// An unchanged pixel in the diff image: its luma, mostly washed out to white.
fn faded(pixel: &[u8; 4]) -> [u8; 4] {
    let [r, g, b] = blend_onto_white(pixel);
    let grey = (255.0 + (luma(r, g, b) - 255.0) * FADE).round() as u8;
    [grey, grey, grey, 255]
}
