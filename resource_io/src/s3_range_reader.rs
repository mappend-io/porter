use super::Error;
use super::range_reader::RangeReader;
use async_trait::async_trait;
use aws_sdk_s3::Client;
use bytes::Bytes;
use tracing::trace;

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
            .map_err(|e| Error::Io(std::io::Error::other(format!("S3 HeadObject failed: {e}"))))?;

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
            .map_err(|e| Error::Io(std::io::Error::other(format!("S3 GetObject failed: {e}"))))?;

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
