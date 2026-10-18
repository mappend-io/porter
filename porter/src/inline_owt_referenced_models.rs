//! The `inline_owt_referenced_models` content transform. Source tiles are
//! GeoJSON features whose points each place a model referenced by URI
//! (`OWT_MDL`); the transform serves each tile as one GLB with every referenced
//! model placed and combined.

use crate::app_state::AppState;
use crate::utils::local_to_ecef_with_rotation;
use axum::http::StatusCode;
use bytes::Bytes;
use geojson::{GeoJson, Geometry, GeometryValue};
use iri_string::types::{UriAbsoluteStr, UriAbsoluteString, UriReferenceStr};
use metrics::counter;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use transforms::combine_referenced_models::*;

pub const NAME: &str = "inline_owt_referenced_models";

// Parsed source tiles kept for content requests
const SOURCE_CACHE_BYTES: u64 = 256 * 1_024 * 1_024;

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {}

/// A model placed by one point of a source feature
struct Placement {
    model_uri: UriAbsoluteString,
    lon: f64,
    lat: f64,
    elev: f64,
    heading: f64,
}

impl Placement {
    fn model_to_world(&self) -> glam::DMat4 {
        local_to_ecef_with_rotation(self.lon, self.lat, self.elev, 0.0, self.heading, 0.0)
    }
}

/// Source tiles, parsed into placements, by GeoJSON URI. A missing source tile
/// is cached as empty.
#[derive(Clone)]
pub struct SourceCache(moka::future::Cache<UriAbsoluteString, Arc<Vec<Placement>>>);

impl Default for SourceCache {
    fn default() -> Self {
        let cache = moka::future::Cache::builder()
            .max_capacity(SOURCE_CACHE_BYTES)
            .weigher(
                |uri: &UriAbsoluteString, placements: &Arc<Vec<Placement>>| {
                    let bytes = uri.as_str().len()
                        + placements
                            .iter()
                            .map(|p| std::mem::size_of::<Placement>() + p.model_uri.as_str().len())
                            .sum::<usize>();
                    bytes.try_into().unwrap_or(u32::MAX)
                },
            )
            .build();
        Self(cache)
    }
}

async fn source_placements(
    app_state: &AppState,
    geojson_uri: &UriAbsoluteStr,
) -> Result<Arc<Vec<Placement>>, StatusCode> {
    let cache = &app_state.inline_owt_referenced_models_sources.0;
    cache
        .try_get_with(geojson_uri.to_owned(), async {
            let bytes = match app_state.resource_loader.read_async(geojson_uri).await {
                Ok(bytes) => bytes,
                Err(resource_io::Error::NotFound(_)) => return Ok(Arc::new(vec![])),
                Err(_) => return Err(StatusCode::NOT_FOUND), // TODO: distinguish 404 vs 500
            };
            let geojson_uri = geojson_uri.to_owned();
            tokio::task::spawn_blocking(move || parse_placements(&bytes, &geojson_uri))
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .map(Arc::new)
        })
        .await
        .map_err(|e| *e)
}

fn parse_placements(
    bytes: &[u8],
    geojson_uri: &UriAbsoluteStr,
) -> Result<Vec<Placement>, StatusCode> {
    let geojson = GeoJson::from_reader(bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let features = match geojson {
        GeoJson::FeatureCollection(fc) => fc.features,
        GeoJson::Feature(f) => vec![f],
        GeoJson::Geometry(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let mut placements = vec![];
    for feature in &features {
        if let Some(Geometry {
            value: GeometryValue::MultiPoint { coordinates },
            ..
        }) = &feature.geometry
        {
            let property = |name| feature.properties.as_ref().and_then(|p| p.get(name));

            // TODO: The transform config should say what column to take
            let mdl = property("OWT_MDL")
                .and_then(|v| v.as_str())
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
            let model_uri: UriAbsoluteString = UriReferenceStr::new(mdl)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .resolve_against(geojson_uri)
                .and_normalize()
                .to_string()
                .parse()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            // TODO: The transform config should say what column to take, if any, otherwise use 0
            let heading = property("AOO").and_then(|v| v.as_f64()).unwrap_or(0.0);

            // TODO: I'm not handling reuse well, I'm making a separate unique
            // instance even if it's shared. This will burn us for trees, etc.
            for pos in coordinates {
                placements.push(Placement {
                    model_uri: model_uri.clone(),
                    lon: pos[0],
                    lat: pos[1],
                    elev: if pos.len() >= 3 { pos[2] } else { 0.0 },
                    heading,
                });
            }
        }
    }
    Ok(placements)
}

/// Whether the transform builds the content payload at `rest`; anything else
/// in the package is served as stored
pub fn transforms_payload(rest: &str) -> bool {
    rest.ends_with(".glb")
}

/// Build the combined GLB for a requested `.glb` tile, `content_uri` being the
/// request resolved against the package `root`
pub async fn tile_payload(
    app_state: &AppState,
    _options: &Options,
    root: &UriAbsoluteStr,
    content_uri: &str,
) -> Result<Bytes, StatusCode> {
    let geojson_uri: UriAbsoluteString = content_uri
        .replace(".glb", ".geojson")
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let placements = source_placements(app_state, &geojson_uri).await?;
    let referenced_models: Vec<_> = placements
        .iter()
        .map(|p| ReferencedModel {
            model_uri: p.model_uri.clone(),
            instances: vec![ReferencedModelInstance {
                model_to_world: p.model_to_world(),
            }],
        })
        .collect();

    // TODO: It'd be better to return an empty model than a 404
    let Some(first) = referenced_models.first() else {
        return Err(StatusCode::NOT_FOUND);
    };

    // TODO: Hackily taking the first reference's matrix as the root matrix
    let root_matrix = gltf_arc::snap_dmat4_to_f32(first.instances[0].model_to_world).as_mat4();

    let doc = combine_referenced_models(
        root,
        &geojson_uri,
        &referenced_models,
        root_matrix,
        app_state.resource_loader.clone(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut raw_doc = tokio::task::spawn_blocking(move || doc.to_gltf_types())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    raw_doc.0.buffers[0].data = raw_doc.1;

    // TODO: Put the y-up xform back, hackily!
    for node in &mut raw_doc.0.nodes {
        if let Some(m) = &node.matrix {
            node.matrix = Some(
                (glam::Mat4::from_rotation_x((-90.0_f32).to_radians())
                    * glam::Mat4::from_cols_array(m))
                .to_cols_array(),
            );
        }
    }

    let bytes = tokio::task::spawn_blocking(move || gltf_io::write::create_glb(&raw_doc.0))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    counter!("porter_transform_inline_owt_referenced_models_tiles_total").increment(1);
    counter!("porter_transform_inline_owt_referenced_models_unique_models_total")
        .increment(referenced_models.len() as u64);

    Ok(bytes)
}
