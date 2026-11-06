//! Wire-format guard for the TypeScript mirror.
//!
//! `apps/desktop/src/api/types.ts` is hand-written against these structs and
//! nothing else checks the two agree: renaming a Rust field, or dropping a
//! `rename_all = "camelCase"`, type-checks fine on both sides and only breaks at
//! runtime. These tests pin the JSON keys the frontend actually reads.

use app_core::{
    CommitChangesRequest, CreateVersionRequest, DocumentState, PreviewResult,
    RenderVersionPreviewRequest, SegmentMaskRequest, SegmentationCapability,
    UpdateLayerParamsRequest, VersionSummary,
};
use serde_json::{json, Value};

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .unwrap_or_else(|| panic!("expected an object, got {value}"))
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// A document shaped like the one the UI receives, including a raster mask so the
/// nested keys are covered too.
fn document_json() -> Value {
    json!({
        "id": "project-a",
        "name": "portrait",
        "collectionId": "collection-one",
        "collectionName": "人像",
        "schemaVersion": 20,
        "revision": 7,
        "createdAt": "2026-11-06T00:00:00Z",
        "updatedAt": "2026-11-06T00:00:00Z",
        "canvas": { "width": 4000, "height": 2000, "workingColorSpace": "linear-srgb" },
        "crop": { "x": 0, "y": 0, "width": 4000, "height": 2000, "angle": 0.0 },
        "source": {
            "assetId": "asset-1",
            "displayName": "portrait.jpg",
            "width": 4000,
            "height": 2000,
            "format": "jpeg",
            "originalPath": "C:/photos/portrait.jpg",
            "previewPath": "C:/cache/preview.png",
            "thumbnailPath": "C:/cache/thumb.png",
            "colorSpace": "sRGB",
            "hasSourceProfile": false
        },
        "layers": [{
            "id": "layer-color",
            "name": "调色",
            "kind": "color",
            "visible": true,
            "locked": false,
            "opacity": 1.0,
            "blendMode": "normal",
            "color": null,
            "portrait": null,
            "retouch": null,
            "mask": {
                "strokes": [],
                "autoSkin": false,
                "regions": [],
                "gradient": null,
                "inverted": false,
                "feather": 0.0,
                "density": 1.0,
                "raster": {
                    // `MaskRaster.asset_id` is a real UUID: the wire value must be
                    // a parseable string, not an opaque label.
                    "assetId": "0f8fad5b-d9cb-469f-a165-70867728950e",
                    "threshold": 0.5,
                    "softness": 0.15,
                    "source": "onnx:foreground"
                }
            },
            "assetId": null,
            "previewPath": null,
            "createdAt": "2026-11-06T00:00:00Z"
        }],
        "canUndo": true,
        "canRedo": false,
        "history": ["曝光"]
    })
}

fn round_trip(value: Value) -> Value {
    // Deserialising first proves the camelCase names the frontend sends are the
    // ones the struct actually accepts; serialising shows what it reads back.
    let document: DocumentState = serde_json::from_value(value).expect("document state parses");
    serde_json::to_value(&document).expect("document state serialises")
}

#[test]
fn document_state_keys_are_pinned() {
    let state = round_trip(document_json());
    assert_eq!(
        sorted_keys(&state),
        vec![
            "canRedo",
            "canUndo",
            "canvas",
            "collectionId",
            "collectionName",
            "createdAt",
            "crop",
            "history",
            "id",
            "layers",
            "name",
            "revision",
            "schemaVersion",
            "source",
            "updatedAt",
        ]
    );
    assert_eq!(
        sorted_keys(&state["canvas"]),
        vec!["height", "width", "workingColorSpace"]
    );
    assert_eq!(
        sorted_keys(&state["crop"]),
        vec!["angle", "height", "width", "x", "y"]
    );
    assert_eq!(
        sorted_keys(&state["source"]),
        vec![
            "assetId",
            "colorSpace",
            "displayName",
            "format",
            "hasSourceProfile",
            "height",
            "originalPath",
            "previewPath",
            "thumbnailPath",
            "width",
        ]
    );
}

