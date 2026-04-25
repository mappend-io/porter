use super::*;
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

// https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#reference-gltf
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
#[validate(schema(function = "validate_model"))]
pub struct Document {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extensions_used: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extensions_required: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub accessors: Vec<Accessor>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub animations: Vec<Animation>,

    #[validate(nested)]
    pub asset: Asset,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub buffers: Vec<Buffer>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub buffer_views: Vec<BufferView>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub cameras: Vec<Camera>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub images: Vec<Image>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub materials: Vec<Material>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub meshes: Vec<Mesh>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub nodes: Vec<Node>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub samplers: Vec<Sampler>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene: Option<SceneIndex>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub scenes: Vec<Scene>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[validate(nested)]
    pub textures: Vec<Texture>,

    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

pub fn validate_model(model: &Document) -> Result<(), ValidationError> {
    if let Some(scene) = model.scene
        && scene.0 as usize >= model.scenes.len()
    {
        let mut err = ValidationError::new("INVALID_SCENE_INDEX");
        err.message = Some("model.scene must be a valid index into model.scenes".into());
        return Err(err);
    }

    Ok(())
}
