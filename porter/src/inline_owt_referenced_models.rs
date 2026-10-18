//! The `inline_owt_referenced_models` content transform. Source tiles are
//! GeoJSON features whose points each place a model referenced by URI
//! (`OWT_MDL`); the transform serves each tile as one GLB with every referenced
//! model placed and combined.

use crate::app_state::AppState;
use crate::utils::local_to_ecef_with_rotation;
use anyhow::Context;
use axum::http::StatusCode;
use bytes::Bytes;
use geojson::{GeoJson, Geometry, GeometryValue};
use iri_string::types::{UriAbsoluteStr, UriReferenceStr};
use metrics::counter;
use serde::{Deserialize, Serialize};
use transforms::combine_referenced_models::*;

pub const NAME: &str = "inline_owt_referenced_models";

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {}

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
    let resolved = content_uri.replace(".glb", ".geojson");
    let uri = UriAbsoluteStr::new(&resolved).map_err(|_| StatusCode::BAD_REQUEST)?;

    let bytes = app_state
        .resource_loader
        .read_async(uri)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?; // TODO: distinguish 404 vs 500

    let mut referenced_models = vec![];
    let geojson = tokio::task::spawn_blocking(move || GeoJson::from_reader(&bytes[..]))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let features = match geojson {
        GeoJson::FeatureCollection(fc) => fc.features,
        GeoJson::Feature(f) => vec![f],
        GeoJson::Geometry(_) => todo!(),
    };

    for feature in &features {
        if let Some(Geometry {
            value: GeometryValue::MultiPoint { coordinates },
            ..
        }) = &feature.geometry
        {
            let mdl = feature
                .properties
                .as_ref()
                // TODO: The transform config should say what column to take
                .and_then(|p| p.get("OWT_MDL"))
                .and_then(|v| v.as_str());

            let uri_ref = UriReferenceStr::new(mdl.unwrap())
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let norm_ref = uri_ref.resolve_against(uri).and_normalize().to_string();

            let aoo = feature
                .properties
                .as_ref()
                // TODO: The transform config should say what column to take, if any, otherwise use 0
                .and_then(|p| p.get("AOO"))
                .and_then(|v| v.as_f64());

            for pos in coordinates {
                let lon = pos[0];
                let lat = pos[1];
                let elev = if pos.len() >= 3 { Some(pos[2]) } else { None };

                let model_to_world = local_to_ecef_with_rotation(
                    lon,
                    lat,
                    elev.unwrap_or(0.0),
                    0.0,
                    aoo.unwrap_or(0.0),
                    0.0,
                );

                // TODO: I'm not handling reuse well, I'm making a separate
                // unique instance even if it's shared. This will burn us
                // for trees, etc.
                referenced_models.push(ReferencedModel {
                    model_uri: norm_ref
                        .parse()
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    instances: vec![ReferencedModelInstance { model_to_world }],
                });
            }
        }
    }

    // TODO: It'd be better to return an empty model than a 404
    if referenced_models.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    // TODO: Hackily taking the first reference's matrix as the root matrix
    let root_matrix = gltf_arc::snap_dmat4_to_f32(
        referenced_models
            .first()
            .context("No reference models")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .instances
            .first()
            .context("No instances in first referenced model")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .model_to_world,
    )
    .as_mat4();

    let doc = combine_referenced_models(
        root,
        uri,
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
