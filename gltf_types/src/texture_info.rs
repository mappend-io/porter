use super::*;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TextureInfo {
    pub index: TextureIndex,

    // TODO: Use a skip_serializing_if is default instead,
    // I'd rather remove the option here
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tex_coord: Option<u32>,

    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl TextureInfo {
    pub fn tex_coord(&self) -> u32 {
        self.tex_coord.unwrap_or(0)
    }
}
