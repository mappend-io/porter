//! The `inline_owt_referenced_models` content transform. Source tiles are
//! GeoJSON features whose points each place a model referenced by URI
//! (`OWT_MDL`); the transform serves each tile as one GLB with every referenced
//! model placed and combined.
//!
//! With `tileLevel`, tiles are served below the source content level: a tile
//! holds the models whose points fall inside its cell, and the transform builds
//! the tile JSON for the levels between, from the source points.

use crate::app_state::AppState;
use crate::layer_definition::{LayerDefinition, MAX_ELEV, MIN_ELEV};
use crate::s2_utils::{cell_id_from_face_level_col_row, face_level_col_row_from_cell_id};
use crate::tiles3d;
use crate::utils::{local_to_ecef_with_rotation, read_error_status};
use axum::http::StatusCode;
use bytes::Bytes;
use geojson::{GeoJson, Geometry, GeometryValue};
use iri_string::types::{UriAbsoluteStr, UriAbsoluteString, UriReferenceStr};
use metrics::counter;
use s2::cellid::CellID;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use transforms::combine_referenced_models::{
    ReferencedModel, ReferencedModelInstance, combine_referenced_models,
};

pub const NAME: &str = "inline_owt_referenced_models";

// Models reach past the point that places them, so sub-tile bounds are padded
// by this much horizontally. The largest building measured in r8v2 data reached
// 171m from its point.
const MODEL_REACH_METERS: f64 = 200.0;
// Added above and below the heights from a feature's `HGT`
const HEIGHT_MARGIN_METERS: f64 = 10.0;
const EARTH_RADIUS_METERS: f64 = 6_371_000.0;
// Parsed source tiles kept for tile JSON and content requests
const SOURCE_CACHE_BYTES: u64 = 256 * 1_024 * 1_024;

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    /// Serve tiles at this S2 level instead of the source content level
    /// (`sourceS2ContentMaxLevel`), each with the models whose points fall in
    /// its cell
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_level: Option<i32>,
}

impl Options {
    pub fn validate(&self, source_level: i32) -> Result<(), String> {
        match self.tile_level {
            Some(level) if !(source_level..=30).contains(&level) => Err(format!(
                "{NAME} tileLevel {level} must be between the source content level \
                 {source_level} and 30"
            )),
            _ => Ok(()),
        }
    }
}

/// A model placed by one point of a source feature
struct Placement {
    model_uri: UriAbsoluteString,
    leaf: CellID,
    lon: f64,
    lat: f64,
    elev: f64,
    heading: f64,
    height: Option<f64>,
}

impl Placement {
    fn model_to_world(&self) -> glam::DMat4 {
        local_to_ecef_with_rotation(self.lon, self.lat, self.elev, 0.0, self.heading, 0.0)
    }

    fn in_cell(&self, cell: CellID) -> bool {
        self.leaf.parent(cell.level()) == cell
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
                Err(e) if e.is_not_found() => return Ok(Arc::new(vec![])),
                Err(e) => return Err(read_error_status(&e)),
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
            let height = property("HGT").and_then(|v| v.as_f64());

            // TODO: I'm not handling reuse well, I'm making a separate unique
            // instance even if it's shared. This will burn us for trees, etc.
            for pos in coordinates {
                let (lon, lat) = (pos[0], pos[1]);
                placements.push(Placement {
                    model_uri: model_uri.clone(),
                    leaf: CellID::from(s2::latlng::LatLng::from_degrees(lat, lon)),
                    lon,
                    lat,
                    elev: if pos.len() >= 3 { pos[2] } else { 0.0 },
                    heading,
                    height,
                });
            }
        }
    }
    Ok(placements)
}

