use crate::app_state::AppState;
use crate::tiles3d;
use crate::utils::*;
use anyhow::{Context, Result};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::IntoResponse;
use axum::{Json, extract::Path, extract::Query, extract::State, http::StatusCode};
use geojson::{GeoJson, Geometry, GeometryValue};
use iri_string::types::{UriAbsoluteStr, UriReferenceStr, UriRelativeStr};
use metrics::counter;
use serde::{Deserialize, Serialize};
use transforms::combine_referenced_models::*;

#[derive(Serialize)]
pub struct LayerItemEndpoint {
    r#type: String,
    uri: String,
}

#[derive(Serialize)]
pub struct LayerItem {
    id: String,
    description: String,
    endpoints: Vec<LayerItemEndpoint>,
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
            let mut endpoints = vec![];

            endpoints.push(LayerItemEndpoint {
                r#type: "3d_tiles".to_string(),
                uri: UriRelativeStr::new(&layer.id)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .resolve_against(base_uri)
                    .to_string(),
            });

            if layer.elevation_raster_content.is_some() {
                endpoints.push(LayerItemEndpoint {
                    r#type: "elevation_mapzen_terrarium_rgb_png".to_string(),
                    uri: UriRelativeStr::new(&format!("terrarium/{}", layer.id,))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                        .resolve_against(base_uri)
                        .to_string(),
                });
            }

            if layer.imagery_raster_content.is_some() {
                endpoints.push(LayerItemEndpoint {
                    r#type: "imagery_wmts_simple_jpg".to_string(),
                    uri: UriRelativeStr::new(&format!("wmts/{}", layer.id,))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                        .resolve_against(base_uri)
                        .to_string(),
                });
            }

            if layer.source_s2_content_extension == "geojson" {
                endpoints.push(LayerItemEndpoint {
                    r#type: "ogc_api_features".to_string(),
                    uri: UriRelativeStr::new(&format!("features/collections/{}/items", layer.id,))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                        .resolve_against(base_uri)
                        .to_string(),
                });
            }

            Ok(LayerItem {
                id: layer.id.clone(),
                description: layer.description.clone().unwrap_or("".to_string()),
                endpoints,
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
        let geojson = tokio::task::spawn_blocking(move || GeoJson::from_reader(&geojson_bytes[..]))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let features = match geojson {
            GeoJson::FeatureCollection(fc) => fc.features,
            GeoJson::Feature(f) => vec![f],
            GeoJson::Geometry(_) => todo!(),
        };

        let _instance_count = 0_u64;

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

        bytes = tokio::task::spawn_blocking(move || gltf_io::write::create_glb(&raw_doc.0))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        counter!("porter_transform_inline_owt_referenced_models_tiles_total").increment(1);
        counter!("porter_transform_inline_owt_referenced_models_unique_models_total")
            .increment(referenced_models.len() as u64);
        //counter!("porter_transform_inline_owt_referenced_models_unique_instances_total")
        //    .increment(features.len() as u64);
    }

    // TODO: If the payload is a tileset, we need to (temporarily, until CesiumJS is fixed), strip the tileset metadata and schema.
    // Or maybe just add the schema to the toplevel schema for terrain in the tileset. That's probably best.

    let content_type = sniff_content_type(&bytes);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    // TODO: Add s2_level
    counter!("porter_content_3dtiles_tiles_total").increment(1);

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

    // TODO: Would be nice to capture s2 level here
    counter!("porter_low_res_globe_passthrough_tiles_total").increment(1);

    Ok((headers, bytes))
}

#[derive(Debug, Deserialize)]
pub struct GetTerrariumTilePaths {
    pub id: String,
    pub zoom: i32,
    pub x: i32,
    pub y: String,
}

pub async fn get_terrarium_tile(
    State(app_state): State<AppState>,
    Path(paths): Path<GetTerrariumTilePaths>,
) -> Result<impl IntoResponse, StatusCode> {
    let actual_y: i32 = paths
        .y
        .strip_suffix(".png")
        .ok_or(StatusCode::NOT_FOUND)?
        .parse()
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let layer_def = app_state
        .get_layer_definition(&paths.id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if layer_def.elevation_raster_content.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let raster_png_uri = format!(
        // HACK: This only works for a .3tz. We should probably push /tileset.json if it's .3tz,
        // then strip the path in any case to get the root "dir".
        "{}/{}",
        layer_def.source_uri_content_template,
        layer_def.elevation_raster_content.clone().unwrap()
    );

    let fallback_raster_png_uri = layer_def.base_globe_terrain_uri.as_ref().map(|uri| {
        format!(
            // HACK: This only works for a .3tz. We should probably push /tileset.json if it's .3tz,
            // then strip the path in any case to get the root "dir".
            "{}/{}",
            uri,
            layer_def.elevation_raster_content.clone().unwrap()
        )
    });

    let index = terrarium::MapzenTileIndex {
        zoom: paths.zoom,
        col: paths.x,
        row: actual_y,
    };

    let bytes = terrarium::build_terrarium_rgb_tile(
        app_state.resource_loader.clone(),
        &raster_png_uri,
        fallback_raster_png_uri,
        layer_def.source_s2_content_package_level,
        &index,
    )
    .await
    // TODO: We should probably backfill here instead
    .unwrap_or_else(|_| terrarium::make_empty_tile(&index));

    let content_type = "image/png";
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    counter!("porter_terrarium_tiles_total", "zoom_level" => paths.zoom.to_string()).increment(1);

    Ok((headers, bytes))
}

#[derive(Debug, Deserialize)]
pub struct GetWmtsSimpleImageryPaths {
    pub id: String,
    pub zoom: i32,
    pub x: i32,
    pub y: String,
}

pub async fn get_wmts_simple_imagery(
    State(app_state): State<AppState>,
    Path(paths): Path<GetTerrariumTilePaths>,
) -> Result<impl IntoResponse, StatusCode> {
    let actual_y: i32 = paths
        .y
        .strip_suffix(".jpg")
        .ok_or(StatusCode::NOT_FOUND)?
        .parse()
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let layer_def = app_state
        .get_layer_definition(&paths.id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if layer_def.imagery_raster_content.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let raster_png_uri = format!(
        // HACK: This only works for a .3tz. We should probably push /tileset.json if it's .3tz,
        // then strip the path in any case to get the root "dir".
        "{}/{}",
        layer_def.source_uri_content_template,
        layer_def.imagery_raster_content.clone().unwrap()
    );

    let fallback_raster_png_uri = layer_def.base_globe_terrain_uri.as_ref().map(|uri| {
        format!(
            // HACK: This only works for a .3tz. We should probably push /tileset.json if it's .3tz,
            // then strip the path in any case to get the root "dir".
            "{}/{}",
            uri,
            layer_def.imagery_raster_content.clone().unwrap()
        )
    });

    let index = terrarium::MapzenTileIndex {
        zoom: paths.zoom,
        col: paths.x,
        row: actual_y,
    };

    let bytes = terrarium::build_simple_wmts_imagery_tile(
        app_state.resource_loader.clone(),
        &raster_png_uri,
        fallback_raster_png_uri,
        layer_def.source_s2_content_package_level,
        &index,
    )
    .await
    // TODO: We should probably backfill here instead
    .unwrap_or_else(|_| terrarium::make_empty_wmts_simple_imagery_tile(&index));

    let content_type = "image/jpg";
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    counter!("porter_wmts_simple_tiles_total", "zoom_level" => paths.zoom.to_string()).increment(1);

    Ok((headers, bytes))
}

pub async fn get_features_api() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Porter Feature API",
            "version": "1.0.0"
        },
        "paths": {
            "/features": {
                "get": {
                    "responses": {
                        "200": { "description": "Landing page" }
                    }
                }
            },
            "/features/collections": {
                "get": {
                    "responses": {
                        "200": { "description": "Collections" }
                    }
                }
            }
        }
    }))
}

