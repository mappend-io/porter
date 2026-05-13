use super::*;

pub fn apply_elevation_warp_from_source(
    target: &mut ElevationRasterMapzen,
    source: &ElevationRasterS2,
) {
    use s2::cellid::CellID;
    use s2::latlng::LatLng;

    let tgt_w = target.pixel_dims.x;
    let tgt_h = target.pixel_dims.y;
    let src_w = source.pixel_dims.x;
    let src_h = source.pixel_dims.y;

    debug_assert_eq!(target.data.len(), (tgt_w * tgt_h) as usize);
    debug_assert_eq!(source.data.len(), (src_w * src_h) as usize);

    // resolution_t is negative; the source raster's row 0 is at the largest t
    // (north edge of the face). So source pixel row r corresponds to cell j
    // index j_origin - r (j decreases as r increases). Sanity check below.
    debug_assert!(source.resolution_s > 0.0);
    debug_assert!(source.resolution_t < 0.0);

    // TODO: Scrub this to make sure sample locations are perfect in mapzen and s2 st
    for ty in 0..tgt_h {
        for tx in 0..tgt_w {
            let (lat, lon) = mapzen_pixel_to_latlon(target, tx, ty);

            let ll = LatLng::from_degrees(lat, lon);
            let leaf = CellID::from(&ll);
            let leaf_center_st = leaf.bound_st().center();
            let s = leaf_center_st.x;
            let t = leaf_center_st.y;

            let sx_float = (s - source.origin_s) / source.resolution_s;
            let sy_float = (t - source.origin_t) / source.resolution_t;

            if sx_float >= 0.0
                && sx_float < src_w as f64
                && sy_float >= 0.0
                && sy_float < src_h as f64
            {
                let result_rgb = sample_bicubic(source, sx_float, sy_float);
                target.data[(ty * tgt_w + tx) as usize] = result_rgb;
            }
        }
    }
}

fn mapzen_pixel_to_latlon(raster: &ElevationRasterMapzen, px: i32, py: i32) -> (f64, f64) {
    use std::f64::consts::PI;
    let x = raster.origin_x + (px as f64 + 0.5) * raster.resolution_x;
    let y = raster.origin_y + (py as f64 + 0.5) * raster.resolution_y;

    let n = (1i64 << raster.tile_index.zoom) as f64;
    let lon = x / n * 360.0 - 180.0;
    let lat = (PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
    (lat, lon)
}

/// Catmull-Rom cubic interpolation kernel
///
/// v0, v1, v2, v3 are the four points, f is the fraction [0, 1] between v1 and v2
#[inline]
fn interpolate_cubic(v0: f32, v1: f32, v2: f32, v3: f32, f: f32) -> f32 {
    let f2 = f * f;
    let f3 = f2 * f;

    // Catmull-Rom coefficients
    let a = -0.5 * v0 + 1.5 * v1 - 1.5 * v2 + 0.5 * v3;
    let b = v0 - 2.5 * v1 + 2.0 * v2 - 0.5 * v3;
    let c = -0.5 * v0 + 0.5 * v2;
    let d = v1;

    a * f3 + b * f2 + c * f + d
}

// TODO: When we loda the source rgb, turn it into f32, so we don't keep decoding
// over and over
pub fn sample_bicubic(source: &ElevationRasterS2, x: f64, y: f64) -> glam::U8Vec3 {
    let w = source.pixel_dims.x as i32;
    let h = source.pixel_dims.y as i32;

    let x_int = x.floor() as i32;
    let y_int = y.floor() as i32;
    let dx = (x - x_int as f64) as f32;
    let dy = (y - y_int as f64) as f32;

    // Helper to get decoded elevation
    let get_elev = |px: i32, py: i32| -> f32 {
        let clamped_x = px.clamp(0, w - 1) as usize;
        let clamped_y = py.clamp(0, h - 1) as usize;
        decode_elevation_from_mapzen_rgb(source.data[clamped_y * w as usize + clamped_x])
    };

    // Bicubic needs a 4x4 grid, interpolate each of the 4 rows horizontally first
    let mut col_results = [0.0f32; 4];
    for i in 0..4 {
        let py = y_int - 1 + i as i32;
        col_results[i] = interpolate_cubic(
            get_elev(x_int - 1, py),
            get_elev(x_int, py),
            get_elev(x_int + 1, py),
            get_elev(x_int + 2, py),
            dx,
        );
    }

    // Then interpolate the 4 row results vertically
    let final_elev = interpolate_cubic(
        col_results[0],
        col_results[1],
        col_results[2],
        col_results[3],
        dy,
    );

    encode_elevation_to_mapzen_rgb(final_elev)
}
