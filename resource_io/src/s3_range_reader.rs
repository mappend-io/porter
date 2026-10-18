use super::Error;
use super::range_reader::RangeReader;
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use bytes::Bytes;
use metrics::counter;
use tracing::trace;

// Missing objects are not found; anything else is a failed read
fn s3_error<E, R>(operation: &str, e: SdkError<E, R>) -> Error
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
    R: std::fmt::Debug,
{
    let missing = matches!(e.code(), Some("NoSuchKey" | "NotFound"));
    let message = format!("S3 {operation} failed: {e}");
    if missing {
        Error::NotFound(message)
    } else {
        Error::Io(std::io::Error::other(message))
    }
}

pub struct S3RangeReader {
    client: Client,
    bucket: String,
    key: String,
    size: u64,
}

impl S3RangeReader {
    pub async fn new(client: Client, bucket: String, key: String) -> Result<Self, Error> {
        let head_output = client
            .head_object()
            .bucket(&bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| s3_error("HeadObject", e))?;

        let size = head_output.content_length().unwrap_or(0) as u64;

        if size == 0 {
            return Err(Error::Io(std::io::Error::other(
                "S3 object is empty or missing content-length",
            )));
        }

        Ok(Self {
            client,
            bucket,
            key,
            size,
        })
    }

    /// Open an object with a single suffix-range GET instead of a HeadObject:
    /// the response carries the object's size, and the last `tail_len` bytes
    /// come along for free. Archives are read from the end first, so this
    /// saves a round trip when opening one. Returns the reader, and the tail
    /// with its offset in the object.
    pub async fn open_with_tail(
        client: Client,
        bucket: String,
        key: String,
        tail_len: u64,
    ) -> Result<(Self, u64, Bytes), Error> {
        let get_output = client
            .get_object()
            .bucket(&bucket)
            .key(&key)
            .range(format!("bytes=-{tail_len}"))
            .send()
            .await
            .map_err(|e| s3_error("GetObject", e))?;

        // e.g. "bytes 1000-1999/2000"
        let total_from_range = get_output
            .content_range()
            .and_then(|range| range.rsplit_once('/'))
            .and_then(|(_, total)| total.parse::<u64>().ok());

        let tail = get_output
            .body
            .collect()
            .await
            .map_err(|e| {
                Error::Io(std::io::Error::other(format!(
                    "Failed to stream S3 body: {e}"
                )))
            })?
            .into_bytes();

        // Without a Content-Range the whole object came back
        let size = total_from_range.unwrap_or(tail.len() as u64);
        if size == 0 || (tail.len() as u64) > size {
            return Err(Error::Io(std::io::Error::other(
                "S3 object is empty or has an invalid content-range",
            )));
        }

        trace!(
            "Opened s3://{}/{} ({} bytes) with a {} byte tail",
            bucket,
            key,
            size,
            tail.len()
        );
        counter!("porter_s3_range_reads_total").increment(1);
        counter!("porter_s3_range_read_bytes_total").increment(tail.len() as u64);

        let tail_offset = size - tail.len() as u64;
        Ok((
            Self {
                client,
                bucket,
                key,
                size,
            },
            tail_offset,
            tail,
        ))
    }
}

/// Read a whole object with a single GetObject
pub async fn read_object(client: &Client, bucket: &str, key: &str) -> Result<Bytes, Error> {
    let get_output = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| s3_error("GetObject", e))?;

    let bytes = get_output
        .body
        .collect()
        .await
        .map_err(|e| {
            Error::Io(std::io::Error::other(format!(
                "Failed to stream S3 body: {e}"
            )))
        })?
        .into_bytes();

    trace!("Read {} bytes from s3://{}/{}", bytes.len(), bucket, key);
    counter!("porter_s3_range_reads_total").increment(1);
    counter!("porter_s3_range_read_bytes_total").increment(bytes.len() as u64);

    Ok(bytes)
}

#[async_trait]
impl RangeReader for S3RangeReader {
    async fn read_range_async(&self, offset: u64, length: u64) -> Result<Bytes, Error> {
        if length == 0 || offset >= self.size {
            return Ok(Bytes::new());
        }

        // Clamp the read length so we don't try to read past EOF
        let actual_length = std::cmp::min(length, self.size - offset);

        // HTTP ranges are inclusive: if offset is 0 and length is 10, we want bytes 0-9
        let end_offset = offset + actual_length - 1;
        let range_header = format!("bytes={}-{}", offset, end_offset);

        let get_output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .range(range_header)
            .send()
            .await
            .map_err(|e| s3_error("GetObject", e))?;

        let aggregated_bytes = get_output.body.collect().await.map_err(|e| {
            Error::Io(std::io::Error::other(format!(
                "Failed to stream S3 body: {e}"
            )))
        })?;

        trace!(
            //println!(
            "Reading {} bytes from offset {} for s3://{}/{}",
            length, offset, self.bucket, self.key
        );

        counter!("porter_s3_range_reads_total").increment(1);
        counter!("porter_s3_range_read_bytes_total").increment(actual_length);

        Ok(aggregated_bytes.into_bytes())
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
