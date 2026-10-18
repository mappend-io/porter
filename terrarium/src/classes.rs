//! Class rasters: one 8-bit class ID per pixel, like land use and land cover.
//! They're warped like imagery, but with nearest sampling since IDs can't be
//! blended, and colored from a palette when encoded.

use anyhow::{Context, Result, bail};
use bytes::{BufMut, Bytes, BytesMut};
use futures::future::join_all;
use tokio::task::spawn_blocking;

use super::{
    MapzenTileIndex, RasterSourceCache, face_level_col_row_from_cell_id, gd_rect_to_s2_coverage,
    mapzen_tile_to_wgs84_envelope, mapzen_zoom_to_s2_level, parse_pam_geotransform,
    resolve_elevation_raster_uri_tokens,
};
use crate::pixel_st::PixelStTable;
use crate::source_cache::present;

/// RGBA color for each class ID. Alpha 0 leaves the class out of the tile.
pub type ClassColors = [[u8; 4]; 256];

pub struct ClassRasterS2 {
    pub face: u8,
    pub pixel_level: i32,
    pub origin_s: f64,
    pub origin_t: f64,
    pub resolution_s: f64,
    pub resolution_t: f64, // Always negative
    pub pixel_dims: glam::IVec2,
    pub data: Vec<u8>,
}

pub struct ClassRasterWmtsSimple {
    pub tile_index: MapzenTileIndex,
    pub origin_x: f64,
    pub origin_y: f64,
    pub resolution_x: f64,
    pub resolution_y: f64, // Always positive
    pub pixel_dims: glam::IVec2,
    pub data: Vec<u8>, // Class IDs, 0 where there is no source
}

pub async fn read_content_raster_s2_from_class_png(
    resource_loader: resource_io::ResourceLoader,
    raster_template_uri: &str,
    s2_package_level: i32,
    token: s2::cellid::CellID,
) -> Result<ClassRasterS2> {
    use iri_string::types::UriAbsoluteStr;
    use std::io::Cursor;

    let package_token = token.parent(s2_package_level as u64);

    let (face, level, col, row) = face_level_col_row_from_cell_id(token);

    tracing::trace!("Loading token: {}", token.to_token());

    let png_resolved_uri = resolve_elevation_raster_uri_tokens(
        raster_template_uri,
        &package_token,
        face,
        level,
        col,
        row,
    );
    let png_uri = UriAbsoluteStr::new(&png_resolved_uri)?;
    let png_bytes = resource_loader
        .read_async(png_uri)
        .await
        .with_context(|| format!("Could not read {png_uri}"))?;

    let sidecar_uri = format!("{png_resolved_uri}.aux.xml");
    let sidecar_uri_abs = UriAbsoluteStr::new(&sidecar_uri)?;
    let sidecar_bytes = resource_loader
        .read_async(sidecar_uri_abs)
        .await
        .with_context(|| format!("Could not read {sidecar_uri_abs}"))?;

    let sidecar_str = std::str::from_utf8(&sidecar_bytes)
        .with_context(|| format!("PNG sidecar at {sidecar_uri} is not valid UTF-8"))?;
    let gt = parse_pam_geotransform(sidecar_str)
        .with_context(|| format!("Could not parse PNG sidecar at {sidecar_uri}"))?;

    // Decode on the blocking pool, a source is millions of pixels
    let png_uri_owned = png_uri.to_string();
    spawn_blocking(move || {
        let decoder = png::Decoder::new(Cursor::new(png_bytes));
        let mut reader = decoder
            .read_info()
            .with_context(|| format!("Could not read PNG header at {png_uri_owned}"))?;

        let info = reader.info();
        let width = info.width;
        let height = info.height;

        // Class IDs are 8-bit grayscale, bail otherwise
        if info.color_type != png::ColorType::Grayscale || info.bit_depth != png::BitDepth::Eight {
            bail!(
                "PNG at {png_uri_owned} is {:?} {:?}, expected 8-bit grayscale",
                info.color_type,
                info.bit_depth,
            );
        }

        let mut data = vec![
            0u8;
            reader
                .output_buffer_size()
                .expect("Output buffer size not set")
        ];
        let frame_info = reader
            .next_frame(&mut data)
            .context("Could not decode PNG frame")?;
        data.truncate(frame_info.buffer_size());

        if data.len() != (width as usize) * (height as usize) {
            bail!(
                "PNG decoded to {} bytes but dimensions are {width}x{height}",
                data.len(),
            );
        }

        let pixel_level_f = -(gt.pixel_width.log2());
        let pixel_level = pixel_level_f.round() as i32;

        Ok(ClassRasterS2 {
            face,
            pixel_level,
            origin_s: gt.origin_x,
            origin_t: gt.origin_y,
            resolution_s: gt.pixel_width,
            resolution_t: gt.pixel_height,
            pixel_dims: glam::ivec2(width as i32, height as i32),
            data,
        })
    })
    .await
    .context("Join error")?
}

