use super::*;
use std::sync::Arc;

pub struct NodeIter<'a> {
    stack: Vec<(&'a Arc<Node>, TraversalContext)>,
}

impl<'a> Iterator for NodeIter<'a> {
    type Item = (&'a Node, TraversalContext);

    fn next(&mut self) -> Option<Self::Item> {
        let (node, context) = self.stack.pop()?;

        // We pop LIFO, so push in reverse
        for child in node.children.iter().rev() {
            let mut child_context = context.clone();
            child_context.transform = context.transform * node.transform.to_mat4();
            self.stack.push((child, child_context));
        }

        Some((node, context))
    }
}

#[derive(Clone, Default, Debug)]
pub struct TraversalContext {
    pub transform: glam::Mat4,
}

impl Node {
    pub fn for_each_node<F>(&self, context: &TraversalContext, f: &mut F)
    where
        F: FnMut(&Node, &TraversalContext),
    {
        f(self, context);

        for child in &self.children {
            let mut child_context = context.clone();
            child_context.transform = context.transform * self.transform.to_mat4();
            child.for_each_node(&child_context, f);
        }
    }
}

impl Scene {
    pub fn iter_nodes(&self) -> NodeIter<'_> {
        let mut stack = Vec::with_capacity(self.nodes.len());
        for node in self.nodes.iter().rev() {
            stack.push((node, TraversalContext::default()));
        }
        NodeIter { stack }
    }

    pub fn for_each_node<F>(&self, context: &TraversalContext, mut f: F)
    where
        F: FnMut(&Node, &TraversalContext),
    {
        for node in &self.nodes {
            node.for_each_node(context, &mut f);
        }
    }
}

impl Document {
    // Depth-first traversal of all nodes
    pub fn iter_nodes(&self) -> NodeIter<'_> {
        let mut stack = vec![];

        if let Some(scene) = &self.default_scene {
            for node in scene.nodes.iter().rev() {
                stack.push((node, TraversalContext::default()));
            }
        }

        for scene in self.other_scenes.iter().rev() {
            for node in scene.nodes.iter().rev() {
                stack.push((node, TraversalContext::default()));
            }
        }

        NodeIter { stack }
    }

    pub fn for_each_node<F>(&self, mut f: F)
    where
        F: FnMut(&Node, &TraversalContext),
    {
        if let Some(scene) = &self.default_scene {
            scene.for_each_node(&TraversalContext::default(), &mut f);
        }

        for scene in &self.other_scenes {
            scene.for_each_node(&TraversalContext::default(), &mut f);
        }
    }
}
