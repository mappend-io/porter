use super::archive_entry::ArchiveEntry;
use super::archive_index::ArchiveIndex;
use super::error::Error;
use super::file_range_reader::FileRangeReader;
use super::range_reader::RangeReader;
use super::{ArchiveEntryWeighter, ArchiveIndexWeighter, BytesWeighter};
use crate::s3_range_reader::S3RangeReader;
use bytes::Bytes;
use iri_string::types::{UriAbsoluteStr, UriAbsoluteString};
use metrics::counter;
use quick_cache::sync::Cache;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;

// read_many: entries closer together than this are fetched in one request. Small
// gaps cost less than another request.
const COALESCE_GAP_BYTES: u64 = 64 * 1_024;
// read_many: cap coalesced requests so a tile's reads stay spread over many requests
const MAX_COALESCED_RANGE_BYTES: u64 = 8 * 1_024 * 1_024;
// read_many: concurrent range requests per archive
const RANGE_READ_CONCURRENCY: usize = 64;
// read_many: concurrency for reads that go through read_async
const READ_MANY_FALLBACK_CONCURRENCY: usize = 16;
// Below this many entries per task, the cost of another blocking task outweighs
// spreading decompression over more threads
const MIN_ENTRIES_PER_DECOMPRESS_TASK: usize = 16;

#[derive(Clone)]
pub struct ResourceLoader {
    readers: Arc<Cache<String, Arc<dyn RangeReader>>>,
    archive_index_cache: Arc<Cache<String, Arc<ArchiveIndex>, ArchiveIndexWeighter>>,
    block_cache: Arc<Cache<([u8; 16], u64), Bytes, BytesWeighter>>, // shared across all block readers to help bound memory
    entry_cache: Arc<Cache<([u8; 16], u64), ArchiveEntry, ArchiveEntryWeighter>>, // compressed archive entries read by read_many
    s3_client: aws_sdk_s3::Client,
}

#[derive(Clone, Debug)]
pub struct ResourceLoaderConfig {
    pub max_readers: u64,
    pub archive_index_cache_bytes: u64,
    pub block_cache_bytes: u64,
    pub entry_cache_bytes: u64,
}

