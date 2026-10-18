use crate::utils::is_model_glb;
use anyhow::{Context, Result};
use iri_string::types::UriAbsoluteStr;

pub async fn read_model(
    bytes: bytes::Bytes,
    model_uri: &UriAbsoluteStr,
) -> Result<gltf_types::Document> {
    // Deserialize the json structure (offloaded to blocking pool)
    let model_uri = model_uri.to_owned();
    tokio::task::spawn_blocking(move || parse_model(bytes, &model_uri))
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {}", e))?
}

/// Synchronous counterpart of [`read_model`] for callers that are already off
/// the async runtime, e.g. parsing a batch of models in one blocking task.
pub fn parse_model(
    bytes: bytes::Bytes,
    model_uri: &UriAbsoluteStr,
) -> Result<gltf_types::Document> {
    let mut glb_chunks: Option<gltf_types::glb::GlbChunks> = None;
    let json_bytes = if is_model_glb(model_uri, &bytes) {
        // If it's a binary model, extract the JSON chunk
        glb_chunks = Some(
            gltf_types::glb::extract_glb_chunks(bytes.clone())
                .context("Could not extract glb chunks")?,
        );
        glb_chunks.as_ref().unwrap().json.clone()
    } else {
        // Otherwise it's just a normal gltf, the bytes are json already
        bytes.clone()
    };

    let mut model = serde_json::from_slice::<gltf_types::Document>(&json_bytes)
        .context("Could not deserialize model")?;

    // GLBs are unique: if the first buffer doesn't have a URI, it pulls from the BIN glb chunk
    if let Some(glb_chunks) = &glb_chunks
        && let Some(bin) = &glb_chunks.bin
        && !model.buffers.is_empty()
    {
        let first_buffer = &mut model.buffers[0];
        if first_buffer.uri.is_none() {
            first_buffer.data = bin.clone();
        }
    }

    Ok(model)
}
