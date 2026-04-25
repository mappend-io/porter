use crate::*;
use anyhow::{Context, Result, anyhow};
use bytes::BytesMut;
use std::collections::HashMap;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AttributeKey {
    semantic: gltf_types::Semantic,
    component_type: gltf_types::ComponentType,
    r#type: gltf_types::Type,
    normalized: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PrimitiveKey {
    pub material: Option<Arc<Material>>,
    pub mode: gltf_types::Mode,
    pub attributes: BTreeSet<AttributeKey>,
    // NOTE: We always promote to u32 internally, and might rewrite to u16 on the way to a model
    pub indexed: bool,
}

pub struct PrimitiveBuilder {
    pub attribute_buffers: HashMap<gltf_types::Semantic, BytesMut>,
    pub index_buffer: BytesMut,
    pub vertex_count: u32,
    pub index_count: u32,
}

impl PrimitiveBuilder {
    pub fn new(key: PrimitiveKey) -> Self {
        let attribute_buffers: HashMap<gltf_types::Semantic, BytesMut> = key
            .attributes
            .iter()
            .map(|key| (key.semantic.clone(), BytesMut::new()))
            .collect();

        Self {
            attribute_buffers,
            index_buffer: BytesMut::new(),
            vertex_count: 0,
            index_count: 0,
        }
    }

    pub fn attributes_key(
        attrs: &BTreeMap<gltf_types::Semantic, Attribute>,
    ) -> BTreeSet<AttributeKey> {
        let mut ret = BTreeSet::new();
        for (semantic, attr) in attrs.iter() {
            let key = AttributeKey {
                semantic: semantic.clone(),
                component_type: attr.component_type,
                r#type: attr.r#type.clone(),
                normalized: attr.normalized,
            };
            ret.insert(key);
        }
        ret
    }

    pub fn append(
        &mut self,
        prim: &Primitive,
        mesh_to_world: glam::DMat4,
        world_to_local: glam::DMat4,
    ) -> Result<()> {
        let mesh_to_local = world_to_local * mesh_to_world;
        let normal_matrix = glam::DMat3::from_mat4(mesh_to_local).inverse().transpose();
        let tangent_matrix = glam::DMat3::from_mat4(mesh_to_local);

        let vertex_count = prim
            .attributes
            .first_key_value()
            .context("Primitive has no attributes")?
            .1
            .count;

        for (semantic, dst_bytes) in &mut self.attribute_buffers {
            let src_attr = prim
                .attributes
                .get(semantic)
                .with_context(|| format!("Source primitive missing attribute: {semantic}"))?;

            if src_attr.count != vertex_count {
                return Err(anyhow!(
                    "Inconsistent vertex count in primitive: {} vs {}",
                    src_attr.count,
                    vertex_count
                ));
            }

            match semantic {
                gltf_types::Semantic::Position => {
                    append_transformed_positions(dst_bytes, src_attr, mesh_to_local)?;
                }
                gltf_types::Semantic::Normal => {
                    append_transformed_normals(dst_bytes, src_attr, normal_matrix)?;
                }
                gltf_types::Semantic::Tangent => {
                    append_transformed_tangents(dst_bytes, src_attr, tangent_matrix)?;
                }
                _ => {
                    dst_bytes.extend_from_slice(&src_attr.data);
                }
            }
        }

        if let Some(src_indices) = &prim.indices {
            append_indices(&mut self.index_buffer, src_indices, self.vertex_count)?;
            self.index_count += src_indices.count;
        }

        // Don't be tempted to move this, we use the original value above when appending indices
        self.vertex_count += vertex_count;

        Ok(())
    }

    pub fn to_primitive(&self, key: &PrimitiveKey) -> Primitive {
        let mut attributes = BTreeMap::new();
        for attr_key in &key.attributes {
            let buffer = self
                .attribute_buffers
                .get(&attr_key.semantic)
                .expect("Buffer must exist for key semantic");
            let attribute = Attribute {
                component_type: attr_key.component_type,
                count: self.vertex_count,
                data: buffer.clone().freeze(), // TODO: consume it, don't clone it
                normalized: attr_key.normalized,
                r#type: attr_key.r#type.clone(),
            };
            attributes.insert(attr_key.semantic.clone(), attribute);
        }

        let indices = if key.indexed {
            Some(Attribute {
                component_type: gltf_types::ComponentType::UnsignedInt,
                count: self.index_count,
                data: self.index_buffer.clone().freeze(), // TODO: consume it, don't clone it
                normalized: false,
                r#type: gltf_types::Type::Scalar,
            })
        } else {
            None
        };

        Primitive {
            attributes,
            indices,
            material: key.material.clone(),
            mode: key.mode,
        }
    }
}

fn append_transformed_positions(
    dst: &mut BytesMut,
    src: &Attribute,
    transform: glam::DMat4,
) -> Result<()> {
    // Positions in glTF are required to be VEC3 of FLOAT.
    if src.component_type != gltf_types::ComponentType::Float
        || src.r#type != gltf_types::Type::Vec3
    {
        return Err(anyhow!(
            "POSITION must be VEC3/FLOAT, got {:?}/{:?}",
            src.r#type,
            src.component_type
        ));
    }

    let positions: &[[f32; 3]] = bytemuck::cast_slice(&src.data);
    dst.reserve(std::mem::size_of_val(positions));

    for &pos in positions {
        let p = glam::DVec3::new(pos[0] as f64, pos[1] as f64, pos[2] as f64);
        let transformed = transform.transform_point3(p);
        let out = [
            transformed.x as f32,
            transformed.y as f32,
            transformed.z as f32,
        ];
        dst.extend_from_slice(bytemuck::bytes_of(&out));
    }

    Ok(())
}

fn append_transformed_normals(
    dst: &mut BytesMut,
    src: &Attribute,
    normal_matrix: glam::DMat3,
) -> Result<()> {
    if src.component_type != gltf_types::ComponentType::Float
        || src.r#type != gltf_types::Type::Vec3
    {
        return Err(anyhow!("NORMAL must be VEC3/FLOAT"));
    }

    let normals: &[[f32; 3]] = bytemuck::cast_slice(&src.data);
    dst.reserve(std::mem::size_of_val(normals));

    for &n in normals {
        let v = glam::DVec3::new(n[0] as f64, n[1] as f64, n[2] as f64);
        let transformed = (normal_matrix * v).normalize();
        let out = [
            transformed.x as f32,
            transformed.y as f32,
            transformed.z as f32,
        ];
        dst.extend_from_slice(bytemuck::bytes_of(&out));
    }

    Ok(())
}

fn append_transformed_tangents(
    dst: &mut BytesMut,
    src: &Attribute,
    tangent_matrix: glam::DMat3,
) -> Result<()> {
    // Tangents are VEC4 of FLOAT: xyz is the tangent vector, w is the sign.
    if src.component_type != gltf_types::ComponentType::Float
        || src.r#type != gltf_types::Type::Vec4
    {
        return Err(anyhow!("TANGENT must be VEC4/FLOAT"));
    }

    let tangents: &[[f32; 4]] = bytemuck::cast_slice(&src.data);
    dst.reserve(std::mem::size_of_val(tangents));

    for &t in tangents {
        let v = glam::DVec3::new(t[0] as f64, t[1] as f64, t[2] as f64);
        let transformed = (tangent_matrix * v).normalize();
        let out = [
            transformed.x as f32,
            transformed.y as f32,
            transformed.z as f32,
            t[3], // sign passes through unchanged
        ];
        dst.extend_from_slice(bytemuck::bytes_of(&out));
    }

    Ok(())
}

fn append_indices(dst: &mut BytesMut, src: &Attribute, vertex_offset: u32) -> Result<()> {
    // We use u32 indices, regardless of source type, so promote everything else
    match src.component_type {
        gltf_types::ComponentType::UnsignedByte => {
            for &i in src.data.iter() {
                let promoted = i as u32 + vertex_offset;
                dst.extend_from_slice(&promoted.to_le_bytes());
            }
        }
        gltf_types::ComponentType::UnsignedShort => {
            let indices: &[u16] = bytemuck::cast_slice(&src.data);
            for &i in indices {
                let promoted = i as u32 + vertex_offset;
                dst.extend_from_slice(&promoted.to_le_bytes());
            }
        }
        gltf_types::ComponentType::UnsignedInt => {
            let indices: &[u32] = bytemuck::cast_slice(&src.data);
            for &i in indices {
                let offset = i
                    .checked_add(vertex_offset)
                    .context("Index overflow when offsetting")?;
                dst.extend_from_slice(&offset.to_le_bytes());
            }
        }
        other => return Err(anyhow!("Indices must be unsigned integer, got {:?}", other)),
    }
    Ok(())
}
