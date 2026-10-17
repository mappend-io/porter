use crate::Error;
use bytes::Bytes;

const LFH_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
pub const LFH_LEN: usize = 30;

// General purpose flag bit 3: sizes and CRC follow the data in a data descriptor
const FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;

pub const METHOD_STORED: u16 = 0;
pub const METHOD_DEFLATE: u16 = 8;
pub const METHOD_ZSTD: u16 = 93;

/// A file inside a zip/3tz archive, with its data still compressed.
#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    pub compression_method: u16,
    pub crc32: u32,
    pub uncompressed_size: u32,
    pub data: Bytes,
}

impl ArchiveEntry {
    /// Content that isn't compressed, e.g. a file outside of any archive
    pub fn stored(data: Bytes) -> Self {
        let mut crc = flate2::Crc::new();
        crc.update(&data);
        Self {
            compression_method: METHOD_STORED,
            crc32: crc.sum(),
            uncompressed_size: data.len() as u32,
            data,
        }
    }

    /// Parse an entry from `buf`, which starts at the entry's local file header
    /// and must extend at least through the end of its compressed data.
    ///
    /// Returns `Ok(None)` if the entry's filename isn't `expected_path`, i.e. an
    /// index hash collision.
    pub fn parse(buf: &Bytes, expected_path: &str) -> Result<Option<ArchiveEntry>, Error> {
        if buf.len() < LFH_LEN || buf[0..4] != LFH_SIGNATURE {
            return Err(Error::BadArchive(
                "Invalid Local File Header signature".to_string(),
            ));
        }

        let u16_at = |i: usize| u16::from_le_bytes([buf[i], buf[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);

        let flags = u16_at(6);
        let filename_len = u16_at(26) as usize;
        let extra_field_len = u16_at(28) as usize;

        let filename_end = LFH_LEN + filename_len;
        if buf.len() < filename_end || &buf[LFH_LEN..filename_end] != expected_path.as_bytes() {
            return Ok(None);
        }

        if flags & FLAG_DATA_DESCRIPTOR != 0 {
            return Err(Error::BadArchive(format!(
                "Entry {expected_path} uses a data descriptor, which is not supported"
            )));
        }

        // The 3tz spec caps internal file sizes at 4GB, so standard 32-bit fields are sufficient here
        let compressed_size = u32_at(18) as usize;
        let data_start = filename_end + extra_field_len;
        let data_end = data_start + compressed_size;
        if buf.len() < data_end {
            return Err(Error::BadArchive(format!(
                "Entry {expected_path} extends past the end of its read"
            )));
        }

        Ok(Some(ArchiveEntry {
            compression_method: u16_at(8),
            crc32: u32_at(14),
            uncompressed_size: u32_at(22),
            data: buf.slice(data_start..data_end),
        }))
    }

    /// Size of the local file header, name, extra field and data, from the
    /// first [`LFH_LEN`] bytes of an entry
    pub fn total_len_from_header(lfh: &[u8]) -> Result<u64, Error> {
        if lfh.len() < LFH_LEN || lfh[0..4] != LFH_SIGNATURE {
            return Err(Error::BadArchive(
                "Invalid Local File Header signature".to_string(),
            ));
        }
        let filename_len = u16::from_le_bytes([lfh[26], lfh[27]]) as u64;
        let extra_field_len = u16::from_le_bytes([lfh[28], lfh[29]]) as u64;
        let compressed_size = u32::from_le_bytes([lfh[18], lfh[19], lfh[20], lfh[21]]) as u64;
        Ok(LFH_LEN as u64 + filename_len + extra_field_len + compressed_size)
    }

    /// Decompress up to the first `len` bytes of the entry's data, e.g. to sniff
    /// its content type without decompressing all of it.
    pub fn decompress_prefix(&self, len: usize) -> Result<Vec<u8>, Error> {
        use std::io::Read;
        let mut out = Vec::with_capacity(len);
        let result = match self.compression_method {
            METHOD_STORED => {
                out.extend_from_slice(&self.data[..len.min(self.data.len())]);
                Ok(0)
            }
            METHOD_DEFLATE => flate2::read::DeflateDecoder::new(&self.data[..])
                .take(len as u64)
                .read_to_end(&mut out),
            METHOD_ZSTD => zstd::stream::read::Decoder::new(&self.data[..])
                .and_then(|d| d.take(len as u64).read_to_end(&mut out)),
            other => {
                return Err(Error::BadArchive(format!(
                    "Unsupported compression method: {}",
                    other
                )));
            }
        };
        result.map_err(|e| Error::Decompression(e.to_string()))?;
        Ok(out)
    }

    /// Decompress the entry's data. This is CPU-bound, so callers on the async
    /// runtime should run it on the blocking pool.
    pub fn decompress(&self) -> Result<Bytes, Error> {
        match self.compression_method {
            METHOD_STORED => Ok(self.data.clone()),
            METHOD_DEFLATE => {
                use std::io::Read;
                let mut decoder = flate2::read::DeflateDecoder::new(&self.data[..]);
                let mut out = Vec::with_capacity(self.uncompressed_size as usize);
                decoder
                    .read_to_end(&mut out)
                    .map_err(|e| Error::Decompression(e.to_string()))?;
                Ok(Bytes::from(out))
            }
            METHOD_ZSTD => {
                let out = zstd::stream::decode_all(&self.data[..])
                    .map_err(|e| Error::Decompression(e.to_string()))?;
                Ok(Bytes::from(out))
            }
            other => Err(Error::BadArchive(format!(
                "Unsupported compression method: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lfh(
        path: &str,
        method: u16,
        flags: u16,
        crc: u32,
        data: &[u8],
        uncomp: u32,
        extra: &[u8],
    ) -> Vec<u8> {
        let mut v = vec![];
        v.extend_from_slice(&LFH_SIGNATURE);
        v.extend_from_slice(&20u16.to_le_bytes()); // version needed
        v.extend_from_slice(&flags.to_le_bytes());
        v.extend_from_slice(&method.to_le_bytes());
        v.extend_from_slice(&[0; 4]); // mod time/date
        v.extend_from_slice(&crc.to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&uncomp.to_le_bytes());
        v.extend_from_slice(&(path.len() as u16).to_le_bytes());
        v.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        v.extend_from_slice(path.as_bytes());
        v.extend_from_slice(extra);
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn parses_entry_and_ignores_trailing_bytes() {
        let mut buf = lfh("a/b.glb", METHOD_STORED, 0, 0x1234, b"hello", 5, b"xx");
        buf.extend_from_slice(b"next entry");
        let entry = ArchiveEntry::parse(&Bytes::from(buf), "a/b.glb")
            .unwrap()
            .unwrap();
        assert_eq!(entry.compression_method, METHOD_STORED);
        assert_eq!(entry.crc32, 0x1234);
        assert_eq!(entry.uncompressed_size, 5);
        assert_eq!(&entry.data[..], b"hello");
        assert_eq!(&entry.decompress().unwrap()[..], b"hello");
    }

    #[test]
    fn distinguishes_collisions_from_bad_entries() {
        let buf = Bytes::from(lfh("a/b.glb", METHOD_STORED, 0, 0, b"hello", 5, b"xx"));
        assert_eq!(
            ArchiveEntry::total_len_from_header(&buf).unwrap(),
            buf.len() as u64
        );

        // A different name is a hash collision, not an error
        assert!(ArchiveEntry::parse(&buf, "a/c.glb").unwrap().is_none());

        // Truncated data, data descriptors and garbage are errors
        assert!(ArchiveEntry::parse(&buf.slice(..buf.len() - 1), "a/b.glb").is_err());
        let descriptor = Bytes::from(lfh(
            "a/b.glb",
            METHOD_STORED,
            FLAG_DATA_DESCRIPTOR,
            0,
            b"hello",
            5,
            b"",
        ));
        assert!(ArchiveEntry::parse(&descriptor, "a/b.glb").is_err());
        assert!(
            ArchiveEntry::parse(&Bytes::from_static(b"not a header at all, nope"), "a").is_err()
        );
    }

    #[test]
    fn stored_entries_have_a_valid_crc() {
        let entry = ArchiveEntry::stored(Bytes::from_static(b"hello"));
        assert_eq!(entry.crc32, 0x3610a686);
        assert_eq!(entry.uncompressed_size, 5);
        assert_eq!(&entry.decompress().unwrap()[..], b"hello");
    }

    #[test]
    fn decompresses_deflate_and_zstd() {
        use std::io::Write;
        let payload = b"some payload some payload some payload".repeat(10);

        let mut enc = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
        enc.write_all(&payload).unwrap();
        let deflated = enc.finish().unwrap();
        let buf = Bytes::from(lfh(
            "x",
            METHOD_DEFLATE,
            0,
            0,
            &deflated,
            payload.len() as u32,
            b"",
        ));
        let entry = ArchiveEntry::parse(&buf, "x").unwrap().unwrap();
        assert_eq!(entry.decompress().unwrap(), payload);
        assert_eq!(entry.decompress_prefix(10).unwrap(), payload[..10]);

        let zstded = zstd::stream::encode_all(&payload[..], 3).unwrap();
        let buf = Bytes::from(lfh(
            "x",
            METHOD_ZSTD,
            0,
            0,
            &zstded,
            payload.len() as u32,
            b"",
        ));
        let entry = ArchiveEntry::parse(&buf, "x").unwrap().unwrap();
        assert_eq!(entry.decompress().unwrap(), payload);
        assert_eq!(entry.decompress_prefix(10).unwrap(), payload[..10]);
        assert_eq!(entry.decompress_prefix(100_000).unwrap(), payload);
    }
}
