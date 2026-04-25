use crate::tiles3d;
use anyhow::Result;
use glam::{DMat4, DVec3};
use iri_string::types::{UriAbsoluteStr, UriAbsoluteString, UriReferenceStr};
use resource_io::ResourceLoader;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileExtMaxarGrid {
    bounding_box: [f64; 6],
    face: i32,
    index: [i32; 2],
    level: i32,
}

fn uri_relative_to_base_dir(resolved: &str, base_uri: &UriAbsoluteStr) -> String {
    let base_str = base_uri.as_str();
    let base_dir = base_str
        .rfind('/')
        .map(|i| &base_str[..=i])
        .unwrap_or(base_str);

    let base_segments: Vec<&str> = base_dir.split('/').collect();
    let resolved_segments: Vec<&str> = resolved.split('/').collect();

    // Find where base_dir and resolved diverge
    let diverge_at = base_segments
        .iter()
        .zip(resolved_segments.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // The relative path is everything in resolved from the divergence point
    resolved_segments[diverge_at..].join("/")
}

fn rewrite_tile_uris_relative_to_root(
    tile: &mut tiles3d::Tile,
    tile_base_uri: &UriAbsoluteStr,
    root_tileset_uri: &UriAbsoluteStr,
) -> Result<()> {
    if let Some(content) = &mut tile.content {
        // Resolve relative to where the tile is actually located
        let relative = UriReferenceStr::new(&content.uri)?;
        let resolved = relative.resolve_against(tile_base_uri).to_string();

        // Make it relative to the root tileset's base directory
        content.uri = uri_relative_to_base_dir(&resolved, root_tileset_uri);
    }

    for child in &mut tile.children {
        rewrite_tile_uris_relative_to_root(child, tile_base_uri, root_tileset_uri)?;
    }

    Ok(())
}

pub async fn get_content_root_tileset(
    resource_loader: ResourceLoader,
    root_tileset_uri: &UriAbsoluteStr,
    content_level: i32,
) -> Result<tiles3d::Tileset> {
    let bytes = resource_loader.read_async(root_tileset_uri).await?;
    let root_tileset = serde_json::from_slice::<tiles3d::Tileset>(&bytes)?;

    // We are seeking out a single tiles3d::Tile that has the MAXAR_grid extension on it with a level value of content_level.
    // We will then slurp out that one tile and make it the root of a new tileset and return it.
    // We probably have to recurse into external tilesets.

    let (mut tile, tile_base_uri) = find_tile_at_level(
        resource_loader.clone(),
        root_tileset_uri,
        &root_tileset.root,
        content_level,
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("No tile found at level {content_level}"))?;

    // TODO: Go through any URIs in root and rewrite them based on where we slurped this from.
    // Since this is being mapped to /<tileset_id>/<some_hash>/content/top, if we slurped it from xyz.3tz/4/4/12/34.json, it might reference
    // children as ../../8/123/456.json. But that won't work if we serve this from top.
    // So we somehow need to return the uri we pulled the data from in find_tile_at_level, and turn a ../../8/123/456.json to 8/123/456.json.
    rewrite_tile_uris_relative_to_root(&mut tile, &tile_base_uri, root_tileset_uri)?;

    let tileset = tiles3d::Tileset {
        root: tile,
        ..root_tileset
    };

    Ok(tileset)
}

#[async_recursion::async_recursion]
async fn find_tile_at_level(
    resource_loader: ResourceLoader,
    base_uri: &UriAbsoluteStr,
    tile: &tiles3d::Tile,
    content_level: i32,
) -> Result<Option<(tiles3d::Tile, UriAbsoluteString)>> {
    // Check if this tile has the MAXAR_grid extension at the right level
    if let Some(ext) = tile
        .root_property
        .get_extension::<TileExtMaxarGrid>("MAXAR_grid")
        && ext.level == content_level
    {
        return Ok(Some((tile.clone(), base_uri.to_owned())));
    }

    if let Some(content) = &tile.content
        && content.uri.ends_with(".json")
    {
        let relative = UriReferenceStr::new(&content.uri)?;
        let resolved_external_uri = relative.resolve_against(base_uri).to_string();
        let external_uri = UriAbsoluteStr::new(&resolved_external_uri)?;
        let bytes = resource_loader.read_async(external_uri).await?;
        let external_tileset = serde_json::from_slice::<tiles3d::Tileset>(&bytes)?;
        if let Some(found) = find_tile_at_level(
            resource_loader.clone(),
            external_uri,
            &external_tileset.root,
            content_level,
        )
        .await?
        {
            return Ok(Some(found));
        }
    }

    for child in &tile.children {
        if let Some(found) =
            find_tile_at_level(resource_loader.clone(), base_uri, child, content_level).await?
        {
            return Ok(Some(found));
        }
    }

    Ok(None)
}

pub fn sniff_content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"glTF") {
        return "model/gltf-binary";
    }
    let trimmed = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(0);
    if bytes.get(trimmed) == Some(&b'{') {
        return "application/json";
    }
    "application/octet-stream"
}

// WGS84 ellipsoid parameters
const WGS84_A: f64 = 6_378_137.0;
const WGS84_F: f64 = 1.0 / 298.257_223_563;
const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F);

fn geodetic_to_ecef(lon_deg: f64, lat_deg: f64, height_m: f64) -> DVec3 {
    let lon = lon_deg.to_radians();
    let lat = lat_deg.to_radians();

    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();

    let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();

    DVec3::new(
        (n + height_m) * cos_lat * cos_lon,
        (n + height_m) * cos_lat * sin_lon,
        (n * (1.0 - WGS84_E2) + height_m) * sin_lat,
    )
}

/// Return (east, north, up) orthonormal basis vectors at point
/// TODO: Add elevation to shift enu origin
fn enu_basis(lon_deg: f64, lat_deg: f64) -> (DVec3, DVec3, DVec3) {
    let lon = lon_deg.to_radians();
    let lat = lat_deg.to_radians();
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();

    let east = DVec3::new(-sin_lon, cos_lon, 0.0);
    let north = DVec3::new(-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat);
    let up = DVec3::new(cos_lat * cos_lon, cos_lat * sin_lon, sin_lat);

    (east, north, up)
}

pub fn local_to_ecef(lon_deg: f64, lat_deg: f64, height_m: f64) -> DMat4 {
    let origin = geodetic_to_ecef(lon_deg, lat_deg, height_m);
    let (east, north, up) = enu_basis(lon_deg, lat_deg);

    // Columns: local X-axis, local Y-axis, local Z-axis, translation.
    DMat4::from_cols(
        east.extend(0.0),     // local +X → East
        up.extend(0.0),       // local +Y → Up
        (-north).extend(0.0), // local +Z → South (so -Z is North = "forward" in glTF)
        origin.extend(1.0),   // translation to ECEF position
    )
}

pub fn local_to_ecef_with_rotation(
    lon_deg: f64,
    lat_deg: f64,
    elev_m: f64,
    _rot_x_deg: f64,
    rot_y_deg: f64,
    _rot_z_deg: f64,
) -> DMat4 {
    let placement = local_to_ecef(lon_deg, lat_deg, elev_m);
    // TODO: Make sure caller is mapping AOO to this rotation properly
    let heading = DMat4::from_rotation_y(-rot_y_deg.to_radians());
    placement * heading
}
