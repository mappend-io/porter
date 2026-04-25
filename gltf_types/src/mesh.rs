use super::*;
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

    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}
