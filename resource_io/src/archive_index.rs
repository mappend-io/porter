use crate::Error;
use crate::range_reader::RangeReader;
use bytemuck::{Pod, Zeroable};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct IndexEntry {
    pub path_md5: [u8; 16],
    pub offset: u64,
}

pub struct ArchiveIndex {
    pub entries: Vec<IndexEntry>,
}

impl ArchiveIndex {
    pub fn hash_path(path: &str) -> [u8; 16] {
        md5::compute(path).0
    }

    pub fn md5_compare(a: &[u8; 16], b: &[u8; 16]) -> std::cmp::Ordering {
        // TODO: clean up unwraps
        let hi_a = u64::from_le_bytes(a[0..8].try_into().unwrap());
        let hi_b = u64::from_le_bytes(b[0..8].try_into().unwrap());

        if hi_a != hi_b {
            hi_a.cmp(&hi_b)
        } else {
            let low_a = u64::from_le_bytes(a[8..16].try_into().unwrap());
            let low_b = u64::from_le_bytes(b[8..16].try_into().unwrap());
            low_a.cmp(&low_b)
        }
    }

    pub fn from_unsorted_entries(mut entries: Vec<IndexEntry>) -> Self {
        entries.sort_unstable_by(|a, b| Self::md5_compare(&a.path_md5, &b.path_md5));
        Self { entries }
    }

    pub fn from_raw_bytes(raw_bytes: &[u8]) -> Self {
        Self {
            entries: bytemuck::allocation::pod_collect_to_vec(raw_bytes),
        }
    }

    // NOTE: This isn't handling md5 collisions. To do that properly would mean
    // emitting a vector of candidates, and the caller would have to walk through
    // them and find which candidate matched the filename from the zip header.
    pub fn find_offset(&self, path_md5: &[u8; 16]) -> Option<u64> {
        self.entries
            .binary_search_by(|e| Self::md5_compare(&e.path_md5, path_md5))
            .ok()
            .map(|index| self.entries[index].offset)
    }

    pub async fn from_3tz_range_reader<R: RangeReader + ?Sized>(
        reader: &R,
    ) -> Result<ArchiveIndex, Error> {
        const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
        const CD_ENTRY_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
        const LFH_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
        const INDEX_FILENAME: &str = "@3dtilesIndex1@";
        const ZIP64_EOCD_LOCATOR_SIZE: usize = 20;
        const ZIP64_EOCD_LOCATOR_SIG: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
        const ZIP64_EOCD_RECORD_SIG: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];

        let file_size = reader.size();

        // Find EOCD: Max ZIP comment length is 65535, plus 22 bytes for the EOCD record
        let footer_len = std::cmp::min(file_size, 65535 + 22);
        let footer = reader.read_from_end_async(footer_len).await?;

        let eocd_pos_in_footer = footer
            .windows(4)
            .rposition(|w| w == EOCD_SIGNATURE)
            .ok_or_else(|| Error::BadArchive("EOCD signature not found".to_string()))?;

        let eocd = &footer[eocd_pos_in_footer..];
        if eocd.len() < 22 {
            return Err(Error::BadArchive("EOCD record too short".to_string()));
        }

        let mut cd_size: u64 = u32::from_le_bytes(eocd[12..16].try_into().unwrap()) as u64;
        let mut cd_offset: u64 = u32::from_le_bytes(eocd[16..20].try_into().unwrap()) as u64;

        // Check for Zip64 Locator (immediately precedes the standard EOCD)
        let eocd_absolute_pos = file_size - (footer_len - eocd_pos_in_footer as u64);

        if eocd_absolute_pos >= ZIP64_EOCD_LOCATOR_SIZE as u64 {
            let locator_offset = eocd_absolute_pos - ZIP64_EOCD_LOCATOR_SIZE as u64;
            let locator_bytes = reader
                .read_range_async(locator_offset, ZIP64_EOCD_LOCATOR_SIZE as u64)
                .await?;

            if locator_bytes[0..4] == ZIP64_EOCD_LOCATOR_SIG {
                let zip64_eocd_offset =
                    u64::from_le_bytes(locator_bytes[8..16].try_into().unwrap());
                let zip64_record = reader.read_range_async(zip64_eocd_offset, 56).await?;

                if zip64_record[0..4] == ZIP64_EOCD_RECORD_SIG {
                    cd_size = u64::from_le_bytes(zip64_record[40..48].try_into().unwrap());
                    cd_offset = u64::from_le_bytes(zip64_record[48..56].try_into().unwrap());
                }
            }
        }

