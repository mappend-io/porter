use bytes::Bytes;
use gltf_types::MimeType;
use ordered_float::OrderedFloat;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Document {
    pub default_scene: Option<Scene>,
    pub other_scenes: Vec<Scene>,
    // TODO: extras, extensions? or flatten?
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub name: Option<String>,
    pub nodes: Vec<Arc<Node>>,
}

#[derive(Clone, Debug)]
pub struct Node {
    //pub camera
    pub children: Vec<Arc<Node>>,
    //pub skin
    pub transform: Transform,
    pub mesh: Option<Arc<Mesh>>,
    //pub weights
    pub name: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Transform {
    Matrix(glam::Mat4),
    ComponentizedTransform {
        rotation: glam::Quat,
        scale: glam::Vec3,
        translation: glam::Vec3,
    },
}

impl Transform {
    pub fn to_mat4(&self) -> glam::Mat4 {
        match self {
            Transform::Matrix(matrix) => *matrix,
            Transform::ComponentizedTransform {
                scale,
                rotation,
                translation,
            } => glam::Mat4::from_scale_rotation_translation(*scale, *rotation, *translation),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ComponentizedTransform {
    pub rotation: glam::Quat,
    pub scale: glam::Vec3,
    pub translation: glam::Vec3,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub primitives: Vec<Primitive>,
    pub name: Option<String>,
}

// NOTE: Attributes are impossible to interleave here, in this form they are
// always packed tightly.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Primitive {
    pub attributes: BTreeMap<gltf_types::Semantic, Attribute>,
    pub indices: Option<Attribute>,
    pub material: Option<Arc<Material>>,
    pub mode: gltf_types::Mode,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Attribute {
    pub data: Bytes,
    pub component_type: gltf_types::ComponentType,
    pub r#type: gltf_types::Type,
    pub normalized: bool,
    pub count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Material {
    pub name: Option<String>,
    // NOTE: No option, we want it to hash the same if present or not.
    // We'll rely on the conversion back to gltf_types to detect if it's
    // the default and omit it.
    pub pbr_metallic_roughness: PbrMetallicRoughness,
    pub normal_texture: Option<NormalTextureInfo>,
    pub occlusion_texture: Option<OcclusionTextureInfo>,
    pub emissive_texture: Option<TextureInfo>,
    pub emissive_factor: [OrderedFloat<f32>; 3],
    pub alpha_mode: gltf_types::AlphaMode,
    pub alpha_cutoff: OrderedFloat<f32>,
    pub double_sided: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PbrMetallicRoughness {
    pub base_color_factor: [OrderedFloat<f32>; 4],
    pub base_color_texture: Option<TextureInfo>,
    pub metallic_factor: OrderedFloat<f32>,
    pub roughness_factor: OrderedFloat<f32>,
    pub metallic_roughness_texture: Option<TextureInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextureInfo {
    pub texture: Arc<Texture>,
    pub tex_coord: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct NormalTextureInfo {
    pub texture: Arc<Texture>,
    pub tex_coord: u32,
    pub scale: OrderedFloat<f32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OcclusionTextureInfo {
    pub texture: Arc<Texture>,
    pub tex_coord: u32,
    pub strength: OrderedFloat<f32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Image {
    pub source: ImageSource,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ImageSource {
    Uri(String, Option<MimeType>),
    Data(Bytes, MimeType),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Sampler {
    pub mag_filter: Option<gltf_types::MagFilter>,
    pub min_filter: Option<gltf_types::MinFilter>,
    pub wrap_s: gltf_types::WrapMode,
    pub wrap_t: gltf_types::WrapMode,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Texture {
    // Since a default is used, we should probably
    // make one when we translate from raw gltf,
    // so when we hash it, no sampleris the same as a default one.
    pub sampler: Arc<Sampler>,
    pub source: Option<Arc<Image>>,
    pub name: Option<String>,
}
