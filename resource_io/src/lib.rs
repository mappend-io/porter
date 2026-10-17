use archive_entry::ArchiveEntry;
use archive_index::ArchiveIndex;
use bytes::Bytes;
use quick_cache::Weighter;
use std::sync::Arc;

pub mod archive_entry;
pub mod archive_index;
pub mod caching_range_reader;
pub mod error;
pub mod file_range_reader;
pub mod range_reader;
pub mod resource_loader;
pub mod s3_range_reader;

pub use error::Error;
pub use resource_loader::{ResourceLoader, ResourceLoaderConfig};

#[derive(Clone)]
pub struct BytesWeighter;

impl Weighter<([u8; 16], u64), Bytes> for BytesWeighter {
    fn weight(&self, _key: &([u8; 16], u64), value: &Bytes) -> u64 {
        value.len() as u64
    }
}

#[derive(Clone)]
pub struct ArchiveIndexWeighter;

impl Weighter<String, Arc<ArchiveIndex>> for ArchiveIndexWeighter {
    fn weight(&self, _key: &String, value: &Arc<ArchiveIndex>) -> u64 {
        value.heap_size() as u64
    }
}

#[derive(Clone)]
pub struct ArchiveEntryWeighter;

impl Weighter<([u8; 16], u64), ArchiveEntry> for ArchiveEntryWeighter {
    fn weight(&self, _key: &([u8; 16], u64), value: &ArchiveEntry) -> u64 {
        // Include a rough per-entry overhead so tiny entries aren't free
        value.data.len() as u64 + 64
    }
}
