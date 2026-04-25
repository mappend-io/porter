use super::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::Validate;

// https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#reference-node
#[skip_serializing_none]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct Node {
    pub camera: Option<CameraIndex>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(custom(function = "utils::validate_unique_ids"))]
    pub children: Vec<NodeIndex>,
    pub skin: Option<SkinIndex>,
    pub matrix: Option<[f32; 16]>,
    pub mesh: Option<MeshIndex>,
    pub rotation: Option<[f32; 4]>,
    pub scale: Option<[f32; 3]>,
    pub translation: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weights: Vec<f32>,
    pub name: Option<String>,
    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl Node {
    pub fn matrix(&self) -> [f32; 16] {
        self.matrix.unwrap_or([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ])
    }

    // TODO: rotation, scale, translation

    // TODO: helper that looks at matrix, TRS, picks right and accumulates them
}

// TODO: validation:
// not both matrix and TRS
