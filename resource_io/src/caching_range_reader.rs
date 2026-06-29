use super::range_reader::RangeReader;
use super::{BytesWeighter, Error};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::future::try_join_all;
use metrics::counter;
use quick_cache::sync::Cache;
use std::sync::Arc;

pub struct CachingRangeReader {
    reader: Arc<dyn RangeReader>,
    block_size: u64,
    cache_namespace: [u8; 16],                                // md5 of path
    cache: Arc<Cache<([u8; 16], u64), Bytes, BytesWeighter>>, // shared cache
}

impl CachingRangeReader {
    pub fn new(
        reader: Arc<dyn RangeReader>,
        block_size: u64,
        cache_namespace: [u8; 16],
        cache: Arc<Cache<([u8; 16], u64), Bytes, BytesWeighter>>,
    ) -> Self {
        CachingRangeReader {
            reader,
            block_size,
            cache_namespace,
            cache,
        }
    }

    async fn fetch_block_async(&self, block_index: u64) -> Result<Bytes, Error> {
        let key = (self.cache_namespace, block_index);
        let cache_result = self.cache.get_value_or_guard_async(&key).await;
        counter!("caching_range_reader_queries").increment(1);

        // If we already have the result, early out, otherwise take a guard
        let guard = match cache_result {
            Ok(bytes) => {
                counter!("caching_range_reader_hits").increment(1);
                return Ok(bytes);
            }
            Err(guard) => guard,
        };

        counter!("caching_range_reader_misses").increment(1);

        let file_size = self.size();
        let fetch_offset = block_index * self.block_size;

        // Guard against out of bounds
        if fetch_offset >= file_size {
            let empty = Bytes::new();
            let _ = guard.insert(empty.clone());
            return Ok(empty);
        }

        let remaining_bytes = file_size - fetch_offset;
        let fetch_length = std::cmp::min(self.block_size, remaining_bytes);

        // Await the wrapped reader
        let block_data = self
            .reader
            .read_range_async(fetch_offset, fetch_length)
            .await?;

        // Add to the cache, resolve guards for other waiters
        let _ = guard.insert(block_data.clone());

        Ok(block_data)
    }
}

#[async_trait]
impl RangeReader for CachingRangeReader {
    async fn read_range_async(&self, offset: u64, length: u64) -> Result<Bytes, Error> {
        if length == 0 {
            return Ok(Bytes::new());
        }

        let start_block = offset / self.block_size;
        let end_block = (offset + length - 1) / self.block_size;

        // If it's a single block, take a zero-copy slice of the block
        if start_block == end_block {
            let block = self.fetch_block_async(start_block).await?;
            let block_offset = (offset % self.block_size) as usize;

            // Don't reach out of bounds if the file ended early
            // TODO: Have an IncompleteRead if block.len() != requested len
            let available_bytes = block.len().saturating_sub(block_offset);
            let actual_length = std::cmp::min(length as usize, available_bytes);

            // Return a zero-copy slice of the cached block
            return Ok(block.slice(block_offset..(block_offset + actual_length)));
        }

        let mut result = BytesMut::with_capacity(length as usize);
        let mut current_offset = offset;
        let mut remaining_length = length;

        let fetch_futures =
            (start_block..=end_block).map(|block_idx| self.fetch_block_async(block_idx));

        // Fetch all blocks concurrently
        let blocks = try_join_all(fetch_futures).await?;

        // The blocks are sequentially ordered, smash them into the result
        for block in blocks {
            let block_offset = (current_offset % self.block_size) as usize;

            let available_bytes = block.len().saturating_sub(block_offset);
            let bytes_to_take = std::cmp::min(remaining_length as usize, available_bytes);

            if bytes_to_take == 0 {
                // TODO: Have an IncompleteRead if block.len() != requested len
                break; // We reached EOF before fulfilling the full length requested
            }

            result.extend_from_slice(&block[block_offset..(block_offset + bytes_to_take)]);

            current_offset += bytes_to_take as u64;
            remaining_length -= bytes_to_take as u64;
        }

        Ok(result.freeze())
    }

    async fn read_from_end_async(&self, length: u64) -> Result<Bytes, Error> {
        let size = self.size();
        let actual_length = std::cmp::min(length, size);
        let offset = size - actual_length;
        self.read_range_async(offset, actual_length).await
    }

    fn size(&self) -> u64 {
        self.reader.size()
    }
}
