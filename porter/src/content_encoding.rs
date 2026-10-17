//! Serving archive entries without recompressing them.
//!
//! Entries in 3TZ archives are usually already compressed: deflate in older
//! data, zstd in newer data. When the client accepts that encoding, the
//! compressed bytes can be sent as they are, with a matching
//! `Content-Encoding`, instead of decompressing them here and possibly
//! compressing them again on the way out.

use crate::utils::sniff_content_type;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::{BufMut, Bytes, BytesMut};
use metrics::counter;
use resource_io::archive_entry::{ArchiveEntry, METHOD_DEFLATE, METHOD_STORED, METHOD_ZSTD};

// RFC 9659: zstd content-coding recipients need only support windows up to 8MB
const MAX_ZSTD_WINDOW_BYTES: u64 = 8 * 1024 * 1024;

// Enough to sniff glTF magic or the start of a JSON document
const SNIFF_PREFIX_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AcceptedEncodings {
    pub zstd: bool,
    pub gzip: bool,
}

impl AcceptedEncodings {
    /// Which of the encodings we can pass through the client accepts, per
    /// `Accept-Encoding` (RFC 9110 section 12.5.3)
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let mut zstd = None;
        let mut gzip = None;
        let mut wildcard = None;

        for value in headers.get_all(header::ACCEPT_ENCODING) {
            let Ok(value) = value.to_str() else {
                continue;
            };
            for item in value.split(',') {
                let mut parts = item.split(';');
                let coding = parts.next().unwrap_or("").trim().to_ascii_lowercase();
                let accepted = parts
                    .filter_map(|p| p.trim().strip_prefix("q=").or(p.trim().strip_prefix("Q=")))
                    .next()
                    .map(|q| q.trim().parse::<f32>().is_ok_and(|q| q > 0.0))
                    .unwrap_or(true);
                match coding.as_str() {
                    "zstd" => zstd = Some(accepted),
                    "gzip" | "x-gzip" => gzip = Some(accepted),
                    "*" => wildcard = Some(accepted),
                    _ => {}
                }
            }
        }

        // Codings not listed explicitly are covered by a wildcard, if any
        let wildcard = wildcard.unwrap_or(false);
        Self {
            zstd: zstd.unwrap_or(wildcard),
            gzip: gzip.unwrap_or(wildcard),
        }
    }
}

/// Respond with an archive entry, passing its compressed data through when the
/// client accepts the encoding, and decompressing it otherwise.
pub async fn entry_response(
    entry: ArchiveEntry,
    request_headers: &HeaderMap,
) -> Result<Response, StatusCode> {
    let mut headers = HeaderMap::new();
    // Whether the body is encoded depends on the request
    headers.insert(header::VARY, HeaderValue::from_static("accept-encoding"));

    let accepted = AcceptedEncodings::from_headers(request_headers);
    if let Some((encoding, body)) = passthrough_body(&entry, accepted) {
        let prefix = entry
            .decompress_prefix(SNIFF_PREFIX_BYTES)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(sniff_content_type(&prefix)),
        );
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
        counter!("porter_archive_entry_passthrough_total", "encoding" => encoding).increment(1);
        return Ok((headers, body).into_response());
    }

    let bytes = if entry.compression_method == METHOD_STORED {
        entry.data
    } else {
        tokio::task::spawn_blocking(move || entry.decompress())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(sniff_content_type(&bytes)),
    );
    Ok((headers, bytes).into_response())
}

// The entry's data framed for a Content-Encoding the client accepts, if any
fn passthrough_body(
    entry: &ArchiveEntry,
    accepted: AcceptedEncodings,
) -> Option<(&'static str, Bytes)> {
    match entry.compression_method {
        METHOD_ZSTD
            if accepted.zstd
                && zstd_frame_window_size(&entry.data)
                    .is_some_and(|window| window <= MAX_ZSTD_WINDOW_BYTES) =>
        {
            Some(("zstd", entry.data.clone()))
        }
        METHOD_DEFLATE if accepted.gzip => Some(("gzip", gzip_wrap(entry))),
        _ => None,
    }
}

/// Wrap a deflate entry's raw data in a gzip member (RFC 1952), using the CRC
/// and size from the entry's local file header
fn gzip_wrap(entry: &ArchiveEntry) -> Bytes {
    debug_assert_eq!(entry.compression_method, METHOD_DEFLATE);
    let mut buf = BytesMut::with_capacity(entry.data.len() + 18);
    // ID1, ID2, CM=deflate, FLG=none, MTIME=0, XFL=0, OS=unknown
    buf.put_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255]);
    buf.put_slice(&entry.data);
    buf.put_u32_le(entry.crc32);
    buf.put_u32_le(entry.uncompressed_size);
    buf.freeze()
}

