use image_diff::{
    DecodedImage, DiffError, DiffOptions, ImageDiff, check_dimensions, decode, diff_encoded,
    encode_png,
};
use wasm_bindgen::prelude::*;

use crate::CompareError;

impl From<DiffError> for CompareError {
    fn from(error: DiffError) -> CompareError {
        CompareError(error.to_string())
    }
}

/// The result of comparing two images.
///
/// Each diff image is moved out with its `take_` method, so its pixels are copied into
/// JavaScript once rather than on every read.
#[wasm_bindgen]
pub struct ImageComparison {
    diff: ImageDiff,
    png: Option<Vec<u8>>,
}

impl ImageComparison {
    fn new(
        diff: Result<ImageDiff, DiffError>,
        diff_rgba: bool,
        diff_png: bool,
    ) -> Result<ImageComparison, CompareError> {
        let mut diff = diff?;
        let png = match (&diff.diff_image, diff_png) {
            (Some(rgba), true) => Some(encode_png(rgba, diff.width, diff.height)?),
            _ => None,
        };
        if !diff_rgba {
            diff.diff_image = None;
        }
        Ok(ImageComparison { diff, png })
    }
}

#[wasm_bindgen]
impl ImageComparison {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.diff.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.diff.height
    }

    /// A JavaScript number rather than a `BigInt`: WebAssembly memory cannot hold an
    /// image with more pixels than a number counts exactly.
    #[wasm_bindgen(getter)]
    pub fn different_pixels(&self) -> f64 {
        self.diff.different_pixels as f64
    }

    /// The diff image as RGBA, row by row, if it was asked for and not already taken.
    pub fn take_diff_rgba(&mut self) -> Option<Vec<u8>> {
        self.diff.diff_image.take()
    }

    /// The diff image as PNG, if it was asked for and not already taken.
    pub fn take_diff_png(&mut self) -> Option<Vec<u8>> {
        self.png.take()
    }
}

fn options(tolerance: f64, diff_rgba: bool, diff_png: bool) -> DiffOptions {
    DiffOptions {
        tolerance,
        diff_image: diff_rgba || diff_png,
    }
}

/// Decodes two PNG, JPEG, WebP, GIF or BMP images and compares them pixel by pixel.
///
/// `tolerance` runs from 0, where any change counts, to 100, where every change is
/// tolerated. `diff_rgba` and `diff_png` paint a diff image in either or both forms.
#[wasm_bindgen]
pub fn compare_images(
    left: &[u8],
    right: &[u8],
    tolerance: f64,
    diff_rgba: bool,
    diff_png: bool,
) -> Result<ImageComparison, CompareError> {
    let diff = diff_encoded(left, right, &options(tolerance, diff_rgba, diff_png));
    ImageComparison::new(diff, diff_rgba, diff_png)
}

/// Like [`compare_images`], but for raw RGBA pixels, row by row, such as a canvas's
/// `ImageData`.
#[wasm_bindgen]
pub fn compare_images_rgba(
    left: &[u8],
    right: &[u8],
    width: u32,
    height: u32,
    tolerance: f64,
    diff_rgba: bool,
    diff_png: bool,
) -> Result<ImageComparison, CompareError> {
    let options = options(tolerance, diff_rgba, diff_png);
    let diff = image_diff::diff_rgba(left, right, width, height, &options);
    ImageComparison::new(diff, diff_rgba, diff_png)
}

/// Two images decoded once and kept in WebAssembly memory, to be compared as often as
/// needed: a tolerance slider can re-run [`ImagePair::compare`] on every move without
/// paying for either decode again.
///
/// The images may differ in size. Their sizes stay readable so the difference can be
/// explained, but comparing them fails.
#[wasm_bindgen]
pub struct ImagePair {
    left: DecodedImage,
    right: DecodedImage,
}

#[wasm_bindgen]
impl ImagePair {
    /// Decodes two PNG, JPEG, WebP, GIF or BMP images.
    #[wasm_bindgen(constructor)]
    pub fn new(left: &[u8], right: &[u8]) -> Result<ImagePair, CompareError> {
        Ok(ImagePair {
            left: decode(left, "left")?,
            right: decode(right, "right")?,
        })
    }

    #[wasm_bindgen(getter)]
    pub fn left_width(&self) -> u32 {
        self.left.width
    }

    #[wasm_bindgen(getter)]
    pub fn left_height(&self) -> u32 {
        self.left.height
    }

    #[wasm_bindgen(getter)]
    pub fn right_width(&self) -> u32 {
        self.right.width
    }

    #[wasm_bindgen(getter)]
    pub fn right_height(&self) -> u32 {
        self.right.height
    }

    #[wasm_bindgen(getter)]
    pub fn same_size(&self) -> bool {
        check_dimensions(&self.left, &self.right).is_ok()
    }

    /// Compares the two images, like [`compare_images`] but without decoding them again.
    pub fn compare(
        &self,
        tolerance: f64,
        diff_rgba: bool,
        diff_png: bool,
    ) -> Result<ImageComparison, CompareError> {
        check_dimensions(&self.left, &self.right)?;
        let diff = image_diff::diff_rgba(
            &self.left.pixels,
            &self.right.pixels,
            self.left.width,
            self.left.height,
            &options(tolerance, diff_rgba, diff_png),
        );
        ImageComparison::new(diff, diff_rgba, diff_png)
    }
}
