use crate::app_state::AppState;
use crate::tiles3d;
use crate::utils::*;
use anyhow::{Context, Result};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::IntoResponse;
use axum::{Json, extract::Path, extract::State, http::StatusCode};
use geojson::{GeoJson, Geometry, GeometryValue};
use iri_string::types::{UriAbsoluteStr, UriReferenceStr, UriRelativeStr};
use serde::{Deserialize, Serialize};
use transforms::combine_referenced_models::*;

#[derive(Serialize)]
pub struct LayerItem {
    id: String,
    description: String,
    endpoint: String,
}

#[derive(Serialize)]
pub struct ListLayerItems {
    items: Vec<LayerItem>,
}

pub async fn get_layers(
    State(app_state): State<AppState>,
) -> Result<Json<ListLayerItems>, StatusCode> {
    let layers = app_state
        .get_layer_definitions()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let base_uri = UriAbsoluteStr::new(&app_state.config.base_url)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut items: Vec<LayerItem> = layers
        .iter()
        .map(|layer| -> Result<LayerItem, StatusCode> {
            let uri = UriRelativeStr::new(&layer.id)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .resolve_against(base_uri)
                .to_string();
            Ok(LayerItem {
                id: layer.id.clone(),
                description: layer.description.clone().unwrap_or("".to_string()),
                endpoint: uri,
            })
        })
        .collect::<Result<Vec<LayerItem>, StatusCode>>()?;
    items.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(ListLayerItems { items }))
}

pub async fn get_root_tileset(
    State(app_state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<tiles3d::Tileset>, StatusCode> {
    let layer_def = app_state
        .get_layer_definition(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let tileset = layer_def.root_tileset();
    Ok(Json(tileset))
}

#[derive(Deserialize)]
pub struct GetRootTilesetTopNodePaths {
    pub id: String,
}

pub async fn get_root_tileset_top_node(
    State(app_state): State<AppState>,
    Path(paths): Path<GetRootTilesetTopNodePaths>,
) -> Result<Json<tiles3d::Tileset>, StatusCode> {
    let layer_def = app_state
        .get_layer_definition(&paths.id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let tileset = layer_def.synthesize_s2_root();
    Ok(Json(tileset))
}

#[derive(Deserialize)]
pub struct GetChildTilesetPaths {
    pub id: String,
    pub face: u8,
    pub level: i32,
    pub col: i32,
    pub row: String,
}

pub async fn get_child_tileset(
    State(app_state): State<AppState>,
    Path(paths): Path<GetChildTilesetPaths>,
) -> Result<Json<tiles3d::Tileset>, StatusCode> {
    // TODO: If level is >= content level, reach into the tileset and get the tileset, walk it and find the
    // child for face/level/col/row
    // TODO: Use face/level/col/row to figure out which content to reach into

    // When we repack tileset json from within a 3tz, we can strip out the tileset metadata
    // and replace it so we have consistent.

    // TODO: If the level is less than the content level, we synthesize a tile.
    // If it's >=, we want to get the tile from the appropriate tileset.
    // For now, we synthesize all tilesets

    let row: i32 = paths
        .row
        .strip_suffix(".json")
        .ok_or(StatusCode::NOT_FOUND)?
        .parse()
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let layer_def = app_state
        .get_layer_definition(&paths.id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let tileset = layer_def.synthesize_tileset(paths.face, paths.level, paths.col, row);
    Ok(Json(tileset))
}

#[derive(Deserialize)]
pub struct GetContentToplevelPaths {
    pub id: String,
    pub token: String,
}

pub async fn get_content_toplevel(
    State(app_state): State<AppState>,
    Path(paths): Path<GetContentToplevelPaths>,
) -> Result<Json<tiles3d::Tileset>, StatusCode> {
    let layer_def = app_state
        .get_layer_definition(&paths.id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mut content_root = layer_def.resolve_content_uri_template(&paths.token);
    if content_root.ends_with(".3tz") {
        content_root.push_str("/tileset.json");
    }
    let uri = UriAbsoluteStr::new(&content_root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // TODO: We should probably cache this

    let tileset = get_content_root_tileset(
        app_state.resource_loader.clone(),
        uri,
        layer_def.source_s2_content_min_level,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(tileset))
}

#[derive(Deserialize)]
pub struct GetContentPayloadPaths {
    pub id: String,
    pub token: String,
    pub rest: String,
}

// ..this is where content transform pipeline would run
// ..if there is no transform, we can just reframe deflated compressed entry from 3tz, or zstd
// note that means we probably want some helper on resource loader to get the compressed content and method
// TODO: If level < content level, read from one of the bg terrain files
pub async fn get_content_payload(
    State(app_state): State<AppState>,
    Path(paths): Path<GetContentPayloadPaths>,
) -> Result<impl IntoResponse, StatusCode> {
    let layer_def = app_state
        .get_layer_definition(&paths.id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let mut content_root = layer_def.resolve_content_uri_template(&paths.token);
    if content_root.ends_with(".3tz") {
        content_root.push_str("/tileset.json");
    }
    let root = UriAbsoluteStr::new(&content_root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let relative = UriReferenceStr::new(&paths.rest).map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut resolved = relative.resolve_against(root).to_string();

    // HACK: I don't know what direction content transforms will go.
    // For now, to get something out, since we only have one, do the
    // simple thing. Eventually these will be chained together and
    // more configurable.
    let inline_owt_referenced_models = layer_def
        .content_transforms
        .contains("inline_owt_referenced_models");

    if inline_owt_referenced_models && paths.rest.ends_with(".glb") {
        resolved = resolved.replace(".glb", ".geojson");
    }

    let uri = UriAbsoluteStr::new(&resolved).map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut bytes = app_state
        .resource_loader
        .read_async(uri)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?; // TODO: distinguish 404 vs 500

    // This all doesn't belong here, but I am waiting to see what the second transform looks like
    if inline_owt_referenced_models && paths.rest.ends_with(".glb") {
        let mut referenced_models = vec![];
        let geojson_bytes = app_state
            .resource_loader
            .read_async(uri)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let geojson = GeoJson::from_reader(&geojson_bytes[..])
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

        let mut raw_doc = doc
            .to_gltf_types()
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

        bytes = gltf_io::write::create_glb(&raw_doc.0)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    // TODO: If the payload is a tileset, we need to (temporarily, until CesiumJS is fixed), strip the tileset metadata and schema.
    // Or maybe just add the schema to the toplevel schema for terrain in the tileset. That's probably best.

    let content_type = sniff_content_type(&bytes);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    Ok((headers, bytes))
}

#[derive(Deserialize)]
pub struct GetBaseGlobeTerrainPayloadPaths {
    pub id: String,
    pub rest: String,
}

pub async fn get_base_globe_terrain_payload(
    State(app_state): State<AppState>,
    Path(paths): Path<GetBaseGlobeTerrainPayloadPaths>,
) -> Result<impl IntoResponse, StatusCode> {
    let layer_def = app_state
        .get_layer_definition(&paths.id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if layer_def.base_globe_terrain_uri.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let glb_uri = format!(
        "{}/{}",
        layer_def.base_globe_terrain_uri.as_ref().unwrap(),
        paths.rest
    );
    let uri = UriAbsoluteStr::new(&glb_uri).map_err(|_| StatusCode::BAD_REQUEST)?;
    let bytes = app_state
        .resource_loader
        .read_async(uri)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?; // TODO: distinguish 404 vs 500

    let content_type = sniff_content_type(&bytes);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    Ok((headers, bytes))
}
