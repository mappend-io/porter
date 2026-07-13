use anyhow::{Context, Result, bail};
use bytes::{BufMut, Bytes, BytesMut};
use futures::future::join_all;
use tokio::task::spawn_blocking;
use warp::*;

pub use geo::*;
pub use utils::*;

mod geo;
mod utils;
mod warp;
mod warp_imagery;

pub struct ImageryRasterS2 {
    pub face: u8,
    pub pixel_level: i32,
    pub origin_s: f64,
    pub origin_t: f64,
    pub resolution_s: f64,
    pub resolution_t: f64, // Always negative
    pub pixel_dims: glam::IVec2,
    pub data: Vec<glam::U8Vec3>,
}

pub struct ImageryRasterWmtsSimple {
    pub tile_index: MapzenTileIndex,
    pub origin_x: f64,     // TODO: Clarify what this means, exactly
    pub origin_y: f64,     // TODO: Clarify what this means, exactly
    pub resolution_x: f64, // TODO: Clarify what this means, exactly
    pub resolution_y: f64, // Always positive
    pub pixel_dims: glam::IVec2,
    pub data: Vec<glam::U8Vec3>, // RGB encoded
}

pub struct ElevationRasterS2 {
    pub face: u8,
    pub pixel_level: i32,
    pub origin_s: f64,
    pub origin_t: f64,
    pub resolution_s: f64,
    pub resolution_t: f64, // Always negative
    pub pixel_dims: glam::IVec2,
    pub data: Vec<f32>,
}

pub struct ElevationRasterMapzen {
    pub tile_index: MapzenTileIndex,
    pub origin_x: f64,     // TODO: Clarify what this means, exactly
    pub origin_y: f64,     // TODO: Clarify what this means, exactly
    pub resolution_x: f64, // TODO: Clarify what this means, exactly
    pub resolution_y: f64, // Always positive
    pub pixel_dims: glam::IVec2,
    pub data: Vec<glam::U8Vec3>, // RGB encoded
}

pub fn mapzen_zoom_to_s2_level(zoom: i32) -> Option<i32> {
    // zoom of 3 => S2 L0
    // zoom of 4 => S2 L0
    // zoom of 5 => S2 L0
    // zoom of 6 => S2 L1
    // zoom of 7 => S2 L2
    // zoom of 8 => S2 L3
    // zoom of 9 => S2 L4
    // zoom of 10 => S2 L5
    // zoom of 11 => S2 L6
    // zoom of 12 => S2 L7
    // and so on
    //
    // TODO: Now that I'm rescaling by 0.5, I think we go 12 -> 6, etc.
    // I think we'll need to be told how to map from one to the other.
    // Or maybe "what level houses 1m data", then we can work from there.
    // We'll probably need to know how deep the layer can go.
    let level = (zoom - 4).max(0);
    if (0..=30).contains(&level) {
        Some(level)
    } else {
        None
    }
}

pub fn resolve_elevation_raster_uri_tokens(
    template: &str,
    package_token: &s2::cellid::CellID,
    face: u8,
    level: i32,
    col: i32,
    row: i32,
) -> String {
    let package_token_str = package_token.to_token();
    template
        .replace("{CONTENT_ROOT_TOKEN}", &package_token_str)
        .replace("{FACE}", &face.to_string())
        .replace("{LEVEL}", &level.to_string())
        .replace("{COL}", &col.to_string())
        .replace("{ROW}", &row.to_string())
}

