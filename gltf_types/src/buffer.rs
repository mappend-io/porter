use super::*;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct Buffer {
    // This is populated just after deserialization for glb embedded buffers,
    // otherwise it will be empty post-read.
    //
    // TODO: Document intention around this: if it's set and uri is set
    // during write, vs. if it's not set and uri is set.
    #[serde(skip)]
    pub data: Bytes,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[validate(range(min = 1))]
    pub byte_length: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

/*
//#[validate(schema(function = "validate_buffer"))]
pub fn validate_buffer(buffer: &Buffer) -> Result<(), ValidationError> {
    if buffer.byte_length == 0 {
        let mut err = ValidationError::new("EMPTY_BUFFER");
        err.message = Some("Buffer byte_length must be > 0".into());
        return Err(err);
    }
    Ok(())
}
*/
