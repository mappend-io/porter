use crate::*;
use anyhow::{Result, bail};
use bytes::{Bytes, BytesMut};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

impl Document {
    pub fn to_gltf_types(&self) -> Result<(gltf_types::Document, Bytes)> {
        Converter::convert(self)
    }
}

struct Converter {
    raw_doc: gltf_types::Document,
    buffer: BytesMut,
    _meshes: HashMap<*const Mesh, gltf_types::MeshIndex>,
    materials: HashMap<*const Material, gltf_types::MaterialIndex>,
    images: HashMap<*const Image, gltf_types::ImageIndex>,
    textures: HashMap<*const Texture, gltf_types::TextureIndex>,
    samplers: HashMap<*const Sampler, gltf_types::SamplerIndex>,
}

impl Converter {
    pub fn convert(doc: &Document) -> Result<(gltf_types::Document, Bytes)> {
        let mut conv = Self {
            raw_doc: gltf_types::Document::default(),
            buffer: BytesMut::new(),
            _meshes: HashMap::new(),
            materials: HashMap::new(),
            images: HashMap::new(),
            textures: HashMap::new(),
            samplers: HashMap::new(),
        };

        // Write default scene first, then write other scenes
        if let Some(scene) = &doc.default_scene {
            conv.raw_doc.scene = Some(conv.scene(scene)?);
        }

        // TODO: Other scenes

        conv.raw_doc.buffers.push(gltf_types::Buffer {
            data: Bytes::new(),
            uri: None,
            byte_length: conv.buffer.len() as u32,
            name: None,
            property: gltf_types::Property::default(),
        });

        Ok((conv.raw_doc, conv.buffer.freeze()))
    }

    fn scene(&mut self, scene: &Scene) -> Result<gltf_types::SceneIndex> {
        let mut nodes = vec![];
        for node in &scene.nodes {
            nodes.push(self.node(node)?);
        }

        let raw_scene = gltf_types::Scene {
            name: scene.name.clone(),
            nodes,
            property: gltf_types::Property::default(),
        };

        self.raw_doc.scenes.push(raw_scene);

        Ok(gltf_types::SceneIndex(
            (self.raw_doc.scenes.len() - 1) as u32,
        ))
    }

    fn node(&mut self, node: &Node) -> Result<gltf_types::NodeIndex> {
        let mut children = vec![];
        for child in &node.children {
            children.push(self.node(child)?);
        }

        let (matrix, rotation, scale, translation): (
            Option<glam::Mat4>,
            Option<glam::Quat>,
            Option<glam::Vec3>,
            Option<glam::Vec3>,
        ) = match node.transform {
            Transform::Matrix(matrix) => (Some(matrix), None, None, None),
            Transform::ComponentizedTransform {
                scale,
                rotation,
                translation,
            } => (None, Some(rotation), Some(scale), Some(translation)),
        };

        let raw_node = gltf_types::Node {
            name: node.name.clone(),
            mesh: node.mesh.as_ref().map(|mesh| self.mesh(mesh)).transpose()?,
            camera: None,
            matrix: matrix.map(|m| m.to_cols_array()),
            rotation: rotation.map(|r| r.to_array()),
            scale: scale.map(|s| s.to_array()),
            translation: translation.map(|t| t.to_array()),
            skin: None,
            weights: vec![],
            children,
            property: gltf_types::Property::default(),
        };

        self.raw_doc.nodes.push(raw_node);

        Ok(gltf_types::NodeIndex((self.raw_doc.nodes.len() - 1) as u32))
    }

    fn mesh(&mut self, mesh: &Mesh) -> Result<gltf_types::MeshIndex> {
        let primitives = mesh
            .primitives
            .iter()
            .map(|p| self.primitive(p))
            .collect::<Result<Vec<_>>>()?;

        if primitives.is_empty() {
            bail!("Mesh must have at least one primitive");
        }

        let raw_mesh = gltf_types::Mesh {
            name: mesh.name.clone(),
            primitives,
            property: gltf_types::Property::default(),
        };

        self.raw_doc.meshes.push(raw_mesh);

        Ok(gltf_types::MeshIndex(
            (self.raw_doc.meshes.len() - 1) as u32,
        ))
    }