pub async fn get_features_landing(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    let base = format!("{}/features", app_state.config.base_url);
    Json(serde_json::json!({
        "title": "Porter Feature API",
        "description": "OGC API - Features",
        "links": [
            { "rel": "self",         "type": "application/json",     "title": "This document",        "href": format!("{base}") },
            { "rel": "conformance",  "type": "application/json",     "title": "Conformance",          "href": format!("{base}/conformance") },
            { "rel": "data",         "type": "application/json",     "title": "Collections",          "href": format!("{base}/collections") },
            { "rel": "service-desc", "type": "application/vnd.oai.openapi+json;version=3.0", "title": "API Definition", "href": format!("{base}/api") },
        ]
    }))
}

pub async fn get_features_conformance() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "conformsTo": [
            "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/core",
            "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/oas30",
            "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/geojson"
        ]
    }))
}

fn collection_doc(base: &str, id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "title": id,
        "extent": {
            "spatial": {
                // TODO: Being a bit lazy here, we could calculate a rough coverage from the given s2 coverage tokens in the layer
                "bbox": [[-180.0, -90.0, 180.0, 90.0]],
                "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
            }
        },
        "crs": [ "http://www.opengis.net/def/crs/OGC/1.3/CRS84" ],
        "itemType": "feature",
        "links": [
            {
                "rel": "items",
                "type": "application/geo+json",
                "title": format!("{id} features"),
                "href": format!("{base}/collections/{id}/items")
            },
            {
                "rel": "self",
                "type": "application/json",
                "href": format!("{base}/collections/{id}")
            }
        ]
    })
}

