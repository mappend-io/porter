use anyhow::{Context, Result};
use gltf_arc::Document;
use gltf_arc::combiner::Combiner;
use iri_string::types::{UriAbsoluteStr, UriAbsoluteString, UriReferenceStr};
use itertools::Itertools;
use resource_io::ResourceLoader;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct ReferencedModelInstance {
    pub model_to_world: glam::DMat4,
    // FUTURE: Add schema, property values
}

#[derive(Clone)]
pub struct ReferencedModel {
    pub model_uri: UriAbsoluteString,
    pub instances: Vec<ReferencedModelInstance>,
}

// prefix must have a trailing slash
fn make_relative(base: &str, target: &str, prefix: &str) -> String {
    debug_assert!(prefix.ends_with('/'), "prefix must have a trailing slash");

    // Trim filename off base first
    let base_dir = match base.rfind('/') {
        Some(i) => &base[..=i],
        None => base,
    };

    // Strip off the common prefix, including the leading /
    let base_rest = &base_dir[prefix.len()..];
    let target_rest = &target[prefix.len()..];

    // How many ../'s do we need?
    let up_count = base_rest.split('/').filter(|s| !s.is_empty()).count();

    // Build up the relative path
    let mut relative = "../".repeat(up_count);
    relative.push_str(target_rest);

    if relative.is_empty() {
        relative.push('.');
    }

    relative
}

fn dir_of(uri: &str) -> &str {
    match uri.rfind('/') {
        // Include the trailing slash
        Some(i) => &uri[..=i],
        None => uri,
    }
}

pub fn rewrite_uris_for_combining(
    doc: &mut gltf_types::Document,
    content_root_uri: &UriAbsoluteStr,
    content_geojson_uri: &UriAbsoluteStr,
    model_uri: &UriAbsoluteStr,
) -> Result<()> {
    let content_root_uri_dir = dir_of(content_root_uri.as_str());

    // content_geojson_uri is something like: /content/00000/4/12/123/456.geojson
    // model_uri is something like: `s3://bucket/path/00000/vectors/xyz.3tz/models/type/name.glb`,
    // which would be served as `/content/00000/models/type/name.glb`
    // We want URIs that this model references to be rewritten to be relative to the
    // geojson's location, e.g. a sibling `xyz.jpg` becomes: ../../models/type/xyz.jpg
    for image in &mut doc.images {
        if let Some(uri) = &image.uri {
            // Resolve the image reference to an absolute URI
            let image_ref = UriReferenceStr::new(uri)?;
            let abs_image_uri_str = image_ref
                .resolve_against(model_uri)
                .and_normalize()
                .to_string();
            let abs_image_uri = UriAbsoluteStr::new(&abs_image_uri_str)?;

            // Rewrite to a path relative to the geojson's location
            let new_uri = make_relative(
                content_geojson_uri.as_str(),
                abs_image_uri.as_str(),
                content_root_uri_dir,
            );

            image.uri = Some(new_uri);
        }
    }

    Ok(())
}