// Always 256x256
pub fn make_wmts_simple_class_raster_for_tile(index: &MapzenTileIndex) -> ClassRasterWmtsSimple {
    const TILE_SIZE: i32 = 256;

    // Same tile-grid geotransform as the imagery tiles
    ClassRasterWmtsSimple {
        tile_index: *index,
        origin_x: index.col as f64,
        origin_y: index.row as f64,
        resolution_x: 1.0 / TILE_SIZE as f64,
        resolution_y: 1.0 / TILE_SIZE as f64,
        pixel_dims: glam::ivec2(TILE_SIZE, TILE_SIZE),
        data: vec![0; (TILE_SIZE * TILE_SIZE) as usize],
    }
}

pub fn make_empty_wmts_simple_class_tile(index: &MapzenTileIndex, colors: &ClassColors) -> Bytes {
    let r = make_wmts_simple_class_raster_for_tile(index);
    encode_wmts_simple_class_tile_png(&r, colors)
}

pub fn apply_class_warp_from_source(
    target: &mut ClassRasterWmtsSimple,
    source: &ClassRasterS2,
    target_st: &PixelStTable,
) {
    let src_w = source.pixel_dims.x;
    let src_h = source.pixel_dims.y;

    debug_assert_eq!(target.data.len(), target_st.len());
    debug_assert_eq!(source.data.len(), (src_w * src_h) as usize);
    debug_assert!(source.resolution_s > 0.0);
    debug_assert!(source.resolution_t < 0.0);

    for (i, face, s, t) in target_st.iter() {
        if face != source.face {
            continue;
        }

        let sx_float = (s - source.origin_s) / source.resolution_s;
        let sy_float = (t - source.origin_t) / source.resolution_t;

        if sx_float >= 0.0 && sx_float < src_w as f64 && sy_float >= 0.0 && sy_float < src_h as f64
        {
            // Nearest: the pixel the sample falls in
            target.data[i] = source.data[sy_float as usize * src_w as usize + sx_float as usize];
        }
    }
}

/// Encode as an indexed PNG, with one palette entry per distinct color
pub fn encode_wmts_simple_class_tile_png(
    raster: &ClassRasterWmtsSimple,
    colors: &ClassColors,
) -> Bytes {
    let width = raster.pixel_dims.x as u32;
    let height = raster.pixel_dims.y as u32;

    debug_assert_eq!(
        raster.data.len(),
        (width * height) as usize,
        "Raster data length does not match pixel_dims"
    );

    // At most 256 distinct colors, so the palette always fits
    let mut palette: Vec<[u8; 4]> = vec![];
    let mut index_for_class = [0u8; 256];
    for (class, &color) in colors.iter().enumerate() {
        let color = if color[3] == 0 { [0; 4] } else { color };
        let index = palette.iter().position(|&c| c == color).unwrap_or_else(|| {
            palette.push(color);
            palette.len() - 1
        });
        index_for_class[class] = index as u8;
    }

    let pixels: Vec<u8> = raster
        .data
        .iter()
        .map(|&class| index_for_class[class as usize])
        .collect();

    let mut buf = BytesMut::new().writer();

    // Scoped to make the encoder flush before freeze
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(
            palette
                .iter()
                .flat_map(|c| [c[0], c[1], c[2]])
                .collect::<Vec<_>>(),
        );
        // Alpha for entries up to the last translucent one, the rest are opaque
        if let Some(last) = palette.iter().rposition(|c| c[3] != 255) {
            encoder.set_trns(palette[..=last].iter().map(|c| c[3]).collect::<Vec<_>>());
        }
        // Class IDs come in runs, which filters break up
        encoder.set_filter(png::Filter::NoFilter);
        encoder.set_compression(png::Compression::High);

        let mut writer = encoder
            .write_header()
            .expect("PNG header write should never fail");

        writer
            .write_image_data(&pixels)
            .expect("PNG data write should never fail");
    }

    buf.into_inner().freeze()
}