    fn accessor(
        &mut self,
        attribute: &Attribute,
        target: gltf_types::Target,
        semantic: Option<&gltf_types::Semantic>,
    ) -> Result<gltf_types::AccessorIndex> {
        let aligned_start = align_length_to_multiple_of(
            self.buffer.len(),
            attribute.component_type.byte_size().unwrap_or(0) as usize,
        );

        // Grow the buffer to the aligned size, padded with zeros
        self.buffer.resize(aligned_start, 0);

        // Capture the attribute's data
        self.buffer.extend_from_slice(&attribute.data);

        let buffer_view = gltf_types::BufferView {
            buffer: gltf_types::BufferIndex(0),
            byte_length: attribute.data.len() as u32,
            byte_offset: Some(aligned_start as u32),
            byte_stride: None, // We pack tightly, no interleaving
            target: Some(target),
            name: None,
            property: gltf_types::Property::default(),
        };

        self.raw_doc.buffer_views.push(buffer_view);

        let buffer_view_index =
            gltf_types::BufferViewIndex(self.raw_doc.buffer_views.len() as u32 - 1);

        // Compute min/max for POSITION (required by spec).
        let (min, max) = if matches!(semantic, Some(gltf_types::Semantic::Position)) {
            let positions: &[[f32; 3]] = bytemuck::cast_slice(&attribute.data);
            if positions.is_empty() {
                bail!("Position accessor has no vertices");
            }
            let mut min = positions[0];
            let mut max = positions[0];
            for &p in &positions[1..] {
                for i in 0..3 {
                    if p[i] < min[i] {
                        min[i] = p[i];
                    }
                    if p[i] > max[i] {
                        max[i] = p[i];
                    }
                }
            }
            (Some(min.to_vec()), Some(max.to_vec()))
        } else {
            (None, None)
        };

        let accessor = gltf_types::Accessor {
            buffer_view: Some(buffer_view_index),
            byte_offset: None, // It's a 1:1 to the BV, no offset
            component_type: attribute.component_type,
            count: attribute.count,
            r#type: attribute.r#type.clone(),
            normalized: attribute.normalized,
            min,
            max,
            name: None,
            property: gltf_types::Property::default(),
        };

        self.raw_doc.accessors.push(accessor);

        Ok(gltf_types::AccessorIndex(
            self.raw_doc.accessors.len() as u32 - 1,
        ))
    }

    fn primitive(&mut self, primitive: &Primitive) -> Result<gltf_types::Primitive> {
        let mut attributes = BTreeMap::new();
        for (semantic, src_attrib) in &primitive.attributes {
            attributes.insert(
                semantic.clone(),
                self.accessor(src_attrib, gltf_types::Target::ArrayBuffer, Some(semantic))?,
            );
        }

        let indices = primitive
            .indices
            .as_ref()
            .map(|a| self.accessor(a, gltf_types::Target::ElementArrayBuffer, None))
            .transpose()?;

        Ok(gltf_types::Primitive {
            mode: Some(primitive.mode), // TODO: Don't set if it's == default
            attributes,
            indices,
            material: primitive
                .material
                .as_ref()
                .map(|m| self.material(m))
                .transpose()?,
            ..Default::default()
        })
    }

    fn material(&mut self, material: &Arc<Material>) -> Result<gltf_types::MaterialIndex> {
        if let Some(cached) = self.materials.get(&Arc::as_ptr(material)) {
            return Ok(*cached);
        }

        let material_index = gltf_types::MaterialIndex(self.raw_doc.materials.len() as u32);
        let mut raw_material = gltf_types::Material {
            // TODO: skip if default
            pbr_metallic_roughness: Some(
                self.pbr_metallic_roughness(&material.pbr_metallic_roughness)?,
            ),
            normal_texture: material
                .normal_texture
                .as_ref()
                .map(|t| self.normal_texture_info(t))
                .transpose()?,
            occlusion_texture: material
                .occlusion_texture
                .as_ref()
                .map(|t| self.occlusion_texture_info(t))
                .transpose()?,
            emissive_texture: material
                .emissive_texture
                .as_ref()
                .map(|t| self.texture_info(t))
                .transpose()?,
            emissive_factor: Some(material.emissive_factor.map(f32::from)), // TODO: skip if default
            alpha_mode: Some(material.alpha_mode.clone()),                  // skip default
            alpha_cutoff: Some(material.alpha_cutoff.into()),               // TODO: skip if default
            double_sided: material.double_sided,
            name: material.name.clone(),
            property: gltf_types::Property::default(),
        };

        if material.alpha_mode == gltf_types::AlphaMode::Mask {
            raw_material.alpha_cutoff = Some(material.alpha_cutoff.into()); // TODO: skip if default
        }

        self.raw_doc.materials.push(raw_material);

        self.materials.insert(Arc::as_ptr(material), material_index);
        Ok(material_index)
    }

    fn pbr_metallic_roughness(
        &mut self,
        pbr: &PbrMetallicRoughness,
    ) -> Result<gltf_types::PbrMetallicRoughness> {
        Ok(gltf_types::PbrMetallicRoughness {
            base_color_factor: Some(pbr.base_color_factor.map(f32::from)), // TODO: skip if default
            base_color_texture: pbr
                .base_color_texture
                .as_ref()
                .map(|t| self.texture_info(t))
                .transpose()?,
            metallic_factor: Some(pbr.metallic_factor.into()), // TODO: skip if default
            roughness_factor: Some(pbr.roughness_factor.into()), // TODO: skip if default
            metallic_roughness_texture: pbr
                .metallic_roughness_texture
                .as_ref()
                .map(|t| self.texture_info(t))
                .transpose()?,
            property: gltf_types::Property::default(),
        })
    }

