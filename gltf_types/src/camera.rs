use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct Camera {}
