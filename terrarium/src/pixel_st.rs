//! Where each output pixel center of a Web Mercator tile falls in S2 (face, s, t)
//! space. Computed once per output tile and shared by every source raster
//! warped into it.

pub struct PixelStTable {
    faces: Vec<u8>,
    st: Vec<(f64, f64)>,
}

impl PixelStTable {
    /// Build the table for a raster in tile-grid coordinates at `zoom`: origin
    /// is the NW corner in tiles, resolution is tiles per pixel, +y is south.
    pub fn new(
        zoom: i32,
        origin: (f64, f64),
        resolution: (f64, f64),
        pixel_dims: glam::IVec2,
    ) -> Self {
        use std::f64::consts::PI;

        let n = (1i64 << zoom) as f64;
        let (w, h) = (pixel_dims.x as usize, pixel_dims.y as usize);

        // Longitude only depends on the column and latitude only on the row
        let lon_sin_cos: Vec<(f64, f64)> = (0..w)
            .map(|px| {
                let x = origin.0 + (px as f64 + 0.5) * resolution.0;
                (x / n * 2.0 * PI - PI).sin_cos()
            })
            .collect();

        let mut faces = Vec::with_capacity(w * h);
        let mut st = Vec::with_capacity(w * h);
        for py in 0..h {
            let y = origin.1 + (py as f64 + 0.5) * resolution.1;
            let lat = (PI * (1.0 - 2.0 * y / n)).sinh().atan();
            let (sin_lat, cos_lat) = lat.sin_cos();
            for &(sin_lon, cos_lon) in &lon_sin_cos {
                let (face, u, v) = xyz_to_face_uv(cos_lat * cos_lon, cos_lat * sin_lon, sin_lat);
                faces.push(face);
                st.push((uv_to_st(u), uv_to_st(v)));
            }
        }

        Self { faces, st }
    }

    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// (pixel index, face, s, t) for each pixel, in row-major order
    pub fn iter(&self) -> impl Iterator<Item = (usize, u8, f64, f64)> + '_ {
        self.faces
            .iter()
            .zip(&self.st)
            .enumerate()
            .map(|(i, (&face, &(s, t)))| (i, face, s, t))
    }
}

// S2's quadratic UV -> ST projection, as in s2::s2::stuv::uv_to_st
#[inline]
fn uv_to_st(u: f64) -> f64 {
    if u >= 0. {
        0.5 * (1. + 3. * u).sqrt()
    } else {
        1. - 0.5 * (1. - 3. * u).sqrt()
    }
}

// As in s2::s2::stuv::xyz_to_face_uv, without needing a Vector
#[inline]
fn xyz_to_face_uv(x: f64, y: f64, z: f64) -> (u8, f64, f64) {
    let mut face = 0;
    let mut value = x;
    if y.abs() > x.abs() {
        face = 1;
        value = y;
    }
    if z.abs() > value.abs() {
        face = 2;
        value = z;
    }
    if value < 0. {
        face += 3;
    }
    let (u, v) = match face {
        0 => (y / x, z / x),
        1 => (-x / y, z / y),
        2 => (-x / z, -y / z),
        3 => (z / x, y / x),
        4 => (z / y, -x / y),
        _ => (-y / z, -x / z),
    };
    (face, u, v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MapzenTileIndex;

    // The previous per-pixel path: lat/lon -> leaf CellID -> leaf center in st
    fn reference(index: &MapzenTileIndex, px: i32, py: i32, size: i32) -> (u8, f64, f64) {
        use s2::cellid::CellID;
        use s2::latlng::LatLng;
        use std::f64::consts::PI;
        let n = (1i64 << index.zoom) as f64;
        let x = index.col as f64 + (px as f64 + 0.5) / size as f64;
        let y = index.row as f64 + (py as f64 + 0.5) / size as f64;
        let lon = x / n * 360.0 - 180.0;
        let lat = (PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
        let leaf = CellID::from(&LatLng::from_degrees(lat, lon));
        let center = leaf.bound_st().center();
        (leaf.face(), center.x, center.y)
    }

    #[test]
    fn matches_leaf_cell_centers() {
        let size = 64;
        for index in [
            MapzenTileIndex {
                zoom: 2,
                col: 1,
                row: 1,
            },
            MapzenTileIndex {
                zoom: 6,
                col: 53,
                row: 27,
            },
            MapzenTileIndex {
                zoom: 14,
                col: 13681,
                row: 7060,
            },
            MapzenTileIndex {
                zoom: 9,
                col: 3,
                row: 500,
            },
        ] {
            let res = 1.0 / size as f64;
            let table = PixelStTable::new(
                index.zoom,
                (index.col as f64, index.row as f64),
                (res, res),
                glam::ivec2(size, size),
            );
            assert_eq!(table.len(), (size * size) as usize);
            for (i, face, s, t) in table.iter() {
                let (px, py) = (i as i32 % size, i as i32 / size);
                let (ref_face, ref_s, ref_t) = reference(&index, px, py, size);
                assert_eq!(face, ref_face, "{index:?} ({px}, {py})");
                // Leaf cells are 2^-30 wide in st, the reference snaps to their center
                assert!((s - ref_s).abs() < 1e-8, "{index:?} ({px}, {py}) s");
                assert!((t - ref_t).abs() < 1e-8, "{index:?} ({px}, {py}) t");
            }
        }
    }
}
