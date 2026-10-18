use super::{AccessorIndex, MaterialIndex, Mode, Semantic};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;
use validator::Validate;

#[skip_serializing_none]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct Primitive {
    /// A plain JSON object, where each key corresponds to a mesh attribute
    /// semantic and each value is the index of the accessor containing
    /// attribute's data.
    #[validate(length(min = 1))]
    pub attributes: BTreeMap<Semantic, AccessorIndex>,

    /// The index of the accessor that contains the vertex indices. When this is
    /// undefined, the primitive defines non-indexed geometry. When defined, the
    /// accessor **MUST** have `SCALAR` type and an unsigned integer component
    /// type.
    pub indices: Option<AccessorIndex>,

    /// The index of the material to apply to this primitive when rendering.
    pub material: Option<MaterialIndex>,

    /// The topology type of primitives to render.
    pub mode: Option<Mode>,

    // TODO: pub targets
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<serde_json::Value>,
}

impl Primitive {
    pub fn mode(&self) -> Mode {
        self.mode.unwrap_or(Mode::Triangles)
    }
}
