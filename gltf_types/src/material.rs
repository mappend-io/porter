use super::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::Validate;

#[skip_serializing_none]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct PbrMetallicRoughness {
    pub base_color_factor: Option<[f32; 4]>,
    pub base_color_texture: Option<TextureInfo>,
    pub metallic_factor: Option<f32>,
    pub roughness_factor: Option<f32>,
    pub metallic_roughness_texture: Option<TextureInfo>,
    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl PbrMetallicRoughness {
    pub fn base_color_factor(&self) -> [f32; 4] {
        self.base_color_factor.unwrap_or([1.0, 1.0, 1.0, 1.0])
    }

    pub fn metallic_factor(&self) -> f32 {
        self.metallic_factor.unwrap_or(1.0)
    }

    pub fn roughness_factor(&self) -> f32 {
        self.roughness_factor.unwrap_or(1.0)
    }
}

#[skip_serializing_none]
#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct NormalTextureInfo {
    pub index: TextureIndex,

    // TODO: Use a skip_serializing_if is default instead,
    // I'd rather remove the option here
    #[serde(default)]
    pub tex_coord: Option<u32>,

    #[serde(default)]
    #[validate(range(min = 0.0, max = 1.0))]
    pub scale: Option<f32>,

    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl NormalTextureInfo {
    pub fn tex_coord(&self) -> u32 {
        self.tex_coord.unwrap_or(0)
    }

    pub fn scale(&self) -> f32 {
        self.scale.unwrap_or(1.0)
    }
}

#[skip_serializing_none]
#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct OcclusionTextureInfo {
    pub index: TextureIndex,

    // TODO: Use a skip_serializing_if is default instead,
    // I'd rather remove the option here
    #[serde(default)]
    pub tex_coord: Option<u32>,

    #[serde(default)]
    #[validate(range(min = 0.0, max = 1.0))]
    pub strength: Option<f32>,

    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl OcclusionTextureInfo {
    pub fn tex_coord(&self) -> u32 {
        self.tex_coord.unwrap_or(0)
    }

    pub fn strength(&self) -> f32 {
        self.strength.unwrap_or(1.0)
    }
}

#[skip_serializing_none]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct Material {
    pub pbr_metallic_roughness: Option<PbrMetallicRoughness>,
    pub normal_texture: Option<NormalTextureInfo>,
    pub occlusion_texture: Option<OcclusionTextureInfo>,
    pub emissive_texture: Option<TextureInfo>,
    pub emissive_factor: Option<[f32; 3]>,
    pub alpha_mode: Option<AlphaMode>,
    pub alpha_cutoff: Option<f32>,
    pub double_sided: bool,
    pub name: Option<String>,
    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl Material {
    pub fn pbr_metallic_roughness(&self) -> PbrMetallicRoughness {
        // TODO: efficiency, what's the right way to do this?
        self.pbr_metallic_roughness.clone().unwrap_or_default()
    }

    pub fn emissive_factor(&self) -> [f32; 3] {
        self.emissive_factor.unwrap_or(default_emissive_factor())
    }

    pub fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode.clone().unwrap_or(default_alpha_mode())
    }

    pub fn alpha_cutoff(&self) -> f32 {
        self.alpha_cutoff.unwrap_or(default_alpha_cutoff())
    }
}
