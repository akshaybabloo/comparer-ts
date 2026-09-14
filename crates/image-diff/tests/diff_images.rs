use image_diff::{DiffError, DiffOptions, diff_band, diff_encoded, diff_rgba, encode_png};

const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn fill(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
    pixel.repeat((width * height) as usize)
}

fn set_pixel(image: &mut [u8], width: u32, x: u32, y: u32, pixel: [u8; 4]) {
    let start = ((y * width + x) * 4) as usize;
    image[start..start + 4].copy_from_slice(&pixel);
}

fn tolerance(tolerance: f64) -> DiffOptions {
    DiffOptions {
        tolerance,
        diff_image: false,
    }
}

/// Counts differing pixels between two single-pixel images.
fn count(left: [u8; 4], right: [u8; 4], at: f64) -> u64 {
    diff_rgba(&left, &right, 1, 1, &tolerance(at))
        .unwrap()
        .different_pixels
}

#[test]
fn identical_images_have_no_differences() {
    let image = fill(8, 5, [12, 34, 56, 255]);

    let diff = diff_rgba(&image, &image, 8, 5, &tolerance(0.0)).unwrap();

    assert_eq!(diff.different_pixels, 0);
    assert_eq!(diff.total_pixels(), 40);
    assert!(diff.identical());
    assert_eq!(diff.diff_image, None);
}

#[test]
fn zero_tolerance_counts_the_smallest_change() {
    assert_eq!(count([100, 100, 100, 255], [101, 100, 100, 255], 0.0), 1);
}

#[test]
fn a_little_tolerance_ignores_the_smallest_change() {
    assert_eq!(count([100, 100, 100, 255], [101, 100, 100, 255], 1.0), 0);
}

#[test]
fn zero_tolerance_counts_changes_hidden_by_transparency() {
    assert_eq!(count([100, 100, 100, 255], [100, 100, 100, 254], 0.0), 1);
    // Both fully transparent, so they look the same, but their bytes are not.
    assert_eq!(count([255, 0, 0, 0], [0, 0, 255, 0], 0.0), 1);
    assert_eq!(count([255, 0, 0, 0], [0, 0, 255, 0], 1.0), 0);
}

#[test]
fn full_tolerance_ignores_even_black_against_white() {
    assert_eq!(count(BLACK, WHITE, 90.0), 1);
    assert_eq!(count(BLACK, WHITE, 100.0), 0);
}

#[test]
fn full_tolerance_ignores_every_change() {
    // The colour difference is a convex quadratic in the channel differences, so it
    // peaks at a corner of the colour cube: each channel fully on in one pixel and off
    // in the other. None of those may be over the largest difference 100 allows.
    for corner in 0..8u8 {
        let left = [
            255 * (corner & 1),
            255 * ((corner >> 1) & 1),
            255 * ((corner >> 2) & 1),
            255,
        ];
        let right = [255 - left[0], 255 - left[1], 255 - left[2], 255];
        assert_eq!(count(left, right, 100.0), 0, "{left:?} vs {right:?}");
    }
}

#[test]
fn diff_image_paints_differences_red_and_the_rest_faded() {
    let (width, height) = (3, 2);
    let left = fill(width, height, BLACK);
    let mut right = left.clone();
    set_pixel(&mut right, width, 2, 1, WHITE);

    let options = DiffOptions {
        tolerance: 0.0,
        diff_image: true,
    };
    let diff = diff_rgba(&left, &right, width, height, &options).unwrap();

    assert_eq!(diff.different_pixels, 1);
    let image = diff.diff_image.unwrap();
    assert_eq!(&image[20..24], &[255, 0, 0, 255]);
    // Black washed out to a light grey.
    assert_eq!(&image[0..4], &[230, 230, 230, 255]);
}

/// Deterministic pseudo-random bytes.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

/// Compares band by band with [`diff_band`], `rows` rows at a time.
fn diff_in_bands(
    left: &[u8],
    right: &[u8],
    width: u32,
    rows: usize,
    tolerance: f64,
) -> (u64, Vec<u8>) {
    let band = rows * width as usize * 4;
    let mut image = vec![0; left.len()];
    let count = left
        .chunks(band)
        .zip(right.chunks(band))
        .zip(image.chunks_mut(band))
        .map(|((left, right), out)| diff_band(left, right, Some(out), tolerance).unwrap())
        .sum();
    (count, image)
}

