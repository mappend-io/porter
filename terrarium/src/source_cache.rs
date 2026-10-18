//! Decoded raster sources. A source raster (a 2048x2048 JPEG or PNG) overlaps
//! many output tiles, so it is decoded once and shared, bounded by its pixel
//! bytes. Concurrent tiles that need the same source wait for one decode
//! rather than each decoding it. Missing sources are remembered too: every
//! tile overlapping one would otherwise look for it again.

use crate::{
    ClassRasterS2, ElevationRasterS2, ImageryRasterS2, face_level_col_row_from_cell_id,
    read_content_raster_s2_from_class_png, read_content_raster_s2_from_jpg,
    read_content_raster_s2_from_lerc_zstd_tif, read_content_raster_s2_from_terrarium_rgb_png,
    resolve_elevation_raster_uri_tokens,
};
use anyhow::{Result, anyhow, bail};
use metrics::counter;
use moka::future::Cache;
use resource_io::ResourceLoader;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

enum Decoded {
    Imagery(Arc<ImageryRasterS2>),
    Elevation(Arc<ElevationRasterS2>),
    Class(Arc<ClassRasterS2>),
}

impl Decoded {
    fn bytes(&self) -> usize {
        match self {
            Decoded::Imagery(raster) => raster.data.len() * 3,
            Decoded::Elevation(raster) => raster.data.len() * 4,
            Decoded::Class(raster) => raster.data.len(),
        }
    }
}

// Weight of a remembered miss, for the entry's bookkeeping
const MISS_BYTES: usize = 256;

/// Decoded sources by resolved URI; `None` is a source that doesn't exist
#[derive(Clone)]
pub struct RasterSourceCache {
    cache: Cache<String, Option<Arc<Decoded>>>,
}

impl RasterSourceCache {
    pub fn new(max_bytes: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_bytes)
            .weigher(|key: &String, value: &Option<Arc<Decoded>>| {
                let bytes = key.len() + value.as_ref().map_or(MISS_BYTES, |d| d.bytes());
                u32::try_from(bytes).unwrap_or(u32::MAX)
            })
            .build();
        Self { cache }
    }

    /// The JPEG imagery source for `token`, if it exists
    pub async fn imagery(
        &self,
        resource_loader: ResourceLoader,
        template: &str,
        package_level: i32,
        token: s2::cellid::CellID,
    ) -> Result<Option<Arc<ImageryRasterS2>>> {
        let decoded = self
            .get(template, package_level, token, || async {
                read_content_raster_s2_from_jpg(resource_loader, template, package_level, token)
                    .await
                    .map(|raster| Decoded::Imagery(Arc::new(raster)))
            })
            .await?;
        match decoded.as_deref() {
            None => Ok(None),
            Some(Decoded::Imagery(raster)) => Ok(Some(raster.clone())),
            Some(_) => bail!("Source {template} was cached as another kind of raster"),
        }
    }

    /// The elevation source for `token`, if it exists: Terrarium RGB PNG for
    /// `.png` templates, LERC/zstd TIFF otherwise
    pub async fn elevation(
        &self,
        resource_loader: ResourceLoader,
        template: &str,
        package_level: i32,
        token: s2::cellid::CellID,
    ) -> Result<Option<Arc<ElevationRasterS2>>> {
        let decoded = self
            .get(template, package_level, token, || async {
                let raster = if template.ends_with(".png") {
                    read_content_raster_s2_from_terrarium_rgb_png(
                        resource_loader,
                        template,
                        package_level,
                        token,
                    )
                    .await?
                } else {
                    read_content_raster_s2_from_lerc_zstd_tif(
                        resource_loader,
                        template,
                        package_level,
                        token,
                    )
                    .await?
                };
                Ok(Decoded::Elevation(Arc::new(raster)))
            })
            .await?;
        match decoded.as_deref() {
            None => Ok(None),
            Some(Decoded::Elevation(raster)) => Ok(Some(raster.clone())),
            Some(_) => bail!("Source {template} was cached as another kind of raster"),
        }
    }

    /// The class ID PNG source for `token`, if it exists
    pub async fn classes(
        &self,
        resource_loader: ResourceLoader,
        template: &str,
        package_level: i32,
        token: s2::cellid::CellID,
    ) -> Result<Option<Arc<ClassRasterS2>>> {
        let decoded = self
            .get(template, package_level, token, || async {
                read_content_raster_s2_from_class_png(
                    resource_loader,
                    template,
                    package_level,
                    token,
                )
                .await
                .map(|raster| Decoded::Class(Arc::new(raster)))
            })
            .await?;
        match decoded.as_deref() {
            None => Ok(None),
            Some(Decoded::Class(raster)) => Ok(Some(raster.clone())),
            Some(_) => bail!("Source {template} was cached as another kind of raster"),
        }
    }

    // Look up the source `template` names for `token`, reading and decoding
    // it with `read` on a miss. Missing sources are cached; failed reads and
    // decodes are not, so they're retried.
    async fn get<F, Fut>(
        &self,
        template: &str,
        package_level: i32,
        token: s2::cellid::CellID,
        read: F,
    ) -> Result<Option<Arc<Decoded>>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Decoded>>,
    {
        let (face, level, col, row) = face_level_col_row_from_cell_id(token);
        let key = resolve_elevation_raster_uri_tokens(
            template,
            &token.parent(package_level as u64),
            face,
            level,
            col,
            row,
        );

        counter!("porter_raster_source_cache_queries").increment(1);
        let missed = AtomicBool::new(false);
        let result = self
            .cache
            .try_get_with(key, async {
                missed.store(true, Ordering::Relaxed);
                counter!("porter_raster_source_cache_misses").increment(1);
                match read().await {
                    Ok(decoded) => Ok(Some(Arc::new(decoded))),
                    Err(e) if is_missing(&e) => Ok(None),
                    Err(e) => Err(e),
                }
            })
            .await;
        if !missed.load(Ordering::Relaxed) {
            counter!("porter_raster_source_cache_hits").increment(1);
        }
        // The error is shared with other waiters, which lose its downcasts.
        // Only missing sources are told apart, and those are an Ok(None).
        result.map_err(|e| anyhow!("{e:#}"))
    }
}

