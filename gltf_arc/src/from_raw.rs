use super::*;
use anyhow::{Context, Result, anyhow};
use bytes::{BufMut, BytesMut};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

// From raw glTF document to our "owned" model (sans indices)
impl TryFrom<&gltf_types::Document> for Document {
    type Error = anyhow::Error;
    fn try_from(raw: &gltf_types::Document) -> Result<Self, Self::Error> {
        Converter::new(raw).document()
    }
}

// Internal helper:
// - Shared objects are turned into Arc and owned
// - Options with defaults are made explicit so they hash alike
struct Converter<'a> {
    raw: &'a gltf_types::Document,
    node_cache: RefCell<HashMap<gltf_types::NodeIndex, Arc<Node>>>,
    mesh_cache: RefCell<HashMap<gltf_types::MeshIndex, Arc<Mesh>>>,
    material_cache: RefCell<HashMap<gltf_types::MaterialIndex, Arc<Material>>>,
    texture_cache: RefCell<HashMap<gltf_types::TextureIndex, Arc<Texture>>>,
    image_cache: RefCell<HashMap<gltf_types::ImageIndex, Arc<Image>>>,
    sampler_cache: RefCell<HashMap<gltf_types::SamplerIndex, Arc<Sampler>>>,
    // A default sampler to avoid re-creating a new Arc'd default each time a default is used
    default_sampler: Arc<Sampler>,
}

impl<'a> Converter<'a> {
    fn new(raw: &'a gltf_types::Document) -> Self {
        Self {
            raw,
            node_cache: RefCell::default(),
            mesh_cache: RefCell::default(),
            material_cache: RefCell::default(),
            texture_cache: RefCell::default(),
            image_cache: RefCell::default(),
            sampler_cache: RefCell::default(),
            default_sampler: Arc::new(Sampler {
                mag_filter: None,
                min_filter: None,
                wrap_s: gltf_types::WrapMode::Repeat,
                wrap_t: gltf_types::WrapMode::Repeat,
                name: None,
            }),
        }
    }

    // Boilerplate reduction helper, used one per cached reuse-type in
    // Self::*_cache. Returns the item from the cache, or calls the build fn
    // passed in, updates cache, and returns result.
    fn cached<K, V, F>(cache: &RefCell<HashMap<K, Arc<V>>>, key: K, build: F) -> Result<Arc<V>>
    where
        K: std::hash::Hash + Eq + Copy,
        F: FnOnce() -> Result<V>,
    {
        if let Some(cached) = cache.borrow().get(&key) {
            return Ok(Arc::clone(cached));
        }
        let value = Arc::new(build()?);
        cache.borrow_mut().insert(key, Arc::clone(&value));
        Ok(value)
    }

    fn node(&self, idx: gltf_types::NodeIndex) -> Result<Arc<Node>> {
        Self::cached(&self.node_cache, idx, || {
            let raw = self
                .raw
                .nodes
                .get(idx.0 as usize)
                .with_context(|| format!("Invalid node index: {idx:?}"))?;
            self.build_node(raw)
        })
    }

    fn mesh(&self, idx: gltf_types::MeshIndex) -> Result<Arc<Mesh>> {
        Self::cached(&self.mesh_cache, idx, || {
            let raw = self
                .raw
                .meshes
                .get(idx.0 as usize)
                .with_context(|| format!("Invalid mesh index: {idx:?}"))?;
            self.build_mesh(raw)
        })
    }

    fn material(&self, idx: gltf_types::MaterialIndex) -> Result<Arc<Material>> {
        Self::cached(&self.material_cache, idx, || {
            let raw = self
                .raw
                .materials
                .get(idx.0 as usize)
                .with_context(|| format!("Invalid material index: {idx:?}"))?;
            self.build_material(raw)
        })
    }

    fn texture(&self, idx: gltf_types::TextureIndex) -> Result<Arc<Texture>> {
        Self::cached(&self.texture_cache, idx, || {
            let raw = self
                .raw
                .textures
                .get(idx.0 as usize)
                .with_context(|| format!("Invalid texture index: {idx:?}"))?;
            self.build_texture(raw)
        })
    }

    fn sampler(&self, idx: gltf_types::SamplerIndex) -> Result<Arc<Sampler>> {
        Self::cached(&self.sampler_cache, idx, || {
            let raw = self
                .raw
                .samplers
                .get(idx.0 as usize)
                .with_context(|| format!("Invalid sampler index: {idx:?}"))?;
            self.build_sampler(raw)
        })
    }

    fn image(&self, idx: gltf_types::ImageIndex) -> Result<Arc<Image>> {
        Self::cached(&self.image_cache, idx, || {
            let raw = self
                .raw
                .images
                .get(idx.0 as usize)
                .with_context(|| format!("Invalid image index: {idx:?}"))?;
            self.build_image(raw)
        })
    }

