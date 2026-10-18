use super::Primitive;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::Validate;

#[skip_serializing_none]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct Mesh {
    #[validate(length(min = 1))]
    pub primitives: Vec<Primitive>,
    //pub weights: Vec<
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<serde_json::Value>,
}
