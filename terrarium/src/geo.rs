#[derive(Copy, Clone, Debug)]
pub struct MapzenTileIndex {
    /// First zoom level is 0
    pub zoom: i32,

    /// 0 on left, increasing toward east
    pub col: i32,

    /// 0 on top, increasing toward south
    pub row: i32,
}

#[derive(Copy, Clone, Debug)]
pub struct Wgs84Coord2d {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Copy, Clone, Debug)]
pub struct Wgs84Rect2d {
    pub south_west: Wgs84Coord2d,
    pub north_east: Wgs84Coord2d,
}

// Adapted from: https://wiki.openstreetmap.org/wiki/Slippy_map_tilenames
pub fn mapzen_tile_to_wgs84_envelope(index: &MapzenTileIndex) -> Wgs84Rect2d {
    use std::f64::consts::PI;

    debug_assert!(index.zoom >= 0);
    debug_assert!(index.col >= 0 && index.row >= 0);
    debug_assert!(
        index.col < (1 << index.zoom) && index.row < (1 << index.zoom),
        "Tile ({}, {}) out of range for zoom {}",
        index.col,
        index.row,
        index.zoom,
    );

    let n = (1i64 << index.zoom) as f64;
    let col = index.col as f64;
    let row = index.row as f64;

    // West/east longitudes are linear in x
    let west_lon = col / n * 360.0 - 180.0;
    let east_lon = (col + 1.0) / n * 360.0 - 180.0;

    // North/south latitudes via inverse Mercator, remember Y increases southward
    let lat_at = |y: f64| (PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
    let north_lat = lat_at(row);
    let south_lat = lat_at(row + 1.0);

    Wgs84Rect2d {
        south_west: Wgs84Coord2d {
            lat: south_lat,
            lon: west_lon,
        },
        north_east: Wgs84Coord2d {
            lat: north_lat,
            lon: east_lon,
        },
    }
}

pub fn gd_rect_to_s2_coverage(gd_rect: &Wgs84Rect2d, s2_level: i32) -> Vec<s2::cellid::CellID> {
    use s2::rect::Rect;
    use s2::region::RegionCoverer;

    let region = Rect::from_degrees(
        gd_rect.south_west.lat,
        gd_rect.south_west.lon,
        gd_rect.north_east.lat,
        gd_rect.north_east.lon,
    );

    let coverer = RegionCoverer {
        min_level: s2_level as u8,
        max_level: s2_level as u8,
        level_mod: 1,
        max_cells: 1024,
    };

    let mut cell_union = coverer.covering(&region);
    cell_union.denormalize(s2_level as u64, 1);
    cell_union.0
}

// TODO: Duplicated from s2_utils
pub fn face_level_col_row_from_cell_id(cell_id: s2::cellid::CellID) -> (u8, i32, i32, i32) {
    let face = cell_id.face();
    let level = cell_id.level();

    let (_, i, j, _orientation) = cell_id.face_ij_orientation();
    let col = (i as u64) >> (30 - level);
    let row = (j as u64) >> (30 - level);

    (face, level as i32, col as i32, row as i32)
}

pub fn hae_to_msl(val: f32) -> f32 {
    // TODO
    val
}