    fn occlusion_texture_info(
        &mut self,
        texture_info: &OcclusionTextureInfo,
    ) -> Result<gltf_types::OcclusionTextureInfo> {
        Ok(gltf_types::OcclusionTextureInfo {
            index: self.texture(&texture_info.texture)?,
            strength: Some(texture_info.strength.into()), // TODO: skip if default
            tex_coord: Some(texture_info.tex_coord),      // TODO: skip if default
            property: gltf_types::Property::default(),
        })
    }

    fn normal_texture_info(
        &mut self,
        texture_info: &NormalTextureInfo,
    ) -> Result<gltf_types::NormalTextureInfo> {
        Ok(gltf_types::NormalTextureInfo {
            index: self.texture(&texture_info.texture)?,
            scale: Some(texture_info.scale.into()), // TODO: skip if default
            tex_coord: Some(texture_info.tex_coord), // TODO: skip if default
            property: gltf_types::Property::default(),
        })
    }

    fn texture_info(&mut self, texture_info: &TextureInfo) -> Result<gltf_types::TextureInfo> {
        Ok(gltf_types::TextureInfo {
            index: self.texture(&texture_info.texture)?,
            tex_coord: Some(texture_info.tex_coord), // TODO: skip if default
            property: gltf_types::Property::default(),
        })
    }

    fn texture(&mut self, texture: &Arc<Texture>) -> Result<gltf_types::TextureIndex> {
        if let Some(cached) = self.textures.get(&Arc::as_ptr(texture)) {
            return Ok(*cached);
        }

        let texture_index = gltf_types::TextureIndex(self.raw_doc.textures.len() as u32);
        let raw_texture = gltf_types::Texture {
            name: texture.name.clone(),
            property: gltf_types::Property::default(),
            sampler: Some(self.sampler(&texture.sampler)?), // TODO: skip if default
            source: texture.source.as_ref().map(|s| self.image(s)).transpose()?,
        };
        self.raw_doc.textures.push(raw_texture);

        self.textures.insert(Arc::as_ptr(texture), texture_index);
        Ok(texture_index)
    }

    fn image(&mut self, image: &Arc<Image>) -> Result<gltf_types::ImageIndex> {
        if let Some(cached) = self.images.get(&Arc::as_ptr(image)) {
            return Ok(*cached);
        }

        let buffer_view: Option<gltf_types::BufferViewIndex>;
        let uri: Option<String>;
        let mime_type: Option<gltf_types::MimeType>;
        match &image.source {
            ImageSource::Data(bytes, this_mime_type) => {
                uri = None;
                mime_type = Some(this_mime_type.clone());

                let start = self.buffer.len() as u32;
                self.buffer.extend_from_slice(bytes);

                let new_buffer_view = gltf_types::BufferView {
                    buffer: gltf_types::BufferIndex(0),
                    byte_length: bytes.len() as u32,
                    byte_offset: Some(start),
                    byte_stride: None,
                    target: None,
                    name: None,
                    property: gltf_types::Property::default(),
                };

                self.raw_doc.buffer_views.push(new_buffer_view);

                buffer_view = Some(gltf_types::BufferViewIndex(
                    self.raw_doc.buffer_views.len() as u32 - 1,
                ));
            }
            ImageSource::Uri(this_uri, this_mime_type) => {
                uri = Some(this_uri.clone());
                mime_type = this_mime_type.clone();
                buffer_view = None;
            }
        };

        let image_index = gltf_types::ImageIndex(self.raw_doc.images.len() as u32);
        let raw_image = gltf_types::Image {
            name: image.name.clone(),
            property: gltf_types::Property::default(),
            buffer_view,
            mime_type,
            uri,
        };
        self.raw_doc.images.push(raw_image);

        self.images.insert(Arc::as_ptr(image), image_index);
        Ok(image_index)
    }

    fn sampler(&mut self, sampler: &Arc<Sampler>) -> Result<gltf_types::SamplerIndex> {
        if let Some(cached) = self.samplers.get(&Arc::as_ptr(sampler)) {
            return Ok(*cached);
        }

        let sampler_index = gltf_types::SamplerIndex(self.raw_doc.samplers.len() as u32);
        let raw_sampler = gltf_types::Sampler {
            mag_filter: sampler.mag_filter,
            min_filter: sampler.min_filter,
            wrap_s: Some(sampler.wrap_s), // TODO: don't set if default
            wrap_t: Some(sampler.wrap_t), // TODO: don't set if default
            name: sampler.name.clone(),
            property: gltf_types::Property::default(),
        };
        self.raw_doc.samplers.push(raw_sampler);

        self.samplers.insert(Arc::as_ptr(sampler), sampler_index);
        Ok(sampler_index)
    }
}

// Increase current_len to be a multiple of alignment
fn align_length_to_multiple_of(current_len: usize, alignment: usize) -> usize {
    current_len.div_ceil(alignment) * alignment
}
