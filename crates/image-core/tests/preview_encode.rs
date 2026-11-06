//! The preview cache uses a faster lossless PNG encoder. Speed must never cost
//! pixels: both paths have to decode to exactly the source RGBA bytes.

use std::path::Path;

use image_core::{
    decode_image_from_path, encode_atomic, encode_atomic_preview, DecodedImage, OutputFormat,
};

fn noisy(width: u32, height: u32) -> DecodedImage {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    let mut state = 0x1234_5678u32;
    for y in 0..height {
        for x in 0..width {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 27) as u8;
            rgba.extend_from_slice(&[
                ((x % 251) as u8).saturating_add(noise),
                ((y % 241) as u8).saturating_add(noise),
                (((x + y) % 239) as u8).saturating_add(noise),
                255,
            ]);
        }
    }
    DecodedImage::new(width, height, rgba).unwrap()
}

#[test]
fn fast_preview_png_decodes_to_the_same_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let fast = dir.path().join("fast.png");
    let archive = dir.path().join("archive.png");
    let image = noisy(160, 120);

    encode_atomic_preview(&fast, &image).unwrap();
    encode_atomic(&archive, &image, OutputFormat::Png, 100).unwrap();

    let from_fast = decode_image_from_path(&fast).unwrap();
    let from_archive = decode_image_from_path(&archive).unwrap();
    assert_eq!((from_fast.width, from_fast.height), (160, 120));
    assert_eq!(from_fast.rgba8, image.rgba8);
    assert_eq!(from_archive.rgba8, image.rgba8);
    assert_eq!(from_fast.rgba8, from_archive.rgba8);
}

#[test]
fn fast_preview_still_requires_a_png_path() {
    let dir = tempfile::tempdir().unwrap();
    let wrong = dir.path().join("preview.jpg");
    let error = encode_atomic_preview(&wrong, &noisy(2, 2)).unwrap_err();
    assert!(
        error.to_string().contains("png") || error.to_string().contains("extension"),
        "unexpected error: {error}"
    );
    assert!(!wrong.exists(), "a rejected path must not leave a file");
}

#[test]
fn fast_preview_overwrites_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path: &Path = &dir.path().join("preview.png");
    let first = noisy(8, 8);
    let second = DecodedImage::new(8, 8, vec![9u8; 8 * 8 * 4]).unwrap();
    encode_atomic_preview(path, &first).unwrap();
    encode_atomic_preview(path, &second).unwrap();
    assert_eq!(decode_image_from_path(path).unwrap().rgba8, second.rgba8);
}