#[test]
fn layer_and_mask_keys_are_pinned() {
    let state = round_trip(document_json());
    let layer = &state["layers"][0];
    assert_eq!(
        sorted_keys(layer),
        vec![
            "assetId",
            "blendMode",
            "color",
            "createdAt",
            "id",
            "kind",
            "locked",
            "mask",
            "name",
            "opacity",
            "portrait",
            "previewPath",
            "retouch",
            "visible",
        ]
    );

    // `regions` and `gradient` are omitted while empty: the TS side declares them
    // optional, so a mask that only carries a raster stays small on the wire.
    assert_eq!(
        sorted_keys(&layer["mask"]),
        vec!["autoSkin", "density", "feather", "inverted", "raster", "strokes"]
    );
    assert_eq!(
        sorted_keys(&layer["mask"]["raster"]),
        vec!["assetId", "softness", "source", "threshold"]
    );
}

#[test]
fn preview_version_and_capability_keys_are_pinned() {
    let preview: PreviewResult = serde_json::from_value(json!({
        "path": "C:/cache/render.png",
        "width": 1600,
        "height": 800,
        "revision": 7,
        "durationMs": 12,
        "cached": false,
        "backend": "cpu",
        "histogram": { "red": [], "green": [], "blue": [], "luma": [] }
    }))
    .expect("preview parses");
    let preview = serde_json::to_value(&preview).unwrap();
    assert_eq!(
        sorted_keys(&preview),
        vec![
            "backend",
            "cached",
            "durationMs",
            "height",
            "histogram",
            "path",
            "revision",
            "width",
        ]
    );
    assert_eq!(
        sorted_keys(&preview["histogram"]),
        vec!["blue", "green", "luma", "red"]
    );

    let version: VersionSummary = serde_json::from_value(json!({
        "id": "version-1",
        "name": "暖调",
        "revision": 7,
        "createdAt": "2026-11-06T00:00:00Z",
        "thumbnailPath": null
    }))
    .expect("version summary parses");
    assert_eq!(
        sorted_keys(&serde_json::to_value(&version).unwrap()),
        vec!["createdAt", "id", "name", "revision", "thumbnailPath"]
    );

    let capability: SegmentationCapability = serde_json::from_value(json!({
        "available": false,
        "reason": "尚未选择分割模型。"
    }))
    .expect("capability parses");
    assert_eq!(
        sorted_keys(&serde_json::to_value(&capability).unwrap()),
        vec!["available", "reason"]
    );
}

#[test]
fn request_payloads_accept_the_names_the_frontend_sends() {
    // Slider drag path.
    let params: UpdateLayerParamsRequest = serde_json::from_value(json!({
        "projectId": "project-a",
        "layerId": "layer-color",
        "color": null,
        "portrait": null,
        "retouch": null
    }))
    .expect("update_layer_params parses");
    assert_eq!(params.project_id, "project-a");
    assert_eq!(params.layer_id, "layer-color");

    let commit: CommitChangesRequest = serde_json::from_value(json!({
        "projectId": "project-a",
        "label": "曝光"
    }))
    .expect("commit_changes parses");
    assert_eq!(commit.label, "曝光");

    let version: CreateVersionRequest = serde_json::from_value(json!({
        "projectId": "project-a",
        "name": "暖调"
    }))
    .expect("create_version parses");
    assert_eq!(version.name, "暖调");

    let version_preview: RenderVersionPreviewRequest = serde_json::from_value(json!({
        "projectId": "project-a",
        "versionId": "version-1",
        "maxDimension": 1600
    }))
    .expect("render_version_preview parses");
    assert_eq!(version_preview.max_dimension, Some(1600));
    // `maxDimension` is optional: omitting it must keep working for callers that
    // just want the default preview size.
    let default_preview: RenderVersionPreviewRequest = serde_json::from_value(json!({
        "projectId": "project-a",
        "versionId": "version-1"
    }))
    .expect("optional maxDimension parses");
    assert_eq!(default_preview.max_dimension, None);

    let segment: SegmentMaskRequest = serde_json::from_value(json!({
        "projectId": "project-a",
        "layerId": "layer-color",
        "modelPath": "C:/models/seg.onnx",
        "threshold": 0.6,
        "softness": 0.2
    }))
    .expect("run_segmentation_mask parses");
    assert_eq!(segment.model_path, "C:/models/seg.onnx");
    assert_eq!(segment.threshold, Some(0.6));

    let segment_defaults: SegmentMaskRequest = serde_json::from_value(json!({
        "projectId": "project-a",
        "layerId": "layer-color",
        "modelPath": "C:/models/seg.onnx"
    }))
    .expect("optional threshold parses");
    assert_eq!(segment_defaults.threshold, None);
    assert_eq!(segment_defaults.softness, None);
}
