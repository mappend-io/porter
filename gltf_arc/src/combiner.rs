use crate::primitive_builder::*;
use crate::*;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Combine many models into one. We are careful to cache and reuse Materials
/// and Primitives when possible. When models are added, they come loaded with a
/// placement transform (which will take them from local Y-up glTF models like a
/// building into ECEF, ready for 3D Tiles). We will extract the primitives,
/// mash together primitives that share materials and vertex formats, and
/// accumulate into a BytesMut for each attribute. The positions will go from
/// local to ECEF, then back through the combiner's ECEF to local transform so
/// they are f32 friendly. When the user is done combining, we will produce a
/// new model from the per-Primitive builders, with a simple structure.
///
/// Static models can still have lights, we'll extract those, but not worrying
/// about it for right now.
///
/// Dynamic models are trickier, we aren't worrying about those for now.
pub struct Combiner {
    local_to_world: glam::DMat4,
    world_to_local: glam::DMat4,
    static_prims: HashMap<PrimitiveKey, PrimitiveBuilder>,
    canonical_materials: HashSet<Arc<Material>>,
    // FUTURE: static_lights
    // FUTURE: dynamic_models
}

impl Combiner {
    /// Be careful with the transform here, make sure it round-trips to f32
    pub fn new(local_to_world: glam::Mat4) -> Self {
        Self {
            local_to_world: local_to_world.as_dmat4(),
            world_to_local: local_to_world.as_dmat4().inverse(),
            static_prims: HashMap::new(),
            canonical_materials: HashSet::new(),
        }
    }

    fn add_static_mesh_prim(&mut self, prim: &Primitive, mesh_to_world: glam::DMat4) -> Result<()> {
        let canonical_mat = prim
            .material
            .as_ref()
            .map(|m| self.canonicalize_material(m.clone()));

        let prim_key = PrimitiveKey {
            attributes: PrimitiveBuilder::attributes_key(&prim.attributes),
            material: canonical_mat,
            mode: prim.mode,
            indexed: prim.indices.is_some(),
        };

        let builder = self
            .static_prims
            .entry(prim_key)
            .or_insert_with_key(|key| PrimitiveBuilder::new(key.clone()));

        builder.append(prim, mesh_to_world, self.world_to_local)?;

        Ok(())
    }

    fn add_static_mesh(&mut self, mesh: &Mesh, mesh_to_world: glam::DMat4) -> Result<()> {
        for prim in &mesh.primitives {
            self.add_static_mesh_prim(prim, mesh_to_world)?;
        }
        Ok(())
    }

    pub fn add_static_scene(&mut self, scene: &Scene, model_to_world: glam::DMat4) -> Result<()> {
        for (node, context) in scene.iter_nodes() {
            if let Some(mesh) = &node.mesh {
                let mesh_to_world = model_to_world
                    * context.transform.as_dmat4()
                    * node.transform.to_mat4().as_dmat4();
                self.add_static_mesh(mesh, mesh_to_world)?;
            }

            // FUTURE: Add nonvisual geometry
        }

        // FUTURE: Collect lights and position/orientation at the node, BEFORE flattening

        Ok(())
    }

    pub fn into_document(self) -> Document {
        // Toplevel node with the transform and static mesh below it
        // Below that static node, all of the lights from the cominbed models with xforms on them
        // Toplevel node for each of the dynamic models

        let mut root_nodes = vec![];

        // Static models
        {
            let primitives = self
                .static_prims
                .iter()
                .map(|(key, builder)| builder.to_primitive(key))
                .collect();

            let mesh = Arc::new(Mesh {
                name: None,
                primitives,
            });

            let root = Arc::new(Node {
                name: None,
                mesh: Some(mesh),
                children: vec![], // FUTURE: Will add more when we have lights
                transform: Transform::Matrix(self.local_to_world.as_mat4()),
            });

            root_nodes.push(root);
        }

        // Dynamic models

        let default_scene = Scene {
            name: None,
            nodes: root_nodes,
        };

        Document {
            default_scene: Some(default_scene),
            other_scenes: vec![],
        }
    }

    fn canonicalize_material(&mut self, mat: Arc<Material>) -> Arc<Material> {
        if let Some(arc) = self.canonical_materials.get(&*mat) {
            return Arc::clone(arc);
        }
        self.canonical_materials.insert(Arc::clone(&mat));
        mat
    }
}
