use super::archive_index::ArchiveIndex;
use super::error::Error;
use super::file_range_reader::FileRangeReader;
use super::range_reader::RangeReader;
use super::{ArchiveIndexWeighter, BytesWeighter};
use crate::s3_range_reader::S3RangeReader;
use bytes::Bytes;
use iri_string::types::{UriAbsoluteStr, UriAbsoluteString};
use quick_cache::sync::Cache;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;

#[derive(Clone)]
pub struct ResourceLoader {
    readers: Arc<Cache<String, Arc<dyn RangeReader>>>,
    archive_index_cache: Arc<Cache<String, Arc<ArchiveIndex>, ArchiveIndexWeighter>>,
    block_cache: Arc<Cache<([u8; 16], u64), Bytes, BytesWeighter>>, // shared across all block readers to help bound memory
    s3_client: aws_sdk_s3::Client,
}

#[derive(Clone, Debug)]
pub struct ResourceLoaderConfig {
    pub max_readers: u64,
    pub archive_index_cache_bytes: u64,
    pub block_cache_bytes: u64,
}

impl Default for ResourceLoaderConfig {
    fn default() -> Self {
        Self {
            max_readers: 8 * 1_024,
            archive_index_cache_bytes: 2 * 1_024 * 1_024 * 1_024, // 2GiB
            block_cache_bytes: 2 * 1_024 * 1_024 * 1_024,         // 2GiB
        }
    }
}

impl ResourceLoader {
    pub async fn new(config: ResourceLoaderConfig) -> Self {
        // Archive size can vary significantly. As big as 100-400MB.
        // Let's say it's 50MB on average.
        let archive_index_cache_estimated_items =
            config.archive_index_cache_bytes / (50 * (1 << 20));
        let archive_index_cache = Cache::with_weighter(
            archive_index_cache_estimated_items as usize,
            config.archive_index_cache_bytes,
            ArchiveIndexWeighter,
        );

        // NOTE: Hardcoded 1MiB block size, aligned with caching range reader init below
        let block_cache_estimated_items = config.block_cache_bytes / (1 << 20);
        let block_cache = Cache::with_weighter(
            block_cache_estimated_items as usize,
            config.block_cache_bytes,
            BytesWeighter,
        );

        let s3_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let s3_client = aws_sdk_s3::Client::new(&s3_config);

        Self {
            readers: Arc::new(Cache::new(config.max_readers as usize)),
            archive_index_cache: Arc::new(archive_index_cache),
            block_cache: Arc::new(block_cache),
            s3_client,
        }
    }

    async fn get_reader_async(&self, uri: &str) -> Result<Arc<dyn RangeReader>, Error> {
        let res = self.readers.get_value_or_guard_async(uri).await;
        let guard = match res {
            Ok(res) => {
                return Ok(res);
            }
            Err(guard) => guard,
        };

        let parsed_uri = UriAbsoluteStr::new(uri)?;
        let reader = match parsed_uri.scheme_str() {
            "file" => {
                /*
                let file_reader = Arc::new(FileRangeReader::new(parsed_uri.path_str())?);
                let caching_reader =
                    Arc::new(crate::caching_range_reader::CachingRangeReader::new(
                        file_reader,
                        (1 << 20) * 1,                // TODO
                        ArchiveIndex::hash_path(uri), // TODO
                        self.block_cache.clone(),
                    ));
                Ok(caching_reader as Arc<dyn RangeReader>)
                */
                Ok(Arc::new(FileRangeReader::new(parsed_uri.path_str())?) as Arc<dyn RangeReader>)
            }
            "https" | "http" => todo!(),
            "s3" => {
                let bucket = parsed_uri.authority_str().unwrap().to_string(); // TODO: check for authority, that's the bucket name
                let path = parsed_uri.path_str().trim_start_matches('/').to_string();
                let s3_reader =
                    Arc::new(S3RangeReader::new(self.s3_client.clone(), bucket, path).await?);
                let caching_reader =
                    Arc::new(crate::caching_range_reader::CachingRangeReader::new(
                        s3_reader,
                        1 << 20,                      // TODO
                        ArchiveIndex::hash_path(uri), // TODO
                        self.block_cache.clone(),
                    ));
                Ok(caching_reader as Arc<dyn RangeReader>)
            }
            _ => Err(Error::BadUri("Unsupported uri scheme".to_string())),
        }?;

        let _ = guard.insert(reader.clone());
        Ok(reader)
    }

    async fn get_archive_index_async(
        &self,
        archive_uri: &str,
        archive_reader: Arc<dyn RangeReader>,
    ) -> Result<Arc<ArchiveIndex>, Error> {
        let res = self
            .archive_index_cache
            .get_value_or_guard_async(archive_uri)
            .await;

        match res {
            Ok(res) => Ok(res),
            Err(guard) => {
                let index = Arc::new(ArchiveIndex::from_3tz_range_reader(&*archive_reader).await?);
                // TODO: Should I do something with the return here?
                let _ = guard.insert(index.clone());
                Ok(index)
            }
        }
    }

