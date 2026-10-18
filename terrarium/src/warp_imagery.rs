use super::{ImageryRasterS2, ImageryRasterWmtsSimple};
use crate::pixel_st::PixelStTable;
use crate::warp::interpolate_cubic;

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

/// Sample at continuous pixel coordinates, where pixel (i, j) covers
/// [i, i + 1) x [j, j + 1) and its value sits at its center
pub fn sample_bicubic(source: &ImageryRasterS2, x: f64, y: f64) -> glam::U8Vec3 {
    let w = source.pixel_dims.x;
    let h = source.pixel_dims.y;

    // Interpolate between pixel centers
    let x = x - 0.5;
    let y = y - 0.5;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_pixel_values_at_their_centers() {
        // Red = 10x + 40y, a ramp the cubic reproduces exactly
        let source = ImageryRasterS2 {
            face: 0,
            pixel_level: 0,
            origin_s: 0.0,
            origin_t: 1.0,
            resolution_s: 0.25,
            resolution_t: -0.25,
            pixel_dims: glam::ivec2(4, 4),
            data: (0..16u8)
                .map(|i| glam::u8vec3(10 * (i % 4) + 40 * (i / 4), 0, 255))
                .collect(),
        };
        let red = |x, y| sample_bicubic(&source, x, y).x;
        assert_eq!(red(1.5, 2.5), 90);
        assert_eq!(red(2.0, 2.5), 95);
        assert_eq!(red(1.5, 2.0), 70);
        assert_eq!(red(0.5, 0.5), 0);
        assert_eq!(red(3.5, 3.5), 150);
        assert_eq!(sample_bicubic(&source, 2.0, 2.0).z, 255);
    }
}
