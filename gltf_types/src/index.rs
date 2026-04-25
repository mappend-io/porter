use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct AccessorIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct AnimationIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct BufferIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct BufferViewIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct CameraIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct ImageIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct MaterialIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct MeshIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct NodeIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct SamplerIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct SceneIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct SkinIndex(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct TextureIndex(pub u32);