pub async fn read_content_raster_s2_from_lerc_zstd_tif(
    resource_loader: resource_io::ResourceLoader,
    raster_template_uri: &str,
    s2_package_level: i32,
    token: s2::cellid::CellID,
) -> Result<ElevationRasterS2> {
    use iri_string::types::UriAbsoluteStr;

    let package_token = token.parent(s2_package_level as u64);

    let (face, level, col, row) = face_level_col_row_from_cell_id(token);

    tracing::trace!("Loading token: {}", token.to_token());

    let tif_resolved_uri = resolve_elevation_raster_uri_tokens(
        raster_template_uri,
        &package_token,
        face,
        level,
        col,
        row,
    );
    let tif_uri = UriAbsoluteStr::new(&tif_resolved_uri)?;
    let tif_bytes = resource_loader
        .read_async(tif_uri)
        .await
        .with_context(|| format!("Could not read {tif_uri}"))?;

    let sidecar_uri = format!("{tif_resolved_uri}.aux.xml");
    let sidecar_uri_abs = UriAbsoluteStr::new(&sidecar_uri)?;
    let sidecar_bytes = resource_loader
        .read_async(sidecar_uri_abs)
        .await
        .with_context(|| format!("Could not read {sidecar_uri_abs}"))?;

    // Make a string from the aux xml sidecar
    let sidecar_str = std::str::from_utf8(&sidecar_bytes)
        .with_context(|| format!("TIF sidecar at {sidecar_uri} is not valid UTF-8"))?;
    let gt = parse_pam_geotransform(sidecar_str)
        .with_context(|| format!("Could not parse tif sidecar at {sidecar_uri}"))?;

    let reader = tiff_reader::TiffFile::from_bytes(tif_bytes.to_vec())
        .with_context(|| format!("Could not open TIFF at {tif_uri}"))?;

    let ifd = reader
        .ifd(0)
        .with_context(|| format!("Could not get tif reader ifd with {tif_uri}"))?;

    let width = ifd.width();
    let height = ifd.height();

    spawn_blocking(move || {
        let (data, _offset) = reader
            .read_image(0)
            .context("Could not decode tif")?
            .into_raw_vec_and_offset();

        let pixel_level_f = -(gt.pixel_width.log2());
        let pixel_level = pixel_level_f.round() as i32;

        Ok(ElevationRasterS2 {
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

pub async fn read_content_raster_s2_from_terrarium_rgb_png(
    resource_loader: resource_io::ResourceLoader,
    raster_template_uri: &str,
    s2_package_level: i32,
    token: s2::cellid::CellID,
) -> Result<ElevationRasterS2> {
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

    // Make a string from the aux xml sidecar
    let sidecar_str = std::str::from_utf8(&sidecar_bytes)
        .with_context(|| format!("TIF sidecar at {sidecar_uri} is not valid UTF-8"))?;
    let gt = parse_pam_geotransform(sidecar_str)
        .with_context(|| format!("Could not parse tif sidecar at {sidecar_uri}"))?;

    // Decode the png, no copy
    let decoder = png::Decoder::new(Cursor::new(png_bytes));
    let mut reader = decoder
        .read_info()
        .with_context(|| format!("Could not read PNG header at {png_uri}"))?;

    let info = reader.info();
    let width = info.width;
    let height = info.height;

    // We expect 8bpp RGB encoded elevations, bail otherwise
    if info.color_type != png::ColorType::Rgb {
        bail!(
            "PNG at {png_uri} has color type {:?}, expected RGB",
            info.color_type,
        );
    }
    if info.bit_depth != png::BitDepth::Eight {
        bail!(
            "PNG at {png_uri} has bit depth {:?}, expected 8",
            info.bit_depth,
        );
    }

    spawn_blocking(move || {
        // Allocate the output buffer the decoder expects and read one frame.
        let mut raw = vec![
            0u8;
            reader
                .output_buffer_size()
                .expect("Output buffer size not set")
        ];
        let frame_info = reader
            .next_frame(&mut raw)
            .context("Could not decode PNG frame")?;

        // next_frame may report a smaller used length than the buffer (e.g. if the
        // image is interlaced or the buffer was over-allocated); trim to it.
        let raw = &raw[..frame_info.buffer_size()];

        let expected_bytes = (width as usize) * (height as usize) * 3;
        if raw.len() != expected_bytes {
            bail!(
                "PNG decoded to {} bytes but dimensions {width}x{height} RGB = {expected_bytes}",
                raw.len(),
            );
        }

        // Decode the flat RGB byte buffer from mapzfen rgb to f32
        let data: Vec<f32> = raw
            .chunks_exact(3)
            .map(|rgb| {
                utils::decode_elevation_from_mapzen_rgb(glam::u8vec3(rgb[0], rgb[1], rgb[2]))
            })
            .collect();

        let pixel_level_f = -(gt.pixel_width.log2());
        let pixel_level = pixel_level_f.round() as i32;

        Ok(ElevationRasterS2 {
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
pub fn make_mapzen_elevation_raster_for_tile(index: &MapzenTileIndex) -> ElevationRasterMapzen {
    const TILE_SIZE: i32 = 256;

    // Geotransform in tile-grid coordinates at this zoom.
    // Origin is the NW corner of the tile (top-left pixel's top-left corner,
    // GDAL convention). Pixel resolution is 1 tile / 256 pixels = 1/256.
    // Both resolutions positive because we treat +y as southward, matching
    // the tile-grid convention.
    let origin_x = index.col as f64;
    let origin_y = index.row as f64;
    let resolution_x = 1.0 / TILE_SIZE as f64;
    let resolution_y = 1.0 / TILE_SIZE as f64;

    ElevationRasterMapzen {
        tile_index: *index,
        origin_x,
        origin_y,
        resolution_x,
        resolution_y,
        pixel_dims: glam::ivec2(TILE_SIZE, TILE_SIZE),
        // TODO: Maybe want to make this an MSL/geoid-relative value
        data: vec![encode_elevation_to_mapzen_rgb(0.0); (TILE_SIZE * TILE_SIZE) as usize],
    }
}

pub fn encode_mapzen_elevation_tile_png(raster: &ElevationRasterMapzen) -> Bytes {
    let width = raster.pixel_dims.x as u32;
    let height = raster.pixel_dims.y as u32;

    debug_assert_eq!(
        raster.data.len(),
        (width * height) as usize,
        "Raster data length does not match pixel_dims"
    );

    // Reinterpret Vec<U8Vec3> to &[u8]
    let pixel_bytes: &[u8] = bytemuck::cast_slice(&raster.data);

    let mut buf = BytesMut::new().writer();

    // Scoped to make the encoder flush before freeze
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast); // TODO: configurable?

        let mut writer = encoder
            .write_header()
            .expect("PNG header write should never fail");

        writer
            .write_image_data(pixel_bytes)
            .expect("PNG data write should never fail");
    }

    buf.into_inner().freeze()
}

pub fn make_empty_tile(index: &MapzenTileIndex) -> Bytes {
    let r = make_mapzen_elevation_raster_for_tile(index);
    encode_mapzen_elevation_tile_png(&r)
}

// Questions:
// - TODO: Lookup whether mapzen tiles are bathymetry in water or hydro surface
// - TODO: Lookup whether mapzen elevations are hae or msl
pub async fn build_terrarium_rgb_tile(
    resource_loader: resource_io::ResourceLoader,
    raster_template_uri: &str, // Path to rasters, we replace {CONTENT_ROOT_TOKEN}, {FACE}, {LEVEL}, {COL}, {ROW}
    fallback_raster_template_uri: Option<String>,
    s2_content_package_level: i32,
    index: &MapzenTileIndex,
) -> Result<Bytes> {
    tracing::trace!("Index {index:?}");

    // Build GD envelope
    let gd_coverage = mapzen_tile_to_wgs84_envelope(index);
    tracing::trace!("Coverage {gd_coverage:?}");

    // Find corresponding S2 raster level
    let s2_raster_level = mapzen_zoom_to_s2_level(index.zoom)
        .with_context(|| format!("Could not map zoom level {} to S2 level", index.zoom))?;
    tracing::trace!(
        "Mapzen level {} maps to s2 raster level {}",
        index.zoom,
        s2_raster_level
    );

    if s2_raster_level < s2_content_package_level && fallback_raster_template_uri.is_none() {
        // Maybe just make an HAE=0 version and return it?
        bail!("Nothing to do: no fallback raster")
    }

    // Find S2 coverage at S2 raster level
    let s2_raster_tokens = gd_rect_to_s2_coverage(&gd_coverage, s2_raster_level);
    tracing::trace!(
        "Found these s2 tokens at level {}: {:?}",
        s2_raster_level,
        s2_raster_tokens
    );

    // FUTURE OPTIMIZATION: Sort S2 raster tokens by S2 content token bins, just
    // to optimize cache locality

    let uri = if s2_raster_level < s2_content_package_level {
        fallback_raster_template_uri.unwrap()
    } else {
        raster_template_uri.to_string()
    };

    // For each content S2 package, read the S2 raster token files
    use futures::future::FutureExt;
    let s2_rasters: Vec<_> = join_all(s2_raster_tokens.iter().map(|token| {
        if raster_template_uri.ends_with(".png") {
            read_content_raster_s2_from_terrarium_rgb_png(
                resource_loader.clone(),
                &uri,
                s2_content_package_level,
                *token,
            )
            .boxed()
        } else {
            read_content_raster_s2_from_lerc_zstd_tif(
                resource_loader.clone(),
                &uri,
                s2_content_package_level,
                *token,
            )
            .boxed()
        }
    }))
    .await
    .into_iter()
    // It's ok that these fail sometimes, we might not have entire world coverage of input data
    .filter_map(|r| r.ok())
    .collect();

    // Make output image, our warp target
    let index = *index;
    spawn_blocking(move || {
        let mut output_raster = make_mapzen_elevation_raster_for_tile(&index);
        output_raster.data.fill(encode_elevation_to_mapzen_rgb(0.0));

        // Perform warp
        for s2_raster in &s2_rasters {
            apply_elevation_warp_from_source(&mut output_raster, s2_raster);
        }

        // Encode image to PNG
        Ok(encode_mapzen_elevation_tile_png(&output_raster))
    })
    .await?
}

// Always 256x256
pub fn make_wmts_simple_imagery_raster_for_tile(
    index: &MapzenTileIndex,
) -> ImageryRasterWmtsSimple {
    const TILE_SIZE: i32 = 256;

    // Geotransform in tile-grid coordinates at this zoom.
    // Origin is the NW corner of the tile (top-left pixel's top-left corner,
    // GDAL convention). Pixel resolution is 1 tile / 256 pixels = 1/256.
    // Both resolutions positive because we treat +y as southward, matching
    // the tile-grid convention.
    let origin_x = index.col as f64;
    let origin_y = index.row as f64;
    let resolution_x = 1.0 / TILE_SIZE as f64;
    let resolution_y = 1.0 / TILE_SIZE as f64;

    ImageryRasterWmtsSimple {
        tile_index: *index,
        origin_x,
        origin_y,
        resolution_x,
        resolution_y,
        pixel_dims: glam::ivec2(TILE_SIZE, TILE_SIZE),
        // TODO: Maybe want to make this an MSL/geoid-relative value
        data: vec![glam::u8vec3(0, 0, 0); (TILE_SIZE * TILE_SIZE) as usize],
    }
}

pub fn make_empty_wmts_simple_imagery_tile(index: &MapzenTileIndex) -> Bytes {
    let r = make_wmts_simple_imagery_raster_for_tile(index);
    encode_wmts_simple_imagery_to_jpg(&r)
}

pub async fn build_simple_wmts_imagery_tile(
    resource_loader: resource_io::ResourceLoader,
    raster_template_uri: &str, // Path to rasters, we replace {CONTENT_ROOT_TOKEN}, {FACE}, {LEVEL}, {COL}, {ROW}
    fallback_raster_template_uri: Option<String>, // Path to rasters, we replace {CONTENT_ROOT_TOKEN}, {FACE}, {LEVEL}, {COL}, {ROW}
    s2_content_package_level: i32,
    index: &MapzenTileIndex,
) -> Result<Bytes> {
    tracing::trace!("Index {index:?}");

    // Build GD envelope
    let gd_coverage = mapzen_tile_to_wgs84_envelope(index);
    tracing::trace!("Coverage {gd_coverage:?}");

    // Find corresponding S2 raster level
    let s2_raster_level = mapzen_zoom_to_s2_level(index.zoom)
        .with_context(|| format!("Could not map zoom level {} to S2 level", index.zoom))?;
    tracing::trace!(
        "Mapzen level {} maps to s2 raster level {}",
        index.zoom,
        s2_raster_level
    );

    if s2_raster_level < s2_content_package_level && fallback_raster_template_uri.is_none() {
        // Maybe just make an HAE=0 version and return it?
        bail!("Nothing to do: no fallback raster")
    }

    // Find S2 coverage at S2 raster level
    let s2_raster_tokens = gd_rect_to_s2_coverage(&gd_coverage, s2_raster_level);
    tracing::trace!(
        "Found these s2 tokens at level {}: {:?}",
        s2_raster_level,
        s2_raster_tokens
    );

    // FUTURE OPTIMIZATION: Sort S2 raster tokens by S2 content token bins, just
    // to optimize cache locality

    let uri = if s2_raster_level < s2_content_package_level {
        fallback_raster_template_uri.unwrap()
    } else {
        raster_template_uri.to_string()
    };

    // For each content S2 package, read the S2 raster token files
    let s2_rasters: Vec<_> = join_all(s2_raster_tokens.iter().map(|token| {
        read_content_raster_s2_from_jpg(
            resource_loader.clone(),
            &uri,
            s2_content_package_level,
            *token,
        )
    }))
    .await
    .into_iter()
    // It's ok that these fail sometimes, we might not have entire world coverage of input data
    .filter_map(|r| r.ok())
    .collect();

    // Make output image, our warp target
    let index = *index;
    spawn_blocking(move || {
        let mut output_raster = make_wmts_simple_imagery_raster_for_tile(&index);
        output_raster.data.fill(glam::u8vec3(0, 0, 0));

        // Perform warp
        for s2_raster in &s2_rasters {
            warp_imagery::apply_imagery_warp_from_source(&mut output_raster, s2_raster);
        }

        // Encode image to PNG
        Ok(encode_wmts_simple_imagery_to_jpg(&output_raster))
    })
    .await?
}

pub fn encode_wmts_simple_imagery_to_jpg(raster: &ImageryRasterWmtsSimple) -> Bytes {
    let width = raster.pixel_dims.x as u32;
    let height = raster.pixel_dims.y as u32;

    debug_assert_eq!(
        raster.data.len(),
        (width * height) as usize,
        "Raster data length does not match pixel_dims"
    );

    // Reinterpret Vec<U8Vec3> to &[u8]
    let pixel_bytes: &[u8] = bytemuck::cast_slice(&raster.data);

    let mut buf = BytesMut::new().writer();

    // Scoped to make the encoder flush before freeze
    {
        use jpeg_encoder::{ColorType, Encoder};

        //let mut buf = Vec::new();
        let encoder = Encoder::new(&mut buf, 85); // quality 85
        encoder
            .encode(pixel_bytes, width as u16, height as u16, ColorType::Rgb)
            .expect("JPG encoder should never fail");
    }

    buf.into_inner().freeze()
}

pub async fn read_content_raster_s2_from_jpg(
    resource_loader: resource_io::ResourceLoader,
    raster_template_uri: &str,
    s2_package_level: i32,
    token: s2::cellid::CellID,
) -> Result<ImageryRasterS2> {
    use iri_string::types::UriAbsoluteStr;

    let package_token = token.parent(s2_package_level as u64);

    let (face, level, col, row) = face_level_col_row_from_cell_id(token);

    tracing::trace!("Loading token: {}", token.to_token());

    let jpg_resolved_uri = resolve_elevation_raster_uri_tokens(
        raster_template_uri,
        &package_token,
        face,
        level,
        col,
        row,
    );
    let jpg_uri = UriAbsoluteStr::new(&jpg_resolved_uri)?;
    let jpg_bytes = resource_loader
        .read_async(jpg_uri)
        .await
        .with_context(|| format!("Could not read {jpg_uri}"))?;

    let sidecar_uri = format!("{jpg_resolved_uri}.aux.xml");
    let sidecar_uri_abs = UriAbsoluteStr::new(&sidecar_uri)?;
    let sidecar_bytes = resource_loader
        .read_async(sidecar_uri_abs)
        .await
        .with_context(|| format!("Could not read {sidecar_uri_abs}"))?;

    // Make a string from the aux xml sidecar
    let sidecar_str = std::str::from_utf8(&sidecar_bytes)
        .with_context(|| format!("TIF sidecar at {sidecar_uri} is not valid UTF-8"))?;
    let gt = parse_pam_geotransform(sidecar_str)
        .with_context(|| format!("Could not parse tif sidecar at {sidecar_uri}"))?;

    // Decode the jpg, no copy
    use zune_core::bytestream::ZCursor;
    use zune_jpeg::JpegDecoder;
    let cursor = ZCursor::new(&jpg_bytes);
    let mut decoder = JpegDecoder::new(cursor);
    let pixels = decoder.decode()?;
    let info = decoder.info().unwrap();

    let width = info.width;
    let height = info.height;

    spawn_blocking(move || {
        let data: Vec<_> = pixels
            .chunks_exact(3)
            .map(|rgb| glam::u8vec3(rgb[0], rgb[1], rgb[2]))
            .collect();

        let pixel_level_f = -(gt.pixel_width.log2());
        let pixel_level = pixel_level_f.round() as i32;

        Ok(ImageryRasterS2 {
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
