use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use image_core::{
    decode_image_from_path, encode_atomic, ensure_parent_dir, OutputFormat, MAX_FILE_BYTES,
};
use task_runtime::CancellationToken;

use crate::error::{ProviderError, ProviderErrorCode};
use crate::map_input_image_error;
use crate::models::{
    now_rfc3339, ImageEditProvider, ProgressCallback, ProviderInfo, ProviderOutput,
};
use crate::registry::{ProviderRegistry, MOCK_PROVIDER_ID, MOCK_PROVIDER_NAME};

/// Fully local, deterministic demo provider. It never touches the network and
/// produces either a subtle warm/soft variant or a grayscale variant when the
/// prompt contains 黑白.
#[derive(Debug, Clone)]
pub struct MockProvider {
    id: String,
    name: String,
    created_at: String,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            id: MOCK_PROVIDER_ID.to_string(),
            name: MOCK_PROVIDER_NAME.to_string(),
            created_at: now_rfc3339(),
        }
    }

    pub fn with_identity(
        id: impl Into<String>,
        name: impl Into<String>,
        created_at: String,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            created_at,
        }
    }
}

#[async_trait]
impl ImageEditProvider for MockProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: crate::ProviderKind::Mock,
            base_url: None,
            model: None,
            has_credential: false,
            capabilities: ProviderRegistry::capabilities_for(crate::ProviderKind::Mock),
            created_at: self.created_at.clone(),
        }
    }

    async fn validate(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn list_models(&self) -> Result<Vec<String>, ProviderError> {
        Ok(vec!["luma-demo-v1".to_string()])
    }

    async fn execute(
        &self,
        input_path: &Path,
        prompt: &str,
        strength: f32,
        output_path: &Path,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> Result<ProviderOutput, ProviderError> {
        cancel.check().map_err(|_| ProviderError::cancelled())?;
        let metadata = std::fs::metadata(input_path).map_err(|e| {
            ProviderError::new(
                ProviderErrorCode::InvalidResponse,
                format!("cannot read mock input: {e}"),
            )
            .with_user_message("无法读取源图片，请重新导入。")
        })?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(ProviderError::new(
                ProviderErrorCode::InputTooLarge,
                format!("input is {} bytes", metadata.len()),
            ));
        }

        let mut image = decode_image_from_path(input_path).map_err(map_input_image_error)?;
        progress(20);
        pause(&cancel).await?;

        let grayscale = prompt.contains("黑白") || {
            let lower = prompt.to_ascii_lowercase();
            lower.contains("grayscale")
                || lower.contains("black and white")
                || lower.contains("monochrome")
        };
        let amount = 0.35 + 0.65 * (strength.clamp(0.0, 100.0) / 100.0);
        for pixel in image.rgba8.chunks_exact_mut(4) {
            let r = pixel[0] as f32;
            let g = pixel[1] as f32;
            let b = pixel[2] as f32;
            if grayscale {
                let y = 0.299 * r + 0.587 * g + 0.114 * b;
                for channel in &mut pixel[..3] {
                    let target = y;
                    *channel = (*channel as f32 + (target - *channel as f32) * amount)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            } else {
                let targets = [r * 1.05 + 6.0, g * 1.015 + 2.0, b * 0.96];
                for (channel, target) in pixel[..3].iter_mut().zip(targets) {
                    *channel = (*channel as f32 + (target - *channel as f32) * amount)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
        }
        progress(70);
        pause(&cancel).await?;

        ensure_parent_dir(output_path).map_err(map_input_image_error)?;
        let bytes = encode_atomic(output_path, &image, OutputFormat::Png, 100)
            .map_err(map_input_image_error)?;
        progress(100);
        Ok(ProviderOutput {
            path: output_path.to_path_buf(),
            width: image.width,
            height: image.height,
            bytes,
        })
    }
}

async fn pause(cancel: &CancellationToken) -> Result<(), ProviderError> {
    tokio::select! {
        _ = cancel.cancelled() => Err(ProviderError::cancelled()),
        _ = tokio::time::sleep(Duration::from_millis(5)) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image_core::DecodedImage;
    use std::sync::{Arc, Mutex};

    fn write_sample(path: &Path) {
        let mut rgba = Vec::new();
        for _ in 0..8 * 8 {
            rgba.extend_from_slice(&[220, 80, 40, 255]);
        }
        let image = DecodedImage::new(8, 8, rgba).unwrap();
        encode_atomic(path, &image, OutputFormat::Png, 100).unwrap();
    }

    #[tokio::test]
    async fn mock_provider_writes_warm_and_grayscale_results() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.png");
        write_sample(&input);
        let provider = MockProvider::new();
        let progress_log = Arc::new(Mutex::new(Vec::new()));
        let log = progress_log.clone();
        let callback: ProgressCallback = Arc::new(move |value| {
            log.lock().unwrap().push(value);
        });

        let output = dir.path().join("out.png");
        provider
            .execute(
                &input,
                "让照片温暖柔和",
                60.0,
                &output,
                callback.clone(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(output.exists());
        assert_eq!(*progress_log.lock().unwrap(), vec![20, 70, 100]);

        let progress_log2 = Arc::new(Mutex::new(Vec::new()));
        let log2 = progress_log2.clone();
        let callback2: ProgressCallback = Arc::new(move |value| {
            log2.lock().unwrap().push(value);
        });
        let gray = dir.path().join("gray.png");
        provider
            .execute(
                &input,
                "黑白风格",
                100.0,
                &gray,
                callback2,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let decoded = decode_image_from_path(&gray).unwrap();
        for pixel in decoded.rgba8.chunks_exact(4) {
            assert!((pixel[0] as i16 - pixel[1] as i16).abs() <= 1);
            assert!((pixel[1] as i16 - pixel[2] as i16).abs() <= 1);
        }
    }

    #[tokio::test]
    async fn mock_provider_honors_pre_cancelled_token() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.png");
        write_sample(&input);
        let token = CancellationToken::new();
        token.cancel();
        let err = MockProvider::new()
            .execute(
                &input,
                "warm",
                50.0,
                &dir.path().join("out.png"),
                Arc::new(|_| {}),
                token,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ProviderErrorCode::Cancelled);
    }
}