pub async fn build_simple_wmts_class_tile(
    resource_loader: resource_io::ResourceLoader,
    source_cache: &RasterSourceCache,
    raster_template_uri: &str, // Path to rasters, we replace {CONTENT_ROOT_TOKEN}, {FACE}, {LEVEL}, {COL}, {ROW}
    fallback_raster_template_uri: Option<String>, // Path to rasters, we replace {CONTENT_ROOT_TOKEN}, {FACE}, {LEVEL}, {COL}, {ROW}
    s2_content_package_level: i32,
    index: &MapzenTileIndex,
    colors: &ClassColors,
) -> Result<Bytes> {
    // Zooms without an S2 level have no data
    let Some(s2_raster_level) = mapzen_zoom_to_s2_level(index.zoom) else {
        return Ok(make_empty_wmts_simple_class_tile(index, colors));
    };

    let uri = if s2_raster_level >= s2_content_package_level {
        raster_template_uri.to_string()
    } else if let Some(fallback) = fallback_raster_template_uri {
        fallback
    } else {
        return Ok(make_empty_wmts_simple_class_tile(index, colors));
    };

    let gd_coverage = mapzen_tile_to_wgs84_envelope(index);
    let s2_raster_tokens = gd_rect_to_s2_coverage(&gd_coverage, s2_raster_level);

    // The sources, decoded once and shared by the tiles that overlap them
    let s2_rasters = present(
        join_all(s2_raster_tokens.iter().map(|token| {
            source_cache.classes(
                resource_loader.clone(),
                &uri,
                s2_content_package_level,
                *token,
            )
        }))
        .await,
    )?;

    let index = *index;
    let colors = *colors;
    spawn_blocking(move || {
        let mut output_raster = make_wmts_simple_class_raster_for_tile(&index);

        let target_st = PixelStTable::new(
            index.zoom,
            (output_raster.origin_x, output_raster.origin_y),
            (output_raster.resolution_x, output_raster.resolution_y),
            output_raster.pixel_dims,
        );
        for s2_raster in &s2_rasters {
            apply_class_warp_from_source(&mut output_raster, s2_raster, &target_st);
        }

        Ok(encode_wmts_simple_class_tile_png(&output_raster, &colors))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(png_bytes: &[u8]) -> (Vec<u8>, Vec<u8>, Option<Vec<u8>>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes))
            .read_info()
            .unwrap();
        let info = reader.info();
        let palette = info.palette.as_ref().unwrap().to_vec();
        let trns = info.trns.as_ref().map(|t| t.to_vec());
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixels).unwrap();
        (pixels, palette, trns)
    }

    #[test]
    fn classes_share_palette_entries_by_color() {
        let index = MapzenTileIndex {
            zoom: 12,
            col: 0,
            row: 0,
        };
        let mut raster = make_wmts_simple_class_raster_for_tile(&index);
        raster.data[1] = 10;
        raster.data[2] = 11;
        raster.data[3] = 50;
        raster.data[4] = 99;

        let mut colors: ClassColors = [[0; 4]; 256];
        colors[10] = [0, 100, 0, 255];
        colors[11] = [0, 100, 0, 255];
        colors[50] = [255, 0, 0, 128];
        colors[99] = [1, 2, 3, 0]; // Transparent, whatever the color

        let (pixels, palette, trns) = decode(&encode_wmts_simple_class_tile_png(&raster, &colors));
        assert_eq!(palette, [0, 0, 0, 0, 100, 0, 255, 0, 0]);
        assert_eq!(trns, Some(vec![0, 255, 128]));
        assert_eq!(&pixels[..5], [0, 1, 1, 2, 0]);
    }

    #[test]
    fn opaque_palettes_have_no_transparency() {
        let index = MapzenTileIndex {
            zoom: 12,
            col: 0,
            row: 0,
        };
        let raster = make_wmts_simple_class_raster_for_tile(&index);
        let colors: ClassColors = [[9, 9, 9, 255]; 256];

        let (pixels, palette, trns) = decode(&encode_wmts_simple_class_tile_png(&raster, &colors));
        assert_eq!(palette, [9, 9, 9]);
        assert_eq!(trns, None);
        assert!(pixels.iter().all(|&p| p == 0));
    }
}