// Sources outside the data's coverage don't exist, and the tile is built
// without them. Any other failure fails the tile, rather than serving a partial
// one that would be cached as if it were complete.
fn is_missing(e: &anyhow::Error) -> bool {
    e.downcast_ref::<resource_io::Error>()
        .is_some_and(|e| e.is_not_found())
}

/// The sources that exist, or the first failure
pub fn present<T>(results: Vec<Result<Option<T>>>) -> Result<Vec<T>> {
    results
        .into_iter()
        .filter_map(|result| result.transpose())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn only_missing_sources_are_skipped() {
        let missing: Result<()> = Err(anyhow::Error::from(resource_io::Error::NotFound(
            "no such key".to_string(),
        )))
        .context("Could not read source");
        assert!(is_missing(&missing.unwrap_err()));

        let failed: Result<()> = Err(anyhow::Error::from(resource_io::Error::Io(
            std::io::Error::other("S3 GetObject failed"),
        )))
        .context("Could not read source");
        assert!(!is_missing(&failed.unwrap_err()));

        assert_eq!(
            present(vec![Ok(Some(1)), Ok(None), Ok(Some(2))]).unwrap(),
            [1, 2]
        );
        assert!(present(vec![Ok(Some(1)), Err(anyhow!("read failed"))]).is_err());
    }

    #[tokio::test]
    async fn decodes_each_source_once_and_remembers_misses() {
        // A package at level 0, with one 4x4 class PNG at level 1
        let dir = std::env::temp_dir().join(format!("porter-source-cache-{}", std::process::id()));
        let template = format!(
            "file://{}/{{FACE}}/{{LEVEL}}/{{COL}}/{{ROW}}.png",
            dir.display()
        );
        let token = s2::cellid::CellID::from_face(0).children()[0];
        let (face, level, col, row) = face_level_col_row_from_cell_id(token);
        let path = dir.join(format!("{face}/{level}/{col}/{row}.png"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let pixels: Vec<u8> = (0..16).collect();
        {
            let mut encoder = png::Encoder::new(std::fs::File::create(&path).unwrap(), 4, 4);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
        std::fs::write(
            path.with_extension("png.aux.xml"),
            "<PAMDataset><GeoTransform>0, 0.25, 0, 1, 0, -0.25</GeoTransform></PAMDataset>",
        )
        .unwrap();
        // A sibling that can't be decoded
        let bad = s2::cellid::CellID::from_face(0).children()[1];
        let (face, level, col, row) = face_level_col_row_from_cell_id(bad);
        let bad_path = dir.join(format!("{face}/{level}/{col}/{row}.png"));
        std::fs::create_dir_all(bad_path.parent().unwrap()).unwrap();
        std::fs::write(&bad_path, b"not a png").unwrap();
        std::fs::write(
            bad_path.with_extension("png.aux.xml"),
            "<GeoTransform>0,1,0,0,0,-1</GeoTransform>",
        )
        .unwrap();

        let loader = ResourceLoader::new(Default::default()).await;
        let cache = RasterSourceCache::new(64 * 1024 * 1024);

        let first = cache
            .classes(loader.clone(), &template, 0, token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.data, pixels);
        assert_eq!(first.pixel_dims, glam::ivec2(4, 4));
        let second = cache
            .classes(loader.clone(), &template, 0, token)
            .await
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second), "decoded again");

        let missing = s2::cellid::CellID::from_face(0).children()[2];
        for _ in 0..2 {
            assert!(
                cache
                    .classes(loader.clone(), &template, 0, missing)
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        for _ in 0..2 {
            assert!(
                cache
                    .classes(loader.clone(), &template, 0, bad)
                    .await
                    .is_err()
            );
        }

        std::fs::remove_dir_all(dir).unwrap();
    }
}
