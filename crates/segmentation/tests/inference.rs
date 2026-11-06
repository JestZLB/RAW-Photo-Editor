//! Real inference through `tract`, using a hand-built ONNX probe.
//!
//! `make_probe_model.py` generates a ReduceMean model that maps
//! `[1,3,512,512]` to `[1,1,512,512]`. It is not a segmentation model, but it
//! performs real arithmetic, so these tests cover model parsing, input fact
//! overriding, optimization, inference, output layout handling and the tiled
//! feathered assembly — everything except semantic quality (ADR-015).

use std::path::{Path, PathBuf};

use image_core::DecodedImage;
use segmentation::{SegmentationModel, TILE};

fn model_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/channel_mean.onnx")
}

fn checkerboard(width: u32, height: u32, cell: u32) -> DecodedImage {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let bright = ((x / cell) + (y / cell)) % 2 == 0;
            let value = if bright { 235u8 } else { 20u8 };
            rgba.extend_from_slice(&[value, value, value, 255]);
        }
    }
    DecodedImage::new(width, height, rgba).expect("checkerboard")
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

#[test]
fn a_real_onnx_model_loads_and_infers() {
    assert!(model_path().is_file(), "run make_probe_model.py first");
    let model = SegmentationModel::load(&model_path()).expect("probe model loads");

    let coverage = model
        .segment(&checkerboard(64, 48, 4))
        .expect("inference runs");

    assert_eq!((coverage.width, coverage.height), (64, 48));
    assert_eq!(coverage.values.len(), 64 * 48);
    assert!(
        coverage
            .values
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0 && *value <= 1.0),
        "coverage must be a probability map"
    );
    // ReduceMean over three identical channels reproduces the input; the
    // checkerboard averages (235 + 20) / 2 = 127.5 / 255 = 0.5.
    let average = mean(&coverage.values);
    assert!(
        (average - 0.5).abs() < 0.05,
        "unexpected probe output mean: {average}"
    );
}

#[test]
fn tiled_inference_covers_every_pixel_without_holes() {
    let model = SegmentationModel::load(&model_path()).expect("probe model loads");
    // Both axes exceed one tile, so the 512px windows must overlap and blend.
    let width = 700u32;
    let height = 600u32;
    let coverage = model
        .segment(&checkerboard(width, height, 16))
        .expect("tiled inference runs");

    assert_eq!((coverage.width, coverage.height), (width, height));
    assert_eq!(coverage.values.len(), (width * height) as usize);
    assert!(coverage.values.iter().all(|value| value.is_finite()));

    // Every tile seam must be covered: sample the columns where two 512px
    // windows meet (the last tile starts at 700-512 = 188).
    for y in [0usize, 300, 599] {
        for x in [0usize, 188, 511, 512, 699] {
            let value = coverage.values[y * width as usize + x];
            assert!((0.0..=1.0).contains(&value), "hole at ({x},{y}): {value}");
        }
    }

    let average = mean(&coverage.values);
    assert!(
        (average - 0.5).abs() < 0.06,
        "tiling must not shift the overall level: {average}"
    );
    assert_eq!(TILE, 512, "the fixture is built for a 512px tile");
}
