use anyhow::{Context, Result, ensure};
use bytes::Bytes;

const GLB_MAGIC: &[u8; 4] = b"glTF";
const CHUNK_TYPE_JSON: &[u8; 4] = b"JSON";
const CHUNK_TYPE_BIN: &[u8; 4] = b"BIN\0";

pub struct GlbChunks {
    pub json: Bytes,
    pub bin: Option<Bytes>,
    pub extra: Vec<([u8; 4], Bytes)>,
}

pub fn extract_glb_chunks(mut data: Bytes) -> Result<GlbChunks> {
    // Validate size
    ensure!(data.len() >= 12, "Buffer too small to hold GLB header");

    // Validate magic
    let magic: [u8; 4] = data.split_to(4).as_ref().try_into().unwrap();
    ensure!(magic == *GLB_MAGIC, "Invalid GLB magic number");

    // Validate version
    let version = u32::from_le_bytes(data.split_to(4).as_ref().try_into().unwrap());
    ensure!(version == 2, "Unsupported GLB version={version}, want 2");

    // Consume the length, but we don't actually use it here
    let _ = u32::from_le_bytes(data.split_to(4).as_ref().try_into().unwrap());

    // Placeholders
    let mut json = None;
    let mut bin = None;
    let mut extra = Vec::new();

    // Consume all chunks, we need at least 8 bytes per chunk
    while data.len() >= 8 {
        let chunk_len = u32::from_le_bytes(data.split_to(4).as_ref().try_into().unwrap()) as usize;
        let chunk_type: [u8; 4] = data.split_to(4).as_ref().try_into().unwrap();

        ensure!(
            data.len() >= chunk_len,
            "Buffer not large enough for chunk payload"
        );

        ensure!(
            chunk_len.is_multiple_of(4),
            "Chunk length must be aligned to 4-byte boundary"
        );

        // Consume payload and advance data
        let chunk_payload = data.split_to(chunk_len);

        // Assign to our placeholder based on type and count rules
        match &chunk_type {
            CHUNK_TYPE_JSON if json.is_none() => json = Some(chunk_payload),
            CHUNK_TYPE_BIN if bin.is_none() => bin = Some(chunk_payload),
            _ => extra.push((chunk_type, chunk_payload)),
        }
    }

    Ok(GlbChunks {
        json: json.context("GLB must have JSON chunk")?,
        bin,
        extra,
    })
}
