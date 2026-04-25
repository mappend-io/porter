use super::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::Validate;

#[skip_serializing_none]
#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct Asset {
    pub copyright: Option<String>,
    pub generator: Option<String>,
    #[validate(length(min = 1))]
    pub version: String, // TODO: Pattern https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#schema-reference-asset
    pub min_version: Option<String>, // TODO: Pattern https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#schema-reference-asset
    #[serde(flatten)]
    #[validate(nested)]
    pub root_property: Property,
}

impl Default for Asset {
    fn default() -> Self {
        Self {
            copyright: None,
            generator: None,
            version: "2.0".to_string(),
            min_version: None,
            root_property: Property::default(),
        }
    }
}