pub async fn get_features_collections(
    State(app_state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let layers = app_state
        .get_layer_definitions()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // TODO: Check on trailing slashes/mashing, use proper join
    let base = format!("{}/features", app_state.config.base_url);

    let collections: Vec<_> = layers
        .iter()
        .filter(|layer| layer.source_s2_content_extension == "geojson")
        .map(|layer| collection_doc(&base, &layer.id))
        .collect();

    Ok(Json(serde_json::json!({
        "links": [
            { "rel": "self", "type": "application/json", "href": format!("{base}/collections") }
        ],
        "collections": collections
    })))
}

pub async fn get_features_collection(
    State(app_state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    // TODO: Check on trailing slashes/mashing, use proper join
    let base = format!("{}/features", app_state.config.base_url);
    Json(collection_doc(&base, &id))
}

#[derive(Deserialize)]
pub struct GetFeaturesPaths {
    pub id: String,
}

#[derive(Deserialize)]
pub struct GetFeaturesBboxParams {
    // minx,miny,maxx,maxy
    pub bbox: Option<String>,

    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn get_features_items(
    State(app_state): State<AppState>,
    Query(params): Query<GetFeaturesBboxParams>,
    Path(paths): Path<GetFeaturesPaths>,
) -> Result<impl IntoResponse, StatusCode> {
    let layer_def = app_state
        .get_layer_definition(&paths.id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // TODO: check for the maxar vector extension, so we can skip over the transformed inline layers
    if layer_def.source_s2_content_extension != "geojson" {
        return Err(StatusCode::NOT_FOUND);
    }

    let gd_coverage = if let Some(bbox_str) = &params.bbox {
        let coords = bbox_str
            .split(',')
            .map(|s| s.parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| StatusCode::BAD_REQUEST)?;

        let [minx, miny, maxx, maxy] = coords.as_slice() else {
            return Err(StatusCode::BAD_REQUEST);
        };

        terrarium::Wgs84Rect2d {
            south_west: terrarium::Wgs84Coord2d {
                lon: *minx,
                lat: *miny,
            },
            north_east: terrarium::Wgs84Coord2d {
                lon: *maxx,
                lat: *maxy,
            },
        }
    } else {
        // A dummy bounding box just to poke the system, QGIS sends out a probe of a single
        // feature. We don't really have a way to paginate features, or find by id, or find
        // the first. So.. do this for now. What would be a better way to do do this?
        terrarium::Wgs84Rect2d {
            south_west: terrarium::Wgs84Coord2d {
                lon: 126.5,
                lat: 35.3,
            },
            north_east: terrarium::Wgs84Coord2d {
                lon: 126.6,
                lat: 35.4,
            },
        }
    };

    // What s2 tiles does the bbox intersect at the layer_def's content level?
    let s2_content_tokens =
        terrarium::gd_rect_to_s2_coverage(&gd_coverage, layer_def.source_s2_content_max_level);
    tracing::trace!(
        "Found these s2 tokens for features request: {:?}",
        s2_content_tokens
    );

    let resource_loader = app_state.resource_loader.clone();

    // For each content S2 tile, extract the vectors and only take those that intersect the box
    let mut features: Vec<geojson::Feature> =
        futures::future::join_all(s2_content_tokens.iter().map(|token| {
            let resource_loader = resource_loader.clone();

            let parent = token.parent(layer_def.source_s2_content_package_level as u64);
            let source_archive = layer_def.resolve_content_uri_template(&parent.to_token());

            async move {
                let (face, level, col, row) =
                    crate::s2_utils::face_level_col_row_from_cell_id(*token);
                let uri_str = format!(
                    "{}/{}/{}/{}/{}.geojson",
                    source_archive, face, level, col, row
                );

                let uri = iri_string::types::UriAbsoluteStr::new(&uri_str)
                    .map_err(|_| anyhow::anyhow!("Invalid URI: {}", uri_str))?;

                // Read the geojson from the storage backend
                let bytes = match resource_loader.read_async(uri).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::trace!(
                            "Failed to read geojson from {} (likely empty tile): {:?}",
                            uri,
                            e
                        );
                        return Err(anyhow::anyhow!("Read error"));
                    }
                };

                // Parse geojson
                let geojson_str = std::str::from_utf8(&bytes)?;
                let parsed = match geojson_str.parse::<geojson::GeoJson>() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("Failed to parse geojson from {}: {:?}", uri, e);
                        return Err(anyhow::anyhow!("Parse error"));
                    }
                };

                let features = match parsed {
                    geojson::GeoJson::FeatureCollection(fc) => fc.features,
                    geojson::GeoJson::Feature(f) => vec![f],
                    _ => vec![],
                };

                // TODO: Filter against the actual bbox, this is loose

                Ok::<_, anyhow::Error>(features)
            }
        }))
        .await
        .into_iter()
        // It's ok that these fail sometimes, we might not have entire world coverage of input data
        .filter_map(|r| r.ok())
        .flatten()
        .collect();

    // Respect the limit parameter if QGIS asked for a small probe (e.g. limit=10)
    if let Some(limit) = params.limit {
        features.truncate(limit);
    }

    counter!("porter_features_total").increment(features.len() as u64);
    counter!("porter_queries_total").increment(1);
    tracing::debug!("OGC API - Features call returned {} items", features.len());

    // We have features, but want a geojson file
    let fc = geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    let combined_geojson = geojson::GeoJson::FeatureCollection(fc);
    let bytes = bytes::Bytes::from(combined_geojson.to_string());

    let content_type = sniff_content_type(&bytes);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    Ok((headers, bytes))
}
