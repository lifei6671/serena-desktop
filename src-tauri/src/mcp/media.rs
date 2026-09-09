use super::{Broker, ServerStatus, process};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, imageops::FilterType};
use rmcp::{
    model::{CallToolResult, ContentBlock},
    schemars::{self, JsonSchema},
};
use serde::Deserialize;
use std::{
    io::{Cursor, Read},
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const MAX_INPUT: u64 = 20 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;
const MAX_EDGE: u32 = 2560;
const MAX_OUTPUT: usize = 6 * 1024 * 1024;
const ENCODE_ATTEMPTS: u32 = 6;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaReadImageArgs {
    /// Image path relative to the active workspace root.
    pub path: String,
}

impl Broker {
    pub async fn read_image(
        &self,
        args: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<CallToolResult, String> {
        let args: MediaReadImageArgs =
            serde_json::from_value(args).map_err(|e| format!("INVALID_PARAMS: {e}"))?;
        let work = async {
            let slot = self.workspace.read().await;
            let active = slot.as_ref().ok_or("NO_ACTIVE_WORKSPACE")?;
            let snapshot = self.supervisor.snapshot();
            if snapshot.server_status != ServerStatus::Running
                || snapshot.process_id != Some(active.pid)
                || active.client.closed()
            {
                return Err("NO_ACTIVE_WORKSPACE".into());
            }
            let root = active.workspace.root.clone();
            let worker_cancel = cancel.clone();
            // Keep the binding locked throughout the local read and CPU work.
            let result =
                tokio::task::spawn_blocking(move || read(&root, &args.path, &worker_cancel))
                    .await
                    .map_err(|e| format!("IMAGE_DECODE_FAILED: {e}"))?;
            drop(slot);
            result
        };
        tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(60), work) => result.unwrap_or_else(|_| { cancel.cancel(); Err("TOOL_TIMEOUT".into()) }),
            _ = cancel.cancelled() => Err("CANCELLED".into()),
        }
    }
}

fn read(root: &Path, path: &str, cancel: &CancellationToken) -> Result<CallToolResult, String> {
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    let path = process::safe_relative(root, path)?;
    let metadata = std::fs::metadata(&path).map_err(|e| format!("INVALID_PATH: {e}"))?;
    if !metadata.is_file() {
        return Err("INVALID_PATH: expected a regular file".into());
    }
    let file = std::fs::File::open(path).map_err(|e| format!("FILE_NOT_FOUND: {e}"))?;
    let metadata = file.metadata().map_err(|e| format!("INVALID_PATH: {e}"))?;
    if metadata.len() > MAX_INPUT {
        return Err("OUTPUT_LIMIT_EXCEEDED: input exceeds 20 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("IMAGE_DECODE_FAILED: {e}"))?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err("OUTPUT_LIMIT_EXCEEDED: input exceeds 20 MiB".into());
    }
    let format = image::guess_format(&bytes).map_err(|_| "UNSUPPORTED_MEDIA_TYPE")?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err("UNSUPPORTED_MEDIA_TYPE".into());
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|e| format!("IMAGE_DECODE_FAILED: {e}"))?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("OUTPUT_LIMIT_EXCEEDED: image exceeds 40 MP".into());
    }
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    let decoded =
        DynamicImage::from_decoder(decoder).map_err(|e| format!("IMAGE_DECODE_FAILED: {e}"))?;
    encode(decoded, format, MAX_OUTPUT, cancel)
}

#[cfg(test)]
#[path = "media_tests.rs"]
pub(super) mod tests;

fn encode(
    mut decoded: DynamicImage,
    format: ImageFormat,
    budget: usize,
    cancel: &CancellationToken,
) -> Result<CallToolResult, String> {
    if decoded.width().max(decoded.height()) > MAX_EDGE {
        decoded = decoded.resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle);
    }
    // WebP uses PNG output: preserve alpha without adding another encoding policy.
    let jpeg = format == ImageFormat::Jpeg;
    let mime = if jpeg { "image/jpeg" } else { "image/png" };
    for attempt in 0..ENCODE_ATTEMPTS {
        if cancel.is_cancelled() {
            return Err("CANCELLED".into());
        }
        let mut output = Cursor::new(Vec::new());
        if jpeg {
            image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut output,
                (85 - attempt * 10) as u8,
            )
            .encode_image(&decoded.to_rgb8())
        } else {
            decoded.write_to(&mut output, ImageFormat::Png)
        }
        .map_err(|e| format!("IMAGE_DECODE_FAILED: {e}"))?;
        let bytes = output.into_inner();
        if bytes.len() <= budget {
            return Ok(CallToolResult::success(vec![ContentBlock::image(
                STANDARD.encode(bytes),
                mime,
            )]));
        }
        decoded = decoded.resize(
            (decoded.width() * 3 / 4).max(1),
            (decoded.height() * 3 / 4).max(1),
            FilterType::Triangle,
        );
    }
    Err("OUTPUT_LIMIT_EXCEEDED".into())
}
