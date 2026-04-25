use anyhow::{Context, Result};
use bytes::{BufMut, Bytes, BytesMut};

// TODO: What if other buffers have a uri.. we need to write to that, right..?
// TODO: Revisit what to do about images, too: if the model references a uri with a relative path,
// we probably need to return it as part of the package: "Here's the glb, and a map of dst to src paths you'd want to include also", somehow.
//
// NOTE: According to https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#glb-stored-buffer
// it's allowable, but not defined, what happens if any buffer other than the first
// has an undefined uri.
pub fn create_glb(model: &gltf_types::Document) -> Result<Bytes> {
    // Serialize the model to bytes
    let json_bytes = serde_json::to_vec(model).context("Could not serialize model")?;

    // Calculate the chunk length (including padding) so we can build the header
    let json_padding = (4 - (json_bytes.len() % 4)) % 4;
    let json_chunk_len = json_bytes.len() + json_padding;

    // Only the first buffer, if it has no uri, goes into the glTF's BIN chunk
    let bin_data = model
        .buffers
        .first()
        .and_then(|b| if b.uri.is_none() { Some(&b.data) } else { None });

    // Again, calculate the length, including padding
    let bin_padding = bin_data.map_or(0, |b| (4 - (b.len() % 4)) % 4);
    let bin_chunk_len = bin_data.map_or(0, |b| b.len() + bin_padding);

    // The header needs the entire length (header + JSON + (optional) BIN)
    let mut total_length = 12 + 8 + json_chunk_len;
    if bin_chunk_len > 0 {
        total_length += 8 + bin_chunk_len;
    }

    // Preallocate the buffer, we know exactly how big it needs to be now
    let mut buf = BytesMut::with_capacity(total_length);

    // Write header
    buf.put_slice(b"glTF"); // magic
    buf.put_u32_le(2); // version
    buf.put_u32_le(total_length as u32); // total length

    // First chunk is always glTF JSON
    buf.put_u32_le(json_chunk_len as u32); // length of this chunk
    buf.put_slice(b"JSON"); // magic
    buf.put_slice(&json_bytes); // json content bytes
    buf.put_bytes(b' ', json_padding); // pad with spaces to 4-byte boundary

    // If present, write BIN chunk from first Buffer
    if let Some(bin) = bin_data {
        buf.put_u32_le(bin_chunk_len as u32); // chunk length
        buf.put_slice(b"BIN\0"); // magic
        buf.put_slice(bin); // buffer contents
        buf.put_bytes(0x00, bin_padding); // pad with 0s to 4-byte boundary
    }

    // Freeze into immutable Bytes
    Ok(buf.freeze())
}