/// The source GeoJSON holding `cell`'s models, inside the package at `root`
fn source_geojson_uri(
    root: &UriAbsoluteStr,
    cell: CellID,
    source_level: i32,
) -> Result<UriAbsoluteString, StatusCode> {
    let (face, level, col, row) = face_level_col_row_from_cell_id(cell.parent(source_level as u64));
    UriReferenceStr::new(&format!("{face}/{level}/{col}/{row}.geojson"))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .resolve_against(root)
        .to_string()
        .parse()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// The cell of a `{face}/{level}/{col}/{row}.glb` content path
fn content_cell(rest: &str) -> Option<CellID> {
    let parts = rest
        .strip_suffix(".glb")?
        .split('/')
        .map(|part| part.parse::<i32>().ok())
        .collect::<Option<Vec<_>>>()?;
    let [face, level, col, row] = parts.as_slice() else {
        return None;
    };
    if !(0..6).contains(face) || !(0..=30).contains(level) {
        return None;
    }
    let max_index = 1i64 << level;
    if !(0..max_index).contains(&(*col as i64)) || !(0..max_index).contains(&(*row as i64)) {
        return None;
    }
    Some(cell_id_from_face_level_col_row(
        *face as u8,
        *level,
        *col,
        *row,
    ))
}

/// Whether the transform builds the content payload at `rest`; anything else
/// in the package is served as stored
pub fn transforms_payload(rest: &str) -> bool {
    rest.ends_with(".glb")
}

/// Build the combined GLB for a requested `.glb` tile, `content_uri` being the
/// request `rest` resolved against the package `root`
pub async fn tile_payload(
    app_state: &AppState,
    layer_def: &LayerDefinition,
    options: &Options,
    root: &UriAbsoluteStr,
    rest: &str,
    content_uri: &str,
) -> Result<Bytes, StatusCode> {
    // Tiles below the source level take their models from the source tile above
    let source_level = layer_def.source_s2_content_max_level;
    let sub_tile = options
        .tile_level
        .and(content_cell(rest))
        .filter(|cell| cell.level() as i32 > source_level);
    let geojson_uri = match sub_tile {
        Some(cell) => source_geojson_uri(root, cell, source_level)?,
        None => content_uri
            .replace(".glb", ".geojson")
            .parse()
            .map_err(|_| StatusCode::BAD_REQUEST)?,
    };

    let placements = source_placements(app_state, &geojson_uri).await?;
    let referenced_models: Vec<_> = placements
        .iter()
        .filter(|p| sub_tile.is_none_or(|cell| p.in_cell(cell)))
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

/// Tile JSON for a cell from the source content level down to `tileLevel`,
/// built from the source points: only cells with models get a tile, bounded
/// by their models. `None` for cells the transform doesn't tile.
pub async fn tileset(
    app_state: &AppState,
    layer_def: &LayerDefinition,
    options: &Options,
    face: u8,
    level: i32,
    col: i32,
    row: i32,
) -> Result<Option<tiles3d::Tileset>, StatusCode> {
    let source_level = layer_def.source_s2_content_max_level;
    let Some(tile_level) = options.tile_level else {
        return Ok(None);
    };
    if !(source_level..=tile_level).contains(&level) {
        return Ok(None);
    }

    let cell = cell_id_from_face_level_col_row(face, level, col, row);
    let token = cell
        .parent(layer_def.source_s2_content_package_level as u64)
        .to_token();
    let mut content_root = layer_def.resolve_content_uri_template(&token);
    if content_root.ends_with(".3tz") {
        content_root.push_str("/tileset.json");
    }
    let root = UriAbsoluteStr::new(&content_root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let placements =
        source_placements(app_state, &source_geojson_uri(root, cell, source_level)?).await?;

    let in_cell: Vec<&Placement> = placements.iter().filter(|p| p.in_cell(cell)).collect();
    let tiling = Tiling {
        layer_def,
        tile_level,
        token: &token,
    };
    let root_tile = tiling
        .tile(cell, &in_cell)
        .unwrap_or_else(|| tiles3d::Tile {
            bounding_volume: crate::s2_utils::s2_rect_to_region(
                &s2::cell::Cell::from(cell).rect_bound(),
                MIN_ELEV,
                MAX_ELEV,
            ),
            geometric_error: tiling.branch_geometric_error(),
            refine: None,
            content: None,
            children: vec![],
            root_property: Default::default(),
        });

    counter!("porter_transform_inline_owt_referenced_models_tilesets_total").increment(1);

    Ok(Some(tiles3d::Tileset {
        asset: tiles3d::Asset::default(),
        extensions_required: layer_def.tileset_extensions_required.clone(),
        extensions_used: layer_def.tileset_extensions_used.clone(),
        root_property: Default::default(),
        geometric_error: root_tile.geometric_error,
        root: root_tile,
    }))
}

struct Tiling<'a> {
    layer_def: &'a LayerDefinition,
    tile_level: i32,
    token: &'a str,
}

impl Tiling<'_> {
    /// Tiles between the source level and `tileLevel` refine at the distance the
    /// source tiles' parents did, so models load as far away as they would
    /// without sub-tiling, but only near cells that have them
    fn branch_geometric_error(&self) -> f64 {
        self.layer_def
            .geometric_error_for_level(self.layer_def.source_s2_content_max_level - 1)
    }

    fn tile(&self, cell: CellID, placements: &[&Placement]) -> Option<tiles3d::Tile> {
        if placements.is_empty() {
            return None;
        }
        let (face, level, col, row) = face_level_col_row_from_cell_id(cell);
        let mut tile = tiles3d::Tile {
            bounding_volume: placements_region(placements),
            geometric_error: self.branch_geometric_error(),
            refine: None,
            content: None,
            children: vec![],
            root_property: Default::default(),
        };
        if level >= self.tile_level {
            tile.geometric_error = self
                .layer_def
                .geometric_error_for_level(self.layer_def.source_s2_content_max_level);
            tile.content = Some(tiles3d::Content {
                uri: format!(
                    "../../../../c/{}/{face}/{level}/{col}/{row}.glb",
                    self.token
                ),
                bounding_volume: None,
                root_property: Default::default(),
            });
        } else {
            tile.children = cell
                .children()
                .iter()
                .filter_map(|&child| {
                    let in_child: Vec<&Placement> = placements
                        .iter()
                        .copied()
                        .filter(|p| p.in_cell(child))
                        .collect();
                    self.tile(child, &in_child)
                })
                .collect();
        }
        Some(tile)
    }
}

/// A region around the placements' points, padded for model reach and height
fn placements_region(placements: &[&Placement]) -> tiles3d::BoundingVolume {
    let (mut west, mut south, mut east, mut north) = (180.0f64, 90.0f64, -180.0f64, -90.0f64);
    let (mut low, mut high) = (f64::MAX, f64::MIN);
    let mut all_have_height = true;
    for p in placements {
        west = west.min(p.lon);
        east = east.max(p.lon);
        south = south.min(p.lat);
        north = north.max(p.lat);
        match p.height {
            Some(height) => {
                // Models can sit as far below their point as their height
                // (foundations on slopes)
                low = low.min(p.elev - height);
                high = high.max(p.elev + height);
            }
            None => all_have_height = false,
        }
    }

    let pad_lat = (MODEL_REACH_METERS / EARTH_RADIUS_METERS).to_degrees();
    let widest_lat = south.abs().max(north.abs()).min(89.0).to_radians();
    let pad_lon = pad_lat / widest_lat.cos();
    let (low, high) = if all_have_height {
        (low - HEIGHT_MARGIN_METERS, high + HEIGHT_MARGIN_METERS)
    } else {
        (MIN_ELEV, MAX_ELEV)
    };

    tiles3d::BoundingVolume::from_lat_lng_elev_degrees(
        (west - pad_lon).max(-180.0),
        (south - pad_lat).max(-90.0),
        (east + pad_lon).min(180.0),
        (north + pad_lat).min(90.0),
        low,
        high,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_cells_round_trip() {
        let cell = cell_id_from_face_level_col_row(1, 14, 13652, 13120);
        assert_eq!(content_cell("1/14/13652/13120.glb"), Some(cell));
        assert_eq!(content_cell("1/14/13652/13120.geojson"), None);
        assert_eq!(content_cell("1/14/16384/0.glb"), None);
        assert_eq!(content_cell("6/1/0/0.glb"), None);
        assert_eq!(content_cell("tileset.json"), None);
    }

    #[test]
    fn tile_level_must_be_at_or_below_the_source_level() {
        let options = |tile_level| Options { tile_level };
        assert!(options(None).validate(12).is_ok());
        assert!(options(Some(12)).validate(12).is_ok());
        assert!(options(Some(14)).validate(12).is_ok());
        assert!(options(Some(11)).validate(12).is_err());
        assert!(options(Some(31)).validate(12).is_err());
    }

    fn placement(cell: CellID, elev: f64, height: Option<f64>) -> Placement {
        let center = s2::latlng::LatLng::from(s2::cell::Cell::from(cell).center());
        Placement {
            model_uri: "s3://bucket/model.glb".parse().unwrap(),
            leaf: CellID::from(center),
            lon: center.lng.deg(),
            lat: center.lat.deg(),
            elev,
            heading: 0.0,
            height,
        }
    }

    fn region(tile: &tiles3d::Tile) -> [f64; 6] {
        match tile.bounding_volume.shape {
            tiles3d::BoundingVolumeShape::Region(region) => region,
            _ => panic!("expected a region"),
        }
    }

    #[test]
    fn sub_tiles_cover_only_cells_with_models() {
        let layer_def: LayerDefinition = serde_json::from_value(serde_json::json!({
            "sourceUriContentTemplate": "s3://bucket/{CONTENT_ROOT_TOKEN}.3tz",
            "sourceS2ContentPackageLevel": 7,
            "sourceS2ContentMinLevel": 12,
            "sourceS2ContentMaxLevel": 12,
            "sourceS2ContentExtension": "glb",
            "sourceS2ContentCoverageTokens": ["1"],
            "rootGeometricError": 16384,
            "tilesetRootProperty": {},
            "tilesetExtensionsUsed": [],
            "tilesetExtensionsRequired": [],
            "version": 0,
            "assetId": 0
        }))
        .unwrap();
        let tiling = Tiling {
            layer_def: &layer_def,
            tile_level: 14,
            token: "1",
        };

        // Two models in one L14 cell, one in another L14 cell of a different L13
        let source = cell_id_from_face_level_col_row(1, 12, 3413, 3280);
        let a = cell_id_from_face_level_col_row(1, 14, 13652, 13120);
        let b = cell_id_from_face_level_col_row(1, 14, 13655, 13123);
        let placements = [
            placement(a, 100.0, Some(20.0)),
            placement(a, 110.0, Some(30.0)),
            placement(b, 50.0, Some(10.0)),
        ];
        let refs: Vec<&Placement> = placements.iter().collect();

        let root = tiling.tile(source, &refs).unwrap();
        assert_eq!(root.geometric_error, 8.0);
        assert!(root.content.is_none());
        assert_eq!(root.children.len(), 2);
        let leaves: Vec<_> = root.children.iter().flat_map(|c| &c.children).collect();
        assert_eq!(leaves.len(), 2);
        assert_eq!(
            leaves[0].content.as_ref().unwrap().uri,
            "../../../../c/1/1/14/13652/13120.glb"
        );
        assert_eq!(leaves[0].geometric_error, 4.0);
        assert!(leaves[0].children.is_empty());

        // Heights span each model's point plus and minus its height
        let [.., low, high] = region(leaves[0]);
        assert_eq!((low, high), (100.0 - 20.0 - 10.0, 110.0 + 30.0 + 10.0));
        let [.., low, high] = region(&root);
        assert_eq!((low, high), (50.0 - 10.0 - 10.0, 110.0 + 30.0 + 10.0));

        // Padded horizontally, and contained by the parent
        let [west, south, east, north, ..] = region(leaves[0]);
        let [p_west, p_south, p_east, p_north, ..] = region(&root);
        let point = placements[0].leaf;
        let point = s2::latlng::LatLng::from(point);
        assert!(west < point.lng.rad() && point.lng.rad() < east);
        assert!(south < point.lat.rad() && point.lat.rad() < north);
        assert!(p_west <= west && p_south <= south && east <= p_east && north <= p_north);

        // A model without HGT leaves the full height range
        let no_height = [placement(a, 100.0, None)];
        let refs: Vec<&Placement> = no_height.iter().collect();
        let [.., low, high] = region(&tiling.tile(a, &refs).unwrap());
        assert_eq!((low, high), (MIN_ELEV, MAX_ELEV));

        assert!(tiling.tile(source, &[]).is_none());
    }
}
