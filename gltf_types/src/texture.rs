use super::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::Validate;

#[skip_serializing_none]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct Texture {
    pub sampler: Option<SamplerIndex>,
    pub source: Option<ImageIndex>,
    pub name: Option<String>,
    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}