#[test]
fn every_split_into_bands_gives_the_same_result() {
    // Odd dimensions, so most splits leave a short last band.
    let (width, height) = (97, 61);
    let len = (width * height * 4) as usize;
    let left = noise(len, 1);
    let mut right = left.clone();
    // Change about a third of the bytes, by amounts from tiny to large.
    for (index, byte) in noise(len, 2).into_iter().enumerate() {
        if byte % 3 == 0 {
            right[index] = right[index].wrapping_add(byte % 40);
        }
    }

    for tolerance in [0.0, 2.5, 30.0] {
        let (expected, expected_image) =
            diff_in_bands(&left, &right, width, height as usize, tolerance);
        assert!(expected > 0, "tolerance {tolerance}");
        assert!(
            expected < u64::from(width * height),
            "tolerance {tolerance}"
        );

        for rows in [1, 3, 7, 200] {
            let (count, image) = diff_in_bands(&left, &right, width, rows, tolerance);
            assert_eq!(
                count, expected,
                "{rows} rows per band, tolerance {tolerance}"
            );
            assert!(
                image == expected_image,
                "{rows} rows per band, tolerance {tolerance}"
            );
        }

        // `diff_rgba` makes one band per thread, so each pool size splits it differently.
        for threads in [1, 2, 4, 7, 64] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let options = DiffOptions {
                tolerance,
                diff_image: true,
            };
            let diff = pool.install(|| diff_rgba(&left, &right, width, height, &options).unwrap());
            assert_eq!(
                diff.different_pixels, expected,
                "{threads} threads, tolerance {tolerance}"
            );
            assert!(
                diff.diff_image.unwrap() == expected_image,
                "{threads} threads, tolerance {tolerance}"
            );
        }
    }
}

#[test]
fn encoded_images_compare_like_their_pixels() {
    let (width, height) = (4, 3);
    let left = fill(width, height, [200, 100, 50, 255]);
    let mut right = left.clone();
    set_pixel(&mut right, width, 1, 1, BLACK);
    set_pixel(&mut right, width, 3, 2, [200, 100, 51, 255]);

    let options = DiffOptions {
        tolerance: 0.0,
        diff_image: true,
    };
    let encoded = diff_encoded(
        &encode_png(&left, width, height).unwrap(),
        &encode_png(&right, width, height).unwrap(),
        &options,
    )
    .unwrap();

    assert_eq!(
        encoded,
        diff_rgba(&left, &right, width, height, &options).unwrap()
    );
    assert_eq!(encoded.different_pixels, 2);
}

#[test]
fn empty_images_are_identical() {
    let diff = diff_rgba(&[], &[], 0, 4, &tolerance(0.0)).unwrap();

    assert_eq!(diff.total_pixels(), 0);
    assert!(diff.identical());
}

#[test]
fn rejects_a_tolerance_out_of_range() {
    for bad in [-1.0, 100.5, f64::NAN] {
        let error = diff_rgba(&WHITE, &WHITE, 1, 1, &tolerance(bad)).unwrap_err();
        assert!(matches!(error, DiffError::InvalidTolerance(_)), "{bad}");
    }
}

#[test]
fn rejects_a_buffer_that_does_not_match_its_dimensions() {
    let error = diff_rgba(
        &fill(2, 2, WHITE),
        &fill(2, 1, WHITE),
        2,
        2,
        &tolerance(0.0),
    );

    assert_eq!(
        error.unwrap_err().to_string(),
        "right buffer is 8 bytes, expected 16"
    );
}

#[test]
fn rejects_images_of_different_sizes() {
    let left = encode_png(&fill(2, 2, WHITE), 2, 2).unwrap();
    let right = encode_png(&fill(3, 2, WHITE), 3, 2).unwrap();

    let error = diff_encoded(&left, &right, &tolerance(0.0)).unwrap_err();

    assert_eq!(
        error.to_string(),
        "images differ in size: left is 2x2, right is 3x2"
    );
}

#[test]
fn rejects_bytes_that_are_not_an_image() {
    let image = encode_png(&WHITE, 1, 1).unwrap();

    let error = diff_encoded(&image, b"not an image", &tolerance(0.0)).unwrap_err();

    assert!(
        matches!(error, DiffError::Decode { side: "right", .. }),
        "{error}"
    );
}
