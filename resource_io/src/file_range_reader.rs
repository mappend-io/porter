use super::Error;
use super::range_reader::RangeReader;
use async_trait::async_trait;
use bytes::Bytes;
use std::fs::File;
use std::sync::Arc;
use tokio::task;

// For "pread" equivalance
#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::FileExt;

pub struct FileRangeReader {
    file: Arc<File>,
    pub size: u64,
}

impl FileRangeReader {
    pub fn new(path: &str) -> Result<FileRangeReader, Error> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        let size = metadata.len();
        Ok(Self {
            file: Arc::new(file),
            size,
        })
    }
}

#[async_trait]
impl RangeReader for FileRangeReader {
    async fn read_range_async(&self, offset: u64, length: u64) -> Result<Bytes, Error> {
        //println!("Read offset={offset}, len={length}");
        // TODO: proper way to handle invalid reads, probably Err instead
        if length == 0 || offset >= self.size {
            return Ok(Bytes::new());
        }

        // Clamp the read length so we don't try to read past EOF
        let actual_length = std::cmp::min(length, self.size - offset) as usize;

        // Clone the Arc to safely move the file handle into the blocking thread
        let file_handle = self.file.clone();

        // Offload the blocking disk I/O to the Tokio blocking pool
        let result = task::spawn_blocking(move || {
            let mut buffer = vec![0u8; actual_length];

            // Perform the stateless, thread-safe read
            #[cfg(unix)]
            let read_result = file_handle.read_at(&mut buffer, offset);

            #[cfg(windows)]
            let read_result = file_handle.seek_read(&mut buffer, offset);

            read_result.map(|bytes_read| {
                // TODO: I think short reads should be an Err instead
                buffer.truncate(bytes_read);
                Bytes::from(buffer)
            })
        })
        .await;

        // Flatten the Result returned by spawn_blocking
        match result {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(io_err)) => Err(Error::Io(io_err)),
            Err(_) => Err(Error::Io(std::io::Error::other(
                "Tokio blocking task panicked or was cancelled",
            ))),
        }
    }

    async fn read_from_end_async(&self, length: u64) -> Result<Bytes, Error> {
        let size = self.size();
        let actual_length = std::cmp::min(length, size);
        let offset = size - actual_length;
        self.read_range_async(offset, actual_length).await
    }

    fn size(&self) -> u64 {
        self.size
    }
}