        // Read the last CD Entry
        // The last CD entry could theoretically be large due to extra fields/comments,
        // so reading a generous chunk ensures we capture it all
        let cd_read_len = std::cmp::min(cd_size, 65536 + 46);
        let cd_data = reader
            .read_range_async(cd_offset + cd_size - cd_read_len, cd_read_len)
            .await?;

        let last_entry_pos = cd_data
            .windows(4)
            .rposition(|w| w == CD_ENTRY_SIGNATURE)
            .ok_or_else(|| Error::BadArchive("Last CD entry not found".to_string()))?;

        let last_entry = &cd_data[last_entry_pos..];
        if last_entry.len() < 46 {
            return Err(Error::BadArchive("Last CD entry is too short".to_string()));
        }

        let filename_len = u16::from_le_bytes(last_entry[28..30].try_into().unwrap()) as usize;
        let extra_field_len = u16::from_le_bytes(last_entry[30..32].try_into().unwrap()) as usize;

        if last_entry.len() < 46 + filename_len + extra_field_len {
            return Err(Error::BadArchive("Incomplete CD entry data".to_string()));
        }

        // Validate the filename to ensure it's the expected 3D Tiles index
        let filename_bytes = &last_entry[46..46 + filename_len];
        if filename_bytes != INDEX_FILENAME.as_bytes() {
            return Err(Error::BadArchive(format!(
                "Expected index file '{}', but found '{}'",
                INDEX_FILENAME,
                String::from_utf8_lossy(filename_bytes)
            )));
        }

        let compressed_size_u32 = u32::from_le_bytes(last_entry[20..24].try_into().unwrap());
        let uncompressed_size_u32 = u32::from_le_bytes(last_entry[24..28].try_into().unwrap());
        let header_offset_u32 = u32::from_le_bytes(last_entry[42..46].try_into().unwrap());

        // Defaults before potentially overriding via Zip64 extra field
        let mut data_size = compressed_size_u32 as u64;
        let mut local_header_offset = header_offset_u32 as u64;

        // Extract offset and size (Handling Zip64 Extra Fields correctly)
        let extra_field = &last_entry[46 + filename_len..46 + filename_len + extra_field_len];
        let mut i = 0;

        while i + 4 <= extra_field.len() {
            let tag = u16::from_le_bytes(extra_field[i..i + 2].try_into().unwrap());
            let size = u16::from_le_bytes(extra_field[i + 2..i + 4].try_into().unwrap()) as usize;

            if i + 4 + size > extra_field.len() {
                break;
            }

            if tag == 0x0001 {
                // Zip64 tag
                let mut data_idx = i + 4;

                // Per the ZIP specification, values in the Zip64 extra field ONLY exist
                // if their 32-bit counterpart in the standard header is 0xFFFFFFFF
                if uncompressed_size_u32 == 0xFFFFFFFF {
                    data_idx += 8; // Skip uncompressed size
                }
                if compressed_size_u32 == 0xFFFFFFFF {
                    data_size =
                        u64::from_le_bytes(extra_field[data_idx..data_idx + 8].try_into().unwrap());
                    data_idx += 8;
                }
                if header_offset_u32 == 0xFFFFFFFF {
                    local_header_offset =
                        u64::from_le_bytes(extra_field[data_idx..data_idx + 8].try_into().unwrap());
                }
                break;
            }
            i += 4 + size;
        }

        // Read Local File Header (LFH) to skip its metadata and get to the raw bytes
        let lfh_bytes = reader.read_range_async(local_header_offset, 30).await?;
        if lfh_bytes.len() < 30 || lfh_bytes[0..4] != LFH_SIGNATURE {
            return Err(Error::BadArchive(
                "Invalid Local File Header signature".to_string(),
            ));
        }

        let lfh_n = u16::from_le_bytes(lfh_bytes[26..28].try_into().unwrap()) as u64;
        let lfh_m = u16::from_le_bytes(lfh_bytes[28..30].try_into().unwrap()) as u64;

        // 6. Read Raw Bytes of the index
        let data_start = local_header_offset + 30 + lfh_n + lfh_m;
        let raw_bytes = reader.read_range_async(data_start, data_size).await?;

        Ok(ArchiveIndex::from_raw_bytes(&raw_bytes))
    }
}