/// Window size of a zstd frame (RFC 8878 section 3.1.1), from its header.
/// None if `data` doesn't start with a standard zstd frame, or the frame needs
/// a dictionary, which clients won't have.
fn zstd_frame_window_size(data: &[u8]) -> Option<u64> {
    const MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
    if data.len() < 5 || data[0..4] != MAGIC {
        return None;
    }

    let descriptor = data[4];
    let fcs_flag = descriptor >> 6;
    let single_segment = (descriptor >> 5) & 1 == 1;
    let dictionary_id_flag = descriptor & 3;
    if dictionary_id_flag != 0 {
        return None;
    }

    if !single_segment {
        // Window_Descriptor follows the frame header descriptor
        let window_descriptor = *data.get(5)?;
        let window_log = 10 + (window_descriptor >> 3) as u64;
        let window_base = 1u64 << window_log;
        let window_add = (window_base / 8) * (window_descriptor & 7) as u64;
        return Some(window_base + window_add);
    }

    // Single segment frames have no window descriptor, the window is the
    // frame content size
    let fcs = &data[5..];
    match fcs_flag {
        0 => fcs.first().map(|&b| b as u64),
        1 => fcs
            .get(..2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]) as u64 + 256),
        2 => fcs
            .get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64),
        _ => fcs
            .get(..8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted(value: &str) -> AcceptedEncodings {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_str(value).unwrap(),
        );
        AcceptedEncodings::from_headers(&headers)
    }

    #[test]
    fn parses_accept_encoding() {
        let both = AcceptedEncodings {
            zstd: true,
            gzip: true,
        };
        let gzip = AcceptedEncodings {
            zstd: false,
            gzip: true,
        };
        let zstd = AcceptedEncodings {
            zstd: true,
            gzip: false,
        };
        assert_eq!(accepted("gzip, deflate, br, zstd"), both);
        assert_eq!(accepted("gzip, deflate, br"), gzip);
        assert_eq!(accepted("ZSTD;q=0.5, gzip;q=0"), zstd);
        assert_eq!(accepted("x-gzip"), gzip);
        assert_eq!(accepted("*"), both);
        assert_eq!(accepted("zstd;q=0, *;q=0.1"), gzip);
        assert_eq!(accepted("identity"), AcceptedEncodings::default());
        assert_eq!(
            AcceptedEncodings::from_headers(&HeaderMap::new()),
            AcceptedEncodings::default()
        );
    }

    #[test]
    fn gzip_wrapped_deflate_decodes() {
        use std::io::{Read, Write};
        let payload = b"glTF and some more bytes that compress, compress, compress".repeat(20);
        let mut enc = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
        enc.write_all(&payload).unwrap();
        let mut crc = flate2::Crc::new();
        crc.update(&payload);
        let entry = ArchiveEntry {
            compression_method: METHOD_DEFLATE,
            crc32: crc.sum(),
            uncompressed_size: payload.len() as u32,
            data: Bytes::from(enc.finish().unwrap()),
        };

        let mut out = vec![];
        flate2::read::GzDecoder::new(&gzip_wrap(&entry)[..])
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn reads_zstd_window_sizes() {
        // Small inputs compressed in one shot are single segment: window == content size
        let small = zstd::bulk::compress(&b"hello hello hello"[..], 3).unwrap();
        assert_eq!(zstd_frame_window_size(&small), Some(17));

        // Streamed without a pledged size: window comes from the descriptor
        let mut enc = zstd::stream::write::Encoder::new(vec![], 19).unwrap();
        enc.include_contentsize(false).unwrap();
        std::io::Write::write_all(&mut enc, &[7u8; 100_000]).unwrap();
        let streamed = enc.finish().unwrap();
        let window = zstd_frame_window_size(&streamed).unwrap();
        assert!(window.is_power_of_two() || window.is_multiple_of(1024));
        assert!(window <= MAX_ZSTD_WINDOW_BYTES);

        // Window_Descriptor exponent 17 => 2^27 = 128MiB, too large for browsers
        let large = [0x28, 0xb5, 0x2f, 0xfd, 0x00, 17 << 3, 0, 0];
        assert_eq!(zstd_frame_window_size(&large), Some(1 << 27));

        // Dictionary frames, and anything that isn't a zstd frame, can't pass through
        assert_eq!(
            zstd_frame_window_size(&[0x28, 0xb5, 0x2f, 0xfd, 0x01, 0, 0]),
            None
        );
        assert_eq!(zstd_frame_window_size(b"glTF...."), None);
    }

    #[test]
    fn passes_through_only_accepted_encodings() {
        let zstd_entry = ArchiveEntry {
            compression_method: METHOD_ZSTD,
            crc32: 0,
            uncompressed_size: 17,
            data: Bytes::from(zstd::stream::encode_all(&b"hello hello hello"[..], 3).unwrap()),
        };
        let all = AcceptedEncodings {
            zstd: true,
            gzip: true,
        };
        assert_eq!(passthrough_body(&zstd_entry, all).unwrap().0, "zstd");
        assert!(
            passthrough_body(
                &zstd_entry,
                AcceptedEncodings {
                    zstd: false,
                    gzip: true
                }
            )
            .is_none()
        );

        let stored = ArchiveEntry::stored(Bytes::from_static(b"{}"));
        assert!(passthrough_body(&stored, all).is_none());
    }
}
