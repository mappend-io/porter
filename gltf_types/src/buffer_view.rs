use super::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::{Validate, ValidationError};

#[skip_serializing_none]
#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct BufferView {
    pub buffer: BufferIndex,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<u32>,

    #[validate(range(min = 1))]
    pub byte_length: u32,

    // When two or more accessors use the same buffer view, this field **MUST** be defined.
    #[validate(custom(function = "validate_byte_stride"))]
    pub byte_stride: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl BufferView {
    pub fn byte_offset(&self) -> u32 {
        self.byte_offset.unwrap_or(0)
    }
}

fn validate_byte_stride(value: u32) -> Result<(), ValidationError> {
    if value.is_multiple_of(4) {
        let mut err = ValidationError::new("INVALID_BYTE_STRIDE");
        err.message = Some("BufferView::byteStride must be a multiple of 4".into());
        return Err(err);
    } else if !(4..=254).contains(&value) {
        let mut err = ValidationError::new("INVALID_BYTE_STRIDE");
        err.message = Some("BufferView::byteStride must between 4 and 254".into());
        return Err(err);
    }
    Ok(())
}