    // TODO: Probably keep some other context for a user of this to know if they are in a zip/.3tz/allowing .3tz
    pub async fn read_async(&self, uri: &UriAbsoluteStr) -> Result<Bytes, Error> {
        // Normalize it yourself outside
        if !uri.is_normalized() {
            return Err(Error::BadUri("uri is not normalized".to_string()));
        }

        // We don't allow relative file URIs that depend on cwd, or should we?
        if uri.scheme_str() == "file" && !uri.path_str().starts_with('/') {
            return Err(Error::BadUri("File uri must be absolute".to_string()));
        }

        if let Some((archive_path, content_path)) = Self::split_archive_parts(uri.as_str()) {
            let reader = self.get_reader_async(&archive_path).await?;
            let index = self
                .get_archive_index_async(&archive_path, reader.clone())
                .await?;
            let path_md5 = &ArchiveIndex::hash_path(&content_path);

            // 1. Find the base index for the MD5 hash
            let base_idx = index
                .entries
                .binary_search_by(|e| ArchiveIndex::md5_compare(&e.path_md5, path_md5))
                .map_err(|_| Error::NotFound("File not found in archive index".to_string()))?;

            // Expand to find the full range of matching MD5 hashes (handling collisions)
            let mut start_idx = base_idx;
            while start_idx > 0 && index.entries[start_idx - 1].path_md5 == *path_md5 {
                start_idx -= 1;
            }
            let mut end_idx = base_idx;
            while end_idx < index.entries.len() - 1
                && index.entries[end_idx + 1].path_md5 == *path_md5
            {
                end_idx += 1;
            }

            // 2. Iterate through matches to verify filename and extract metadata
            let mut target_file_metadata = None;

            for idx in start_idx..=end_idx {
                let offset = index.entries[idx].offset;

                // Read the 30-byte static Local File Header
                let lfh = reader.read_range_async(offset, 30).await?;
                if lfh.len() < 30 || lfh[0..4] != [0x50, 0x4b, 0x03, 0x04] {
                    return Err(Error::BadArchive(
                        "Invalid Local File Header signature".to_string(),
                    ));
                }

                let filename_len = u16::from_le_bytes(lfh[26..28].try_into().unwrap()) as u64;

                // Read and verify the filename to confirm this isn't a hash collision
                let filename_bytes = reader.read_range_async(offset + 30, filename_len).await?;
                if filename_bytes != content_path.as_bytes() {
                    continue; // Collision detected: try the next adjacent entry
                }

                // Match found! Extract remaining metadata.
                let compression_method = u16::from_le_bytes(lfh[8..10].try_into().unwrap());
                // The 3tz spec caps internal file sizes at 4GB, so standard 32-bit fields are sufficient here
                let compressed_size = u32::from_le_bytes(lfh[18..22].try_into().unwrap()) as u64;
                let uncompressed_size =
                    u32::from_le_bytes(lfh[22..26].try_into().unwrap()) as usize;
                let extra_field_len = u16::from_le_bytes(lfh[28..30].try_into().unwrap()) as u64;

                let data_offset = offset + 30 + filename_len + extra_field_len;

                target_file_metadata = Some((
                    data_offset,
                    compressed_size,
                    uncompressed_size,
                    compression_method,
                ));
                break;
            }

            let (data_offset, compressed_size, uncompressed_size, compression_method) =
                target_file_metadata.ok_or_else(|| {
                    Error::NotFound(
                        "Filename mismatch (hash collision resolved to no valid file)".to_string(),
                    )
                })?;

            // 3. Read compressed bytes
            let compressed_data = reader
                .read_range_async(data_offset, compressed_size)
                .await?;

            // 4. Decompress based on method
            let decompressed_bytes = match compression_method {
                0 => {
                    // Stored (No compression)
                    compressed_data
                }
                8 => {
                    // Deflate
                    use std::io::Read;
                    let mut decoder = flate2::read::DeflateDecoder::new(&compressed_data[..]);
                    let mut out = Vec::with_capacity(uncompressed_size);
                    decoder
                        .read_to_end(&mut out)
                        .map_err(|e| Error::Decompression(e.to_string()))?;
                    Bytes::from(out)
                }
                93 => {
                    // Zstandard
                    let out = zstd::stream::decode_all(&compressed_data[..])
                        .map_err(|e| Error::Decompression(e.to_string()))?;
                    Bytes::from(out)
                }
                _ => {
                    return Err(Error::BadArchive(format!(
                        "Unsupported compression method: {}",
                        compression_method
                    )));
                }
            };

            Ok(decompressed_bytes)
        } else {
            let reader = self.get_reader_async(uri.as_str()).await?;
            let bytes = reader.read_range_async(0, reader.size()).await?;
            Ok(bytes)
        }
    }