impl Default for ResourceLoaderConfig {
    fn default() -> Self {
        Self {
            max_readers: 8 * 1_024,
            archive_index_cache_bytes: 2 * 1_024 * 1_024 * 1_024, // 2GiB
            block_cache_bytes: 2 * 1_024 * 1_024 * 1_024,         // 2GiB
            entry_cache_bytes: 1_024 * 1_024 * 1_024,             // 1GiB
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

        // Referenced models are typically ~5-20KiB compressed
        let entry_cache_estimated_items = config.entry_cache_bytes / (16 * 1_024);
        let entry_cache = Cache::with_weighter(
            entry_cache_estimated_items as usize,
            config.entry_cache_bytes,
            ArchiveEntryWeighter,
        );

        let s3_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let s3_client = aws_sdk_s3::Client::new(&s3_config);

        Self {
            readers: Arc::new(Cache::new(config.max_readers as usize)),
            archive_index_cache: Arc::new(archive_index_cache),
            block_cache: Arc::new(block_cache),
            entry_cache: Arc::new(entry_cache),
            s3_client,
        }
    }

    fn validate_uri(uri: &UriAbsoluteStr) -> Result<(), Error> {
        // Normalize it yourself outside
        if !uri.is_normalized() {
            return Err(Error::BadUri("uri is not normalized".to_string()));
        }

        // We don't allow relative file URIs that depend on cwd, or should we?
        if uri.scheme_str() == "file" && !uri.path_str().starts_with('/') {
            return Err(Error::BadUri("File uri must be absolute".to_string()));
        }

        Ok(())
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
                let path = url::Url::parse(parsed_uri.as_str())
                    .map_err(|e| Error::BadUri(e.to_string()))?
                    .to_file_path()
                    .map_err(|_| {
                        Error::BadUri(format!("Could not extract file path from uri {uri}"))
                    })?;
                Ok(Arc::new(FileRangeReader::new(&path.to_string_lossy())?)
                    as Arc<dyn RangeReader>)
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

    // Sometimes we need to bypass the cache for guaranteed freshness
    async fn build_uncached_reader(&self, uri: &str) -> Result<Arc<dyn RangeReader>, Error> {
        let parsed_uri = UriAbsoluteStr::new(uri)?;
        match parsed_uri.scheme_str() {
            "file" => {
                let path = url::Url::parse(parsed_uri.as_str())
                    .map_err(|e| Error::BadUri(e.to_string()))?
                    .to_file_path()
                    .map_err(|_| {
                        Error::BadUri(format!("Could not extract file path from uri {uri}"))
                    })?;
                Ok(Arc::new(FileRangeReader::new(&path.to_string_lossy())?)
                    as Arc<dyn RangeReader>)
            }
            "https" | "http" => todo!(),
            "s3" => {
                let bucket = parsed_uri.authority_str().unwrap().to_string();
                let path = parsed_uri.path_str().trim_start_matches('/').to_string();
                let s3_reader =
                    Arc::new(S3RangeReader::new(self.s3_client.clone(), bucket, path).await?);
                Ok(s3_reader as Arc<dyn RangeReader>)
            }
            _ => Err(Error::BadUri("Unsupported uri scheme".to_string())),
        }
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
        Self::validate_uri(uri)?;

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
                    // Deflate (offloaded to blocking pool to avoid blocking the Tokio runtime)
                    let compressed_data = compressed_data.clone();
                    tokio::task::spawn_blocking(move || {
                        use std::io::Read;
                        let mut decoder = flate2::read::DeflateDecoder::new(&compressed_data[..]);
                        let mut out = Vec::with_capacity(uncompressed_size);
                        decoder
                            .read_to_end(&mut out)
                            .map_err(|e| Error::Decompression(e.to_string()))?;
                        Ok::<Bytes, Error>(Bytes::from(out))
                    })
                    .await
                    .map_err(|e| {
                        Error::Decompression(format!("Tokio spawn_blocking failed: {}", e))
                    })??
                }
                93 => {
                    // Zstandard (offloaded to blocking pool)
                    let compressed_data = compressed_data.clone();
                    tokio::task::spawn_blocking(move || {
                        let out = zstd::stream::decode_all(&compressed_data[..])
                            .map_err(|e| Error::Decompression(e.to_string()))?;
                        Ok::<Bytes, Error>(Bytes::from(out))
                    })
                    .await
                    .map_err(|e| {
                        Error::Decompression(format!("Tokio spawn_blocking failed: {}", e))
                    })??
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

    // Bypasses reader and block caches for a fresh result
    pub async fn read_async_uncached(&self, uri: &UriAbsoluteStr) -> Result<Bytes, Error> {
        if !uri.is_normalized() {
            return Err(Error::BadUri("uri is not normalized".to_string()));
        }

        if uri.scheme_str() == "file" && !uri.path_str().starts_with('/') {
            return Err(Error::BadUri("File uri must be absolute".to_string()));
        }

        if Self::split_archive_parts(uri.as_str()).is_some() {
            return Err(Error::BadUri(
                "read_async_uncached does not support archive-embedded uris".to_string(),
            ));
        }

        let reader = self.build_uncached_reader(uri.as_str()).await?;
        reader.read_range_async(0, reader.size()).await
    }

    /// Read many URIs at once.
    ///
    /// Entries inside archives are fetched with exact ranges (an entry ends where
    /// the next one in the archive starts) instead of through the block cache,
    /// coalescing entries that sit close together into one request, and the
    /// compressed entries are cached. Referenced models are small and scattered
    /// throughout their archive, so whole blocks mostly fetch unrelated entries:
    /// 8-10x more bytes than needed for a typical building tile.
    pub async fn read_many(
        &self,
        uris: &[&UriAbsoluteStr],
    ) -> HashMap<UriAbsoluteString, Result<Bytes, Error>> {
        use futures::stream::{self, StreamExt};
        use std::collections::BTreeMap;

        let mut results = HashMap::with_capacity(uris.len());

        // Anything that can't be resolved with a ranged read takes the regular path
        let mut fallback: Vec<UriAbsoluteString> = vec![];

        // Group: archive_path → Vec<(uri, path within archive)>
        let mut by_archive: BTreeMap<String, Vec<(UriAbsoluteString, String)>> = BTreeMap::new();
        for uri in uris {
            let owned = UriAbsoluteStr::to_owned(*uri);
            if let Err(e) = Self::validate_uri(uri) {
                results.insert(owned, Err(e));
                continue;
            }
            match Self::split_archive_parts(uri.as_str()) {
                Some((archive_path, content_path)) => by_archive
                    .entry(archive_path)
                    .or_default()
                    .push((owned, content_path)),
                None => fallback.push(owned),
            }
        }

        let mut entries = vec![];
        for (archive_path, members) in by_archive {
            match self
                .read_archive_entries(&archive_path, members.clone())
                .await
            {
                Ok((found, missed)) => {
                    entries.extend(found);
                    fallback.extend(missed);
                }
                // Let the regular path report a per-uri error
                Err(_) => fallback.extend(members.into_iter().map(|(uri, _)| uri)),
            }
        }

        results.extend(decompress_entries(entries).await);

        let rest: Vec<_> = stream::iter(fallback.into_iter().map(|uri| async move {
            let result = self.read_async(uri.as_ref()).await;
            (uri, result)
        }))
        .buffer_unordered(READ_MANY_FALLBACK_CONCURRENCY)
        .collect()
        .await;
        results.extend(rest);

        results
    }

    // Fetch entries of one archive with coalesced exact-range reads. Returns the
    // entries found, and the uris that need to go through the regular path.
    async fn read_archive_entries(
        &self,
        archive_path: &str,
        members: Vec<(UriAbsoluteString, String)>,
    ) -> Result<
        (
            Vec<(UriAbsoluteString, ArchiveEntry)>,
            Vec<UriAbsoluteString>,
        ),
        Error,
    > {
        use futures::stream::{self, StreamExt};

        let reader = self.get_reader_async(archive_path).await?;
        let index = self
            .get_archive_index_async(archive_path, reader.clone())
            .await?;
        // Same namespace as the block cache uses for this archive
        let namespace = ArchiveIndex::hash_path(archive_path);

        let mut found = vec![];
        let mut missed = vec![];

        // (offset, end, uri, content_path) of entries not in the entry cache
        let mut wanted = vec![];
        for (uri, content_path) in members {
            let Some(offset) = index.find_offset(&ArchiveIndex::hash_path(&content_path)) else {
                missed.push(uri);
                continue;
            };
            counter!("porter_archive_entry_cache_queries").increment(1);
            if let Some(entry) = self.entry_cache.get(&(namespace, offset)) {
                counter!("porter_archive_entry_cache_hits").increment(1);
                found.push((uri, entry));
                continue;
            }
            counter!("porter_archive_entry_cache_misses").increment(1);
            match index.entry_end(offset) {
                Some(end) if end > offset => wanted.push((offset, end, uri, content_path)),
                _ => missed.push(uri),
            }
        }

        // Coalesce entries into ranges, in archive order
        wanted.sort_unstable_by_key(|w| w.0);
        let mut ranges: Vec<CoalescedRange> = vec![];
        for (offset, end, uri, content_path) in wanted {
            match ranges.last_mut() {
                Some((start, range_end, members))
                    if offset <= *range_end + COALESCE_GAP_BYTES
                        && end - *start <= MAX_COALESCED_RANGE_BYTES =>
                {
                    *range_end = (*range_end).max(end);
                    members.push((offset, uri, content_path));
                }
                _ => ranges.push((offset, end, vec![(offset, uri, content_path)])),
            }
        }

        let fetched: Vec<_> = stream::iter(ranges.into_iter().map(|(start, end, members)| {
            let reader = reader.clone();
            async move {
                counter!("porter_archive_entry_range_reads_total").increment(1);
                let result = reader.read_range_direct_async(start, end - start).await;
                (result, start, members)
            }
        }))
        .buffer_unordered(RANGE_READ_CONCURRENCY)
        .collect()
        .await;

        for (result, start, members) in fetched {
            let Ok(buf) = result else {
                missed.extend(members.into_iter().map(|(_, uri, _)| uri));
                continue;
            };
            for (offset, uri, content_path) in members {
                let rel = (offset - start) as usize;
                if rel >= buf.len() {
                    missed.push(uri);
                    continue;
                }
                match ArchiveEntry::parse(&buf.slice(rel..), &content_path) {
                    Ok(Some(mut entry)) => {
                        // Don't let a cached entry pin the whole coalesced buffer
                        entry.data = Bytes::copy_from_slice(&entry.data);
                        self.entry_cache.insert((namespace, offset), entry.clone());
                        found.push((uri, entry));
                    }
                    // Collisions, data descriptors and errors are handled (and
                    // reported) by the regular path
                    _ => missed.push(uri),
                }
            }
        }

        Ok((found, missed))
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
                let path = url::Url::parse(uri.as_str())
                    .map_err(|e| Error::BadUri(e.to_string()))?
                    .to_file_path()
                    .map_err(|_| {
                        Error::BadUri(format!("Could not extract file path from uri {uri}"))
                    })?;
                let mut entries = tokio::fs::read_dir(path).await?;
                let mut items = vec![];
                while let Some(entry) = entries.next_entry().await? {
                    if entry.file_type().await?.is_file() {
                        let path = entry.path();
                        if let Some(file_name) = path.file_name() {
                            items.push(file_name.to_string_lossy().to_string());
                        }
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
                    .prefix(&prefix)
                    .send()
                    .await
                    .map_err(|e| Error::S3(e.to_string()))?;

                let mut items = vec![];
                for obj in res.contents() {
                    if let Some(key) = obj.key() {
                        // Strip the prefix, it better be there
                        let Some(relative) = key.strip_prefix(&prefix) else {
                            continue;
                        };
                        // Only keep non-empty keys without a slash
                        if !relative.contains('/') && !relative.is_empty() {
                            items.push(relative.to_string());
                        }
                    }
                }

                Ok(items)
            }
            _ => Err(Error::BadUri("Unsupported uri scheme".to_string())),
        }
    }
}

// Decompress entries on the blocking pool, split into chunks across threads
// rather than one task per entry
async fn decompress_entries(
    mut entries: Vec<(UriAbsoluteString, ArchiveEntry)>,
) -> Vec<(UriAbsoluteString, Result<Bytes, Error>)> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk_size = entries
        .len()
        .div_ceil(workers)
        .max(MIN_ENTRIES_PER_DECOMPRESS_TASK);

    let mut tasks = tokio::task::JoinSet::new();
    // Keep each task's uris so a failed task still reports per-uri errors
    let mut task_uris = HashMap::new();
    while !entries.is_empty() {
        let chunk = entries.split_off(entries.len().saturating_sub(chunk_size));
        let uris: Vec<_> = chunk.iter().map(|(uri, _)| uri.clone()).collect();
        let handle = tasks.spawn_blocking(move || {
            chunk
                .into_iter()
                .map(|(uri, entry)| (uri, entry.decompress()))
                .collect::<Vec<_>>()
        });
        task_uris.insert(handle.id(), uris);
    }

    let mut results = vec![];
    while let Some(result) = tasks.join_next_with_id().await {
        match result {
            Ok((_, decompressed)) => results.extend(decompressed),
            Err(e) => {
                for uri in task_uris.remove(&e.id()).unwrap_or_default() {
                    results.push((
                        uri,
                        Err(Error::Decompression(format!(
                            "Tokio spawn_blocking failed: {e}"
                        ))),
                    ));
                }
            }
        }
    }
    results
}

// read_many: one coalesced read, [start, end) of the archive, and the entries in
// it as (offset, uri, path within the archive)
type CoalescedRange = (u64, u64, Vec<(u64, UriAbsoluteString, String)>);
