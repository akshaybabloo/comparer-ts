use comparer::{ImagePair, compare_images, compare_images_rgba};
use image_diff::encode_png;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// A 2x2 image of `pixel`, with its last pixel replaced by `last`.
fn image(pixel: [u8; 4], last: [u8; 4]) -> Vec<u8> {
    [pixel, pixel, pixel, last].concat()
}

#[test]
fn compare_images_rgba_counts_differing_pixels() {
    let comparison =
        compare_images_rgba(&image(RED, RED), &image(RED, BLUE), 2, 2, 0.0, false, false).unwrap();

    assert_eq!((comparison.width(), comparison.height()), (2, 2));
    assert_eq!(comparison.different_pixels(), 1.0);
}

#[test]
fn diff_images_are_only_painted_in_the_forms_asked_for() {
    let (left, right) = (image(RED, RED), image(RED, BLUE));

    let mut neither = compare_images_rgba(&left, &right, 2, 2, 0.0, false, false).unwrap();
    assert_eq!(neither.take_diff_rgba(), None);
    assert_eq!(neither.take_diff_png(), None);

    let mut rgba = compare_images_rgba(&left, &right, 2, 2, 0.0, true, false).unwrap();
    assert_eq!(rgba.take_diff_png(), None);
    let pixels = rgba.take_diff_rgba().unwrap();
    assert_eq!(pixels.len(), 16);
    assert_eq!(&pixels[12..], &RED);
    // Taken, so a second read has nothing to copy.
    assert_eq!(rgba.take_diff_rgba(), None);

    let mut png = compare_images_rgba(&left, &right, 2, 2, 0.0, false, true).unwrap();
    assert_eq!(png.take_diff_rgba(), None);
    assert!(
        png.take_diff_png()
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );

    let mut both = compare_images_rgba(&left, &right, 2, 2, 0.0, true, true).unwrap();
    assert_eq!(both.take_diff_rgba(), Some(pixels));
    assert!(both.take_diff_png().is_some());
}

#[test]
fn compare_images_decodes_the_png_it_paints() {
    let (left, right) = (image(RED, RED), image(RED, BLUE));
    let same = compare_images_rgba(&left, &left, 2, 2, 0.0, false, true)
        .unwrap()
        .take_diff_png()
        .unwrap();
    let changed = compare_images_rgba(&left, &right, 2, 2, 0.0, false, true)
        .unwrap()
        .take_diff_png()
        .unwrap();

    // The two diff images differ only in the one pixel painted red.
    let comparison = compare_images(&same, &changed, 0.0, false, false).unwrap();

    assert_eq!((comparison.width(), comparison.height()), (2, 2));
    assert_eq!(comparison.different_pixels(), 1.0);
}

#[test]
fn compare_images_reports_why_it_failed() {
    let error = compare_images(b"not", b"images", 0.0, false, false)
        .err()
        .unwrap();

    assert!(
        error
            .to_string()
            .starts_with("left image could not be decoded"),
        "{error}"
    );
}

fn png(pixel: [u8; 4], last: [u8; 4]) -> Vec<u8> {
    encode_png(&image(pixel, last), 2, 2).unwrap()
}

#[test]
fn image_pair_compares_at_any_tolerance_without_decoding_again() {
    let pair = ImagePair::new(&png(RED, RED), &png(RED, [250, 0, 0, 255])).unwrap();

    assert!(pair.same_size());
    assert_eq!(
        pair.compare(0.0, false, false).unwrap().different_pixels(),
        1.0
    );
    assert_eq!(
        pair.compare(10.0, false, false).unwrap().different_pixels(),
        0.0
    );
    // And back again: comparing never consumes the decoded pixels.
    let mut again = pair.compare(0.0, false, true).unwrap();
    assert_eq!(again.different_pixels(), 1.0);
    assert!(again.take_diff_png().is_some());
}

#[test]
fn image_pair_reports_both_sizes_but_refuses_to_compare_different_ones() {
    let wide = encode_png(&[RED; 6].concat(), 3, 2).unwrap();
    let pair = ImagePair::new(&png(RED, RED), &wide).unwrap();

    assert!(!pair.same_size());
    assert_eq!((pair.left_width(), pair.left_height()), (2, 2));
    assert_eq!((pair.right_width(), pair.right_height()), (3, 2));
    assert_eq!(
        pair.compare(0.0, false, false).err().unwrap().to_string(),
        "images differ in size: left is 2x2, right is 3x2"
    );
}

#[test]
fn image_pair_names_the_image_that_cannot_be_decoded() {
    let error = ImagePair::new(&png(RED, RED), b"nope").err().unwrap();

    assert!(
        error
            .to_string()
            .starts_with("right image could not be decoded")
    );
}
