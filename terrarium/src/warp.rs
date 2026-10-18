use super::{ElevationRasterMapzen, ElevationRasterS2, encode_elevation_to_mapzen_rgb};
use crate::pixel_st::PixelStTable;

/// Table of pixel centers in S2 st space for `target`, to pass to
/// [`apply_elevation_warp_from_source`] for each source raster
pub fn elevation_pixel_st_table(target: &ElevationRasterMapzen) -> PixelStTable {
    PixelStTable::new(
        target.tile_index.zoom,
        (target.origin_x, target.origin_y),
        (target.resolution_x, target.resolution_y),
        target.pixel_dims,
    )
}

pub fn apply_elevation_warp_from_source(
    target: &mut ElevationRasterMapzen,
    source: &ElevationRasterS2,
    target_st: &PixelStTable,
) {
    let src_w = source.pixel_dims.x;
    let src_h = source.pixel_dims.y;

    debug_assert_eq!(target.data.len(), target_st.len());
    debug_assert_eq!(source.data.len(), (src_w * src_h) as usize);

    // resolution_t is negative; the source raster's row 0 is at the largest t
    // (north edge of the face). So source pixel row r corresponds to cell j
    // index j_origin - r (j decreases as r increases). Sanity check below.
    debug_assert!(source.resolution_s > 0.0);
    debug_assert!(source.resolution_t < 0.0);

    // TODO: Scrub this to make sure sample locations are perfect in mapzen and s2 st
    for (i, face, s, t) in target_st.iter() {
        if face != source.face {
            continue;
        }

        let sx_float = (s - source.origin_s) / source.resolution_s;
        let sy_float = (t - source.origin_t) / source.resolution_t;

        if sx_float >= 0.0 && sx_float < src_w as f64 && sy_float >= 0.0 && sy_float < src_h as f64
        {
            target.data[i] = sample_bicubic(source, sx_float, sy_float);
        }
    }
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

/// Sample at continuous pixel coordinates, where pixel (i, j) covers
/// [i, i + 1) x [j, j + 1) and its value sits at its center
pub fn sample_bicubic(source: &ElevationRasterS2, x: f64, y: f64) -> glam::U8Vec3 {
    let w = source.pixel_dims.x;
    let h = source.pixel_dims.y;

    // Interpolate between pixel centers
    let x = x - 0.5;
    let y = y - 0.5;
    let x_int = x.floor() as i32;
    let y_int = y.floor() as i32;
    let dx = (x - x_int as f64) as f32;
    let dy = (y - y_int as f64) as f32;

    // Helper to get decoded elevation
    let get_elev = |px: i32, py: i32| -> f32 {
        let clamped_x = px.clamp(0, w - 1) as usize;
        let clamped_y = py.clamp(0, h - 1) as usize;
        source.data[clamped_y * w as usize + clamped_x]
    };

    // Bicubic needs a 4x4 grid, interpolate each of the 4 rows horizontally first
    let mut col_results = [0.0f32; 4];
    for (i, row_result) in col_results.iter_mut().enumerate() {
        let py = y_int - 1 + i as i32;
        *row_result = interpolate_cubic(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_elevation_from_mapzen_rgb;

    #[test]
    fn samples_pixel_values_at_their_centers() {
        // Elevation = 10x + 40y, a ramp the cubic reproduces exactly
        let source = ElevationRasterS2 {
            face: 0,
            pixel_level: 0,
            origin_s: 0.0,
            origin_t: 1.0,
            resolution_s: 0.25,
            resolution_t: -0.25,
            pixel_dims: glam::ivec2(4, 4),
            data: (0..16)
                .map(|i| (10 * (i % 4) + 40 * (i / 4)) as f32)
                .collect(),
        };
        let sample = |x, y| decode_elevation_from_mapzen_rgb(sample_bicubic(&source, x, y));
        assert_eq!(sample(1.5, 2.5), 90.0);
        assert_eq!(sample(2.0, 2.5), 95.0);
        assert_eq!(sample(1.5, 2.0), 70.0);
        // Edges clamp to the outermost pixels
        assert_eq!(sample(0.5, 0.5), 0.0);
        assert_eq!(sample(3.5, 3.5), 150.0);
    }
}