    fn build_image(&self, raw: &gltf_types::Image) -> Result<Image> {
        let source = if let Some(uri) = &raw.uri {
            Ok(ImageSource::Uri(uri.clone(), raw.mime_type.clone()))
        } else if let Some(raw_bv_idx) = raw.buffer_view {
            let raw_bv = self
                .raw
                .buffer_views
                .get(raw_bv_idx.0 as usize)
                .with_context(|| format!("Invalid buffer view index: {raw_bv_idx:?}"))?;

            let raw_buffer = self
                .raw
                .buffers
                .get(raw_bv.buffer.0 as usize)
                .with_context(|| format!("Invalid buffer index: {:?}", raw_bv.buffer.0))?;

            let start = raw_bv.byte_offset() as usize;
            let end = start + (raw_bv.byte_length as usize);
            let bytes = raw_buffer.data.slice(start..end);

            let mime_type = raw
                .mime_type
                .clone()
                .context("Images from buffer must have mime_type")?;

            Ok(ImageSource::Data(bytes, mime_type))
        } else {
            Err(anyhow!("No image source could be found"))
        }?;

        Ok(Image {
            name: raw.name.clone(),
            source,
        })
    }

    fn build_sampler(&self, raw: &gltf_types::Sampler) -> Result<Sampler> {
        Ok(Sampler {
            name: raw.name.clone(),
            mag_filter: raw.mag_filter,
            min_filter: raw.min_filter,
            wrap_s: raw.wrap_s(),
            wrap_t: raw.wrap_t(),
        })
    }

    fn build_texture(&self, raw: &gltf_types::Texture) -> Result<Texture> {
        let sampler = match raw.sampler {
            Some(idx) => self.sampler(idx)?,
            None => Arc::clone(&self.default_sampler),
        };

        let source = match raw.source {
            Some(idx) => Some(self.image(idx)?),
            None => None,
        };

        Ok(Texture {
            sampler,
            source,
            name: raw.name.clone(),
        })
    }

    fn texture_info(&self, raw: &gltf_types::TextureInfo) -> Result<TextureInfo> {
        Ok(TextureInfo {
            texture: self.texture(raw.index)?,
            tex_coord: raw.tex_coord(),
        })
    }

    fn normal_texture_info(
        &self,
        raw: &gltf_types::NormalTextureInfo,
    ) -> Result<NormalTextureInfo> {
        Ok(NormalTextureInfo {
            scale: raw.scale().into(),
            texture: self.texture(raw.index)?,
            tex_coord: raw.tex_coord(),
        })
    }

    fn occlusion_texture_info(
        &self,
        raw: &gltf_types::OcclusionTextureInfo,
    ) -> Result<OcclusionTextureInfo> {
        Ok(OcclusionTextureInfo {
            strength: raw.strength().into(),
            texture: self.texture(raw.index)?,
            tex_coord: raw.tex_coord(),
        })
    }

    fn pbr_metallic_roughness(
        &self,
        raw: &gltf_types::PbrMetallicRoughness,
    ) -> Result<PbrMetallicRoughness> {
        Ok(PbrMetallicRoughness {
            base_color_factor: raw.base_color_factor().map(|f| f.into()),
            base_color_texture: raw
                .base_color_texture
                .as_ref()
                .map(|info| self.texture_info(info))
                .transpose()?,
            metallic_factor: raw.metallic_factor().into(),
            metallic_roughness_texture: raw
                .metallic_roughness_texture
                .as_ref()
                .map(|info| self.texture_info(info))
                .transpose()?,
            roughness_factor: raw.roughness_factor().into(),
        })
    }

    fn build_material(&self, raw: &gltf_types::Material) -> Result<Material> {
        Ok(Material {
            name: None,
            alpha_cutoff: raw.alpha_cutoff().into(),
            alpha_mode: raw.alpha_mode(),
            double_sided: raw.double_sided,
            emissive_factor: raw.emissive_factor().map(|f| f.into()),
            emissive_texture: raw
                .emissive_texture
                .as_ref()
                .map(|info| self.texture_info(info))
                .transpose()?,
            normal_texture: raw
                .normal_texture
                .as_ref()
                .map(|info| self.normal_texture_info(info))
                .transpose()?,
            occlusion_texture: raw
                .occlusion_texture
                .as_ref()
                .map(|info| self.occlusion_texture_info(info))
                .transpose()?,
            pbr_metallic_roughness: self.pbr_metallic_roughness(&raw.pbr_metallic_roughness())?,
        })
    }

