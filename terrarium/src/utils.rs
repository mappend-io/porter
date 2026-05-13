use anyhow::{Context, Result, bail};

pub struct PamGeoTransform {
    pub origin_x: f64,
    pub pixel_width: f64,
    pub row_rotation: f64,
    pub origin_y: f64,
    pub column_rotation: f64,
    pub pixel_height: f64,
}

pub fn parse_pam_geotransform(xml: &str) -> Result<PamGeoTransform> {
    let re =
        regex::Regex::new(r"<GeoTransform>\s*([^<]+?)\s*</GeoTransform>").expect("Bad GT regex");
    let caps = re
        .captures(xml)
        .context("No <GeoTransform> element found")?;

    let vals: Vec<f64> = caps[1]
        .split(',')
        .map(|s| s.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .context("Could not split GeoTransform values")?;

    if vals.len() != 6 {
        bail!("GeoTransform must have 6 values, got {}", vals.len());
    }

    Ok(PamGeoTransform {
        origin_x: vals[0],
        pixel_width: vals[1],
        row_rotation: vals[2],
        origin_y: vals[3],
        column_rotation: vals[4],
        pixel_height: vals[5],
    })
}

pub fn encode_elevation_to_mapzen_rgb(val: f32) -> glam::U8Vec3 {
    // https://www.mapzen.com/blog/terrain-tile-service/
    // Terrarium format PNG tiles contain raw elevation data in meters, in
    // Mercator projection (EPSG:3857). All values are positive with a 32,768
    // offset, split into the red, green, and blue channels, with 16 bits of
    // integer and 8 bits of fraction. To decode:
    //
    // (red * 256 + green + blue / 256) - 32768

    // So encode is the inverse:
    //
    // shifted = elevation_m + 32768, valid range [0, 65536)
    // r = high byte of the integer part of shifted (8 bits, 256m units)
    // g =  low byte of the integer part of shifted (8 bits, 1m units)
    // b = fractional part of shifted, scaled to 8 bits (1/256m units)
    //
    // Anything outside of [-32768m, +32767.996m] is saturated to the limit.
    // NaNs are mapped to 0m.

    const OFFSET: f32 = 32768.0;
    const MAX_SHIFTED: f32 = 65536.0 - 1.0 / 256.0; // largest representable

    // Defend against nan, shift value offset
    let shifted = if val.is_nan() {
        OFFSET // -> sea level
    } else {
        (val + OFFSET).clamp(0.0, MAX_SHIFTED)
    };

    let int_part = shifted.floor(); // [0, 65535]
    let frac_part = shifted - int_part; // [0, 1)

    let int_u32 = int_part as u32; // bounded to [0, 65535]
    let r = (int_u32 >> 8) as u8;
    let g = (int_u32 & 0xFF) as u8;
    let b = (frac_part * 256.0) as u8; // floor to [0, 255]

    glam::U8Vec3::new(r, g, b)
}

pub fn decode_elevation_from_mapzen_rgb(rgb: glam::U8Vec3) -> f32 {
    // https://www.mapzen.com/blog/terrain-tile-service/
    // Terrarium decode, per the format spec:
    //
    //   elevation_m = (red * 256 + green + blue / 256) - 32768

    const OFFSET: f32 = 32768.0;

    let r = rgb.x as f32;
    let g = rgb.y as f32;
    let b = rgb.z as f32;

    r * 256.0 + g + b / 256.0 - OFFSET
}
