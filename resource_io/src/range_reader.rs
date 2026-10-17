use super::Error;
use async_trait::async_trait;
use bytes::Bytes;

#[async_trait]
pub trait RangeReader: Send + Sync {
    async fn read_range_async(&self, offset: u64, length: u64) -> Result<Bytes, Error>;
    async fn read_from_end_async(&self, length: u64) -> Result<Bytes, Error>;
    fn size(&self) -> u64;

    /// Read a range from the underlying storage, bypassing any block cache.
    /// Useful when the caller caches the result at a finer granularity.
    async fn read_range_direct_async(&self, offset: u64, length: u64) -> Result<Bytes, Error> {
        self.read_range_async(offset, length).await
    }
}
