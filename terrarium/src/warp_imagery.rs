use super::*;
use crate::pixel_st::PixelStTable;

/// Table of pixel centers in S2 st space for `target`, to pass to
/// [`apply_imagery_warp_from_source`] for each source raster
pub fn imagery_pixel_st_table(target: &ImageryRasterWmtsSimple) -> PixelStTable {
    PixelStTable::new(
        target.tile_index.zoom,
        (target.origin_x, target.origin_y),
        (target.resolution_x, target.resolution_y),
        target.pixel_dims,
    )
}

pub fn apply_imagery_warp_from_source(
    target: &mut ImageryRasterWmtsSimple,
    source: &ImageryRasterS2,
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

pub fn sample_bicubic(source: &ImageryRasterS2, x: f64, y: f64) -> glam::U8Vec3 {
    let w = source.pixel_dims.x;
    let h = source.pixel_dims.y;
    let x_int = x.floor() as i32;
    let y_int = y.floor() as i32;
    let dx = (x - x_int as f64) as f32;
    let dy = (y - y_int as f64) as f32;

    let get_pixel = |px: i32, py: i32| -> glam::U8Vec3 {
        let clamped_x = px.clamp(0, w - 1) as usize;
        let clamped_y = py.clamp(0, h - 1) as usize;
        source.data[clamped_y * w as usize + clamped_x]
    };

    let mut col_results = [[0.0f32; 3]; 4];
    for (i, row_result) in col_results.iter_mut().enumerate() {
        let py = y_int - 1 + i as i32;
        let p0 = get_pixel(x_int - 1, py);
        let p1 = get_pixel(x_int, py);
        let p2 = get_pixel(x_int + 1, py);
        let p3 = get_pixel(x_int + 2, py);
        for ch in 0..3 {
            row_result[ch] = interpolate_cubic(
                p0[ch] as f32,
                p1[ch] as f32,
                p2[ch] as f32,
                p3[ch] as f32,
                dx,
            );
        }
    }

    let mut rgb = [0u8; 3];
    for ch in 0..3 {
        let v = interpolate_cubic(
            col_results[0][ch],
            col_results[1][ch],
            col_results[2][ch],
            col_results[3][ch],
            dy,
        );
        rgb[ch] = v.round().clamp(0.0, 255.0) as u8;
    }

    glam::u8vec3(rgb[0], rgb[1], rgb[2])
}