    async fn compute_offset_for_uri(&self, uri: &UriAbsoluteStr) -> Option<u64> {
        let (archive_path, content_path) = Self::split_archive_parts(uri.as_str())?;
        let reader = self.get_reader_async(&archive_path).await.ok()?;
        let index = self
            .get_archive_index_async(&archive_path, reader)
            .await
            .ok()?;
        let path_md5 = ArchiveIndex::hash_path(&content_path);

        let base_idx = index
            .entries
            .binary_search_by(|e| ArchiveIndex::md5_compare(&e.path_md5, &path_md5))
            .ok()?;

        Some(index.entries[base_idx].offset)
    }

    // TODO: Mostly useful if reading from an archive, we can lookup the paths and sort them by their order
    // in the file to maximize cache hits for blocks/reuse.
    pub async fn read_many(
        &self,
        uris: &[&UriAbsoluteStr],
    ) -> HashMap<UriAbsoluteString, Result<Bytes, Error>> {
        // Will need to reorder the results back to match the uris in the requested order.

        // TODO: Next step: group uris by archive uri, offset in archive.
        // If uris aren't in archive, fine, can go into a catch-all group.
        // Then work through them in that order.
        //println!("OPTIMIZE THE ORDERING");

        use std::collections::BTreeMap;

        // Group: archive_path → Vec<(uri, offset_within_archive)>
        let mut by_archive: BTreeMap<String, Vec<(UriAbsoluteString, u64)>> = BTreeMap::new();

        for uri in uris {
            let owned = UriAbsoluteStr::to_owned(*uri);
            if let Some((archive_path, _)) = Self::split_archive_parts(uri.as_str()) {
                let offset = self.compute_offset_for_uri(uri).await.unwrap_or(u64::MAX);
                by_archive
                    .entry(archive_path)
                    .or_default()
                    .push((owned, offset));
            } else {
                // Non-archive URI — handle separately, no sorting needed
                by_archive
                    .entry(String::new())
                    .or_default()
                    .push((owned, 0));
            }
        }

        // Sort within each archive by offset, then flatten
        let ordered: Vec<UriAbsoluteString> = by_archive
            .into_values()
            .flat_map(|mut group| {
                group.sort_by_key(|(_, offset)| *offset);
                group.into_iter().map(|(uri, _)| uri)
            })
            .collect();

        use futures::stream::{self, StreamExt};
        const CONCURRENCY: usize = 16;

        stream::iter(ordered.into_iter().map(|uri| async move {
            let result = self.read_async(uri.as_ref()).await;
            (uri, result)
        }))
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await

        /*
        let mut res = HashMap::new();
        for uri in ordered {
            res.insert(uri.clone(), self.read_async(&uri).await);
        }
        res
        */

        /*
        let mut res = HashMap::new();
        for uri in uris {
            res.insert(UriAbsoluteStr::to_owned(*uri), self.read_async(&uri).await);
        }
        res
        */
    }

    // Respect rules in https://github.com/Maxar-Public/3d-tiles/blob/wff1.7.0/extensions/MAXAR_content_3tz/1.0.0/README.md#path-resolver-algorithm
    fn split_archive_parts(path: &str) -> Option<(String, String)> {
        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(.+\.(?:zip|3tz))[\\/]?(.*)$").unwrap());

        RE.captures(path).and_then(|caps| {
            let archive_path = caps.get(1).unwrap().as_str().to_string();
            let internal_path = caps
                .get(2)
                .map_or("".to_string(), |m| m.as_str().to_string());

            if internal_path.is_empty() && !archive_path.ends_with(".3tz") {
                None
            } else {
                let internal_path = if internal_path.is_empty() {
                    "tileset.json".to_string()
                } else {
                    internal_path
                };
                Some((archive_path, internal_path))
            }
        })
    }

    pub async fn list_items_nonrecursive_async(
        &self,
        uri: &UriAbsoluteStr,
    ) -> Result<Vec<String>, Error> {
        match uri.scheme_str() {
            "file" => {
                let path = uri.path_str();
                let mut entries = tokio::fs::read_dir(path).await?;
                let mut items = vec![];
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();
                    if let Some(file_name) = path.file_name() {
                        items.push(file_name.to_string_lossy().to_string());
                    }
                }
                Ok(items)
            }
            "s3" => {
                let bucket = uri.authority_str().unwrap().to_string();
                let prefix = uri.path_str().trim_start_matches('/').to_string();

                let res = self
                    .s3_client
                    .list_objects_v2()
                    .bucket(bucket)
                    .prefix(prefix)
                    .send()
                    .await
                    .map_err(|e| Error::S3(e.to_string()))?;

                let mut items = vec![];
                for obj in res.contents() {
                    if let Some(key) = obj.key() {
                        let stem = key.split('/').next_back().unwrap_or("");
                        if !stem.is_empty() {
                            items.push(stem.to_string());
                        }
                    }
                }

                Ok(items)
            }
            _ => Err(Error::BadUri("Unsupported uri scheme".to_string())),
        }
    }
}
