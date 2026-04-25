use super::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::Validate;

#[skip_serializing_none]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct Sampler {
    pub mag_filter: Option<MagFilter>,
    pub min_filter: Option<MinFilter>,
    pub wrap_s: Option<WrapMode>,
    pub wrap_t: Option<WrapMode>,
    pub name: Option<String>,
    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl Sampler {
    pub fn wrap_s(&self) -> WrapMode {
        self.wrap_s.unwrap_or(WrapMode::Repeat)
    }

    pub fn wrap_t(&self) -> WrapMode {
        self.wrap_t.unwrap_or(WrapMode::Repeat)
    }
}