    fn attribute(&self, idx: gltf_types::AccessorIndex) -> Result<Attribute> {
        let raw_accessor = self
            .raw
            .accessors
            .get(idx.0 as usize)
            .with_context(|| format!("Invalid accessor index: {idx:?}"))?;

        let raw_bv_idx = raw_accessor
            .buffer_view
            .context("Cannot extract attribute with no buffer view")?;

        let raw_bv = self
            .raw
            .buffer_views
            .get(raw_bv_idx.0 as usize)
            .with_context(|| format!("Invalid buffer view index: {raw_bv_idx:?}"))?;

        let raw_buffer = self
            .raw
            .buffers
            .get(raw_bv.buffer.0 as usize)
            .with_context(|| format!("Invalid buffer index: {:?}", raw_bv.buffer.0))?;

        if raw_buffer.data.is_empty() && raw_buffer.uri.is_some() {
            return Err(anyhow!(
                "Accessor references a buffer via uri, which must be inlined first"
            ));
        }

        let src_byte_offset = raw_accessor.byte_offset() + raw_bv.byte_offset();
        let src_byte_stride = raw_bv.byte_stride.unwrap_or(0) as usize;
        let count = raw_accessor.count;
        let component_count = raw_accessor
            .r#type
            .component_count()
            .context("Can't extract component count for unknown type")?
            as u32;

        let component_byte_size = raw_accessor
            .component_type
            .byte_size()
            .context("Can't extract byte size for unknown component type")?
            as u32;

        let byte_length = (count * component_count * component_byte_size) as usize;

        let data = if src_byte_stride == 0 {
            // With no stride, copy it in one go
            let start = src_byte_offset as usize;
            let end = start + byte_length;
            raw_buffer.data.slice(start..end)
        } else {
            // Extract piece by piece from interleaved data
            let mut data = BytesMut::with_capacity(byte_length);
            let mut start = src_byte_offset as usize;
            for _ in 0..count {
                let end = start + (component_count * component_byte_size) as usize;
                let chunk = &raw_buffer.data[start..end];
                data.put_slice(chunk);
                start += src_byte_stride;
            }
            data.freeze()
        };

        Ok(Attribute {
            component_type: raw_accessor.component_type,
            normalized: raw_accessor.normalized,
            r#type: raw_accessor.r#type.clone(),
            data,
            count,
        })
    }

    fn build_primitive(&self, raw: &gltf_types::Primitive) -> Result<Primitive> {
        let attributes = raw
            .attributes
            .iter()
            .map(|(sem, idx)| {
                Ok((
                    sem.clone(),
                    self.attribute(*idx)
                        .with_context(|| format!("Could not convert attribute {sem}"))?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let indices = raw
            .indices
            .map(|idx| self.attribute(idx))
            .transpose()
            .context("Could not convert indices")?;
        let material = raw
            .material
            .map(|idx| self.material(idx))
            .transpose()
            .context("Could not convert material")?;

        Ok(Primitive {
            attributes,
            indices,
            material,
            mode: raw.mode(),
        })
    }

    fn build_mesh(&self, raw: &gltf_types::Mesh) -> Result<Mesh> {
        let primitives = raw
            .primitives
            .iter()
            .map(|p| self.build_primitive(p))
            .collect::<Result<Vec<_>>>()
            .context("Could not convert primitive")?;
        Ok(Mesh {
            name: raw.name.clone(),
            primitives,
        })
    }

    fn build_node(&self, raw: &gltf_types::Node) -> Result<Node> {
        let children = raw
            .children
            .iter()
            .map(|c| self.node(*c))
            .collect::<Result<Vec<_>>>()?;
        let mesh = raw
            .mesh
            .map(|m| self.mesh(m))
            .transpose()
            .context("Could not convert mesh")?;

        let transform = if let Some(matrix_vals) = raw.matrix {
            let mat = glam::Mat4::from_cols_array(&matrix_vals);
            Transform::Matrix(mat)
        } else {
            let rotation =
                glam::Quat::from_array(raw.rotation.unwrap_or(gltf_types::default_rotation()));
            let scale = glam::Vec3::from_array(raw.scale.unwrap_or(gltf_types::default_scale()));
            let translation = glam::Vec3::from_array(
                raw.translation.unwrap_or(gltf_types::default_translation()),
            );
            Transform::ComponentizedTransform {
                rotation,
                scale,
                translation,
            }
        };

        Ok(Node {
            name: raw.name.clone(),
            transform,
            children,
            mesh,
        })
    }

    fn build_scene(&self, raw: &gltf_types::Scene) -> Result<Scene> {
        let nodes = raw
            .nodes
            .iter()
            .map(|n| self.node(*n))
            .collect::<Result<Vec<_>>>()
            .context("Could not convert node")?;
        Ok(Scene {
            name: raw.name.clone(),
            nodes,
        })
    }

    fn document(&self) -> Result<Document> {
        let mut default_scene = None;
        let mut other_scenes = vec![];

        for (scene_idx, raw_scene) in self.raw.scenes.iter().enumerate() {
            let scene = self
                .build_scene(raw_scene)
                .context("Could not convert scene")?;
            let is_default = self.raw.scene.is_some_and(|s| s.0 as usize == scene_idx);
            if is_default {
                default_scene = Some(scene);
            } else {
                other_scenes.push(scene);
            }
        }

        Ok(Document {
            default_scene,
            other_scenes,
        })
    }
}