pub async fn combine_referenced_models(
    content_root_uri: &UriAbsoluteStr,
    content_geojson_uri: &UriAbsoluteStr,
    referenced_models: &[ReferencedModel],
    // Conveyed as Mat4, not DMat4 to ensure you better make it f32 friendly
    combined_model_matrix: glam::Mat4,
    resource_loader: ResourceLoader,
) -> Result<Document> {
    // What unique model URIs do we need to fetch?
    let unique_uris: Vec<&UriAbsoluteStr> = referenced_models
        .iter()
        .map(|m| m.model_uri.as_ref())
        .unique()
        .collect();

    // Read them all into gltf_arc::Documents, with URIs rewritten as if each
    // model were co-located with the .geojson
    let models = read_many_models(
        &unique_uris,
        content_root_uri,
        content_geojson_uri,
        resource_loader,
    )
    .await?;

    // Combine them (offloaded to blocking pool to avoid blocking Tokio runtime on large inputs)
    let references = referenced_models.to_vec();
    let doc = tokio::task::spawn_blocking(move || {
        combine_many_glbs(models, &references, combined_model_matrix)
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {}", e))??;

    Ok(doc)
}

// Below this many models per task, the cost of another blocking task outweighs
// spreading the parse over more threads
const MIN_MODELS_PER_PARSE_TASK: usize = 16;

// Fetch all of the unique model URIs in parallel (ordered for cache coherency),
// then parse, rewrite and convert each into a gltf_arc::Document. Parsing is
// split into chunks across blocking threads rather than one task per model, a
// tile can reference thousands of models.
async fn read_many_models(
    uris: &[&UriAbsoluteStr],
    content_root_uri: &UriAbsoluteStr,
    content_geojson_uri: &UriAbsoluteStr,
    resource_loader: ResourceLoader,
) -> Result<BTreeMap<UriAbsoluteString, gltf_arc::Document>> {
    // TODO: Pipeline reads and parses — process each model as its bytes arrive
    // rather than waiting for all reads to complete before starting any parsing.
    let mut res = resource_loader.read_many(uris).await;

    let mut inputs = Vec::with_capacity(uris.len());
    for uri in uris {
        let key = UriAbsoluteStr::to_owned(uri);
        let bytes = match res.remove(&key) {
            Some(Ok(b)) => b,
            Some(Err(e)) => return Err(anyhow::anyhow!("Failed to read {uri}: {e}")),
            None => return Err(anyhow::anyhow!("No result returned for URI: {uri}")),
        };
        inputs.push((key, bytes));
    }

    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk_size = inputs
        .len()
        .div_ceil(workers)
        .max(MIN_MODELS_PER_PARSE_TASK);

    let mut tasks = tokio::task::JoinSet::new();
    while !inputs.is_empty() {
        let chunk = inputs.split_off(inputs.len().saturating_sub(chunk_size));
        let content_root_uri = content_root_uri.to_owned();
        let content_geojson_uri = content_geojson_uri.to_owned();
        tasks.spawn_blocking(move || {
            chunk
                .into_iter()
                .map(|(uri, bytes)| {
                    let mut raw = gltf_io::read::parse_model(bytes, &uri)
                        .with_context(|| format!("Failed to parse model: {uri}"))?;
                    rewrite_uris_for_combining(
                        &mut raw,
                        &content_root_uri,
                        &content_geojson_uri,
                        &uri,
                    )?;
                    let converted = gltf_arc::Document::try_from(&raw)
                        .with_context(|| format!("Failed to convert document: {uri}"))?;
                    Ok((uri, converted))
                })
                .collect::<Result<Vec<_>>>()
        });
    }

    let mut documents = BTreeMap::new();
    while let Some(result) = tasks.join_next().await {
        let parsed = result.map_err(|e| anyhow::anyhow!("spawn_blocking failed: {}", e))??;
        documents.extend(parsed);
    }

    Ok(documents)
}

fn combine_many_glbs(
    // NOTE: Using BTreeMap to ensure same order each time
    models: BTreeMap<UriAbsoluteString, gltf_arc::Document>,
    references: &[ReferencedModel],
    // Conveyed as Mat4, not DMat4 to ensure you better make it f32 friendly
    // TODO: Might change this and we'll do a snap to f32 here?
    combined_model_matrix: glam::Mat4,
) -> Result<gltf_arc::Document> {
    let mut combiner = Combiner::new(combined_model_matrix);

    for refs in references {
        let model = models
            .get(&refs.model_uri)
            .context("Could not find model")?;
        for inst in &refs.instances {
            let scene = model.default_scene.as_ref().context("No default scene")?;
            combiner.add_static_scene(scene, inst.model_to_world)?;
        }
    }

    Ok(combiner.into_document())
}
