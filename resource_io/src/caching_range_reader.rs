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

    /// Insert the blocks that lie entirely within `tail` (bytes from
    /// `tail_offset` to the end of the object) into the cache
    pub fn seed_from_tail(&self, tail_offset: u64, tail: &Bytes) {
        let size = self.size();
        debug_assert_eq!(tail_offset + tail.len() as u64, size);

        let first_block = tail_offset.div_ceil(self.block_size);
        let mut block_index = first_block;
        while block_index * self.block_size < size {
            let start = block_index * self.block_size;
            let end = std::cmp::min(start + self.block_size, size);
            // Copy, so the cache doesn't pin the unaligned head of the tail
            let block = Bytes::copy_from_slice(
                &tail[(start - tail_offset) as usize..(end - tail_offset) as usize],
            );
            self.cache
                .insert((self.cache_namespace, block_index), block);
            block_index += 1;
        }
    }

    async fn fetch_block_async(&self, block_index: u64) -> Result<Bytes, Error> {
        let key = (self.cache_namespace, block_index);
        let cache_result = self.cache.get_value_or_guard_async(&key).await;
        counter!("porter_caching_range_reader_queries").increment(1);

        // If we already have the result, early out, otherwise take a guard
        let guard = match cache_result {
            Ok(bytes) => {
                counter!("porter_caching_range_reader_hits").increment(1);
                return Ok(bytes);
            }
            Err(guard) => guard,
        };

        counter!("porter_caching_range_reader_misses").increment(1);

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

            let available_bytes = block.len().saturating_sub(block_offset);
            let actual_length = std::cmp::min(length as usize, available_bytes);

            return Ok(block.slice(block_offset..(block_offset + actual_length)));
        }

        // If this is a large read (arbitrarily defined here), don't try to cache
        // it at all. Just read directly. Huge objects pollute the cache, and are
        // generally something read a single time (a huge archive index, maybe).
        if end_block - start_block > 16 {
            counter!("porter_caching_range_reader_large_reads").increment(1);
            return self.reader.read_range_async(offset, length).await;
        }

        let mut result = BytesMut::with_capacity(length as usize);
        let mut current_offset = offset;
        let mut remaining_length = length;

        let fetch_futures =
            (start_block..=end_block).map(|block_idx| self.fetch_block_async(block_idx));

        let blocks = try_join_all(fetch_futures).await?;

        for block in blocks {
            let block_offset = (current_offset % self.block_size) as usize;

            let available_bytes = block.len().saturating_sub(block_offset);
            let bytes_to_take = std::cmp::min(remaining_length as usize, available_bytes);

            if bytes_to_take == 0 {
                break;
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

    async fn read_range_direct_async(&self, offset: u64, length: u64) -> Result<Bytes, Error> {
        if let Some(bytes) = self.read_cached_range(offset, length) {
            return Ok(bytes);
        }
        self.reader.read_range_async(offset, length).await
    }
}

impl CachingRangeReader {
    /// The range, if every block it covers is already cached (e.g. seeded from
    /// the tail); never fetches
    fn read_cached_range(&self, offset: u64, length: u64) -> Option<Bytes> {
        let length = std::cmp::min(length, self.size().saturating_sub(offset));
        if length == 0 {
            return Some(Bytes::new());
        }
        let start_block = offset / self.block_size;
        let end_block = (offset + length - 1) / self.block_size;
        let blocks = (start_block..=end_block)
            .map(|block_index| self.cache.get(&(self.cache_namespace, block_index)))
            .collect::<Option<Vec<_>>>()?;

        let end = offset + length;
        if let [block] = blocks.as_slice() {
            let block_start = start_block * self.block_size;
            let range = (offset - block_start) as usize..(end - block_start) as usize;
            return (range.end <= block.len()).then(|| block.slice(range));
        }
        let mut result = BytesMut::with_capacity(length as usize);
        for (block_index, block) in (start_block..).zip(&blocks) {
            let block_start = block_index * self.block_size;
            let from = offset.saturating_sub(block_start) as usize;
            let to = std::cmp::min((end - block_start) as usize, block.len());
            result.extend_from_slice(block.get(from..to)?);
        }
        (result.len() as u64 == length).then(|| result.freeze())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MemReader {
        data: Bytes,
        reads: AtomicUsize,
    }

    #[async_trait]
    impl RangeReader for MemReader {
        async fn read_range_async(&self, offset: u64, length: u64) -> Result<Bytes, Error> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let end = std::cmp::min(offset + length, self.data.len() as u64);
            Ok(self.data.slice(offset as usize..end as usize))
        }
        async fn read_from_end_async(&self, length: u64) -> Result<Bytes, Error> {
            let size = self.size();
            self.read_range_async(size - length, length).await
        }
        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[tokio::test]
    async fn seeded_tail_blocks_need_no_reads() {
        // 16 byte blocks, 56 bytes: blocks 0-2 are full, block 3 has 8 bytes
        let data = Bytes::from((0..56u8).collect::<Vec<_>>());
        let inner = Arc::new(MemReader {
            data: data.clone(),
            reads: AtomicUsize::new(0),
        });
        let cache = Arc::new(Cache::with_weighter(16, 1024, BytesWeighter));
        let reader = CachingRangeReader::new(inner.clone(), 16, [0; 16], cache);

        // A tail starting mid-block 1 seeds blocks 2 and 3 only
        reader.seed_from_tail(20, &data.slice(20..));

        assert_eq!(
            reader.read_from_end_async(8).await.unwrap(),
            data.slice(48..)
        );
        assert_eq!(
            reader.read_range_async(32, 24).await.unwrap(),
            data.slice(32..)
        );
        assert_eq!(inner.reads.load(Ordering::Relaxed), 0);

        assert_eq!(
            reader.read_range_async(20, 4).await.unwrap(),
            data.slice(20..24)
        );
        assert_eq!(inner.reads.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn direct_reads_use_cached_blocks_only() {
        let data = Bytes::from((0..56u8).collect::<Vec<_>>());
        let inner = Arc::new(MemReader {
            data: data.clone(),
            reads: AtomicUsize::new(0),
        });
        let cache = Arc::new(Cache::with_weighter(16, 1024, BytesWeighter));
        let reader = CachingRangeReader::new(inner.clone(), 16, [0; 16], cache);
        reader.seed_from_tail(20, &data.slice(20..));

        // Within the seeded blocks 2 and 3, across a block boundary and to EOF
        for (offset, length) in [(33, 5), (40, 16), (32, 100)] {
            let end = std::cmp::min(offset + length, 56);
            assert_eq!(
                reader
                    .read_range_direct_async(offset, length)
                    .await
                    .unwrap(),
                data.slice(offset as usize..end as usize)
            );
        }
        assert_eq!(inner.reads.load(Ordering::Relaxed), 0);

        // Touching an uncached block reads the whole range from the inner
        // reader, without caching it
        assert_eq!(
            reader.read_range_direct_async(10, 30).await.unwrap(),
            data.slice(10..40)
        );
        assert_eq!(
            reader.read_range_direct_async(10, 30).await.unwrap(),
            data.slice(10..40)
        );
        assert_eq!(inner.reads.load(Ordering::Relaxed), 2);
    }
}
