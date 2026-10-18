use crate::tiles3d;

/// Whether `face`, `level`, `col` and `row` name an S2 cell. Out-of-range
/// values make nonsense cell IDs, or panic in the shifts below.
pub fn is_valid_cell(face: u8, level: i32, col: i32, row: i32) -> bool {
    face < 6
        && (0..=30).contains(&level)
        && (0..1i64 << level).contains(&i64::from(col))
        && (0..1i64 << level).contains(&i64::from(row))
}

pub fn cell_id_from_face_level_col_row(
    face: u8,
    level: i32,
    col: i32,
    row: i32,
) -> s2::cellid::CellID {
    let i = (col as u64) << (30 - level);
    let j = (row as u64) << (30 - level);
    let leaf_id = s2::cellid::CellID::from_face_ij(face, i as i32, j as i32);
    leaf_id.parent(level as u64)
}

pub use terrarium::face_level_col_row_from_cell_id;

pub fn s2_rect_to_region(
    rect: &s2::rect::Rect,
    min_height: f64,
    max_height: f64,
) -> tiles3d::BoundingVolume {
    tiles3d::BoundingVolume::from_lat_lng_elev_degrees(
        rect.lo().lng.deg(),
        rect.lo().lat.deg(),
        rect.hi().lng.deg(),
        rect.hi().lat.deg(),
        min_height,
        max_height,
    )
}

pub fn s2_token_to_s2_rect(token: &str) -> s2::rect::Rect {
    let cell_id = s2::cellid::CellID::from_token(token);
    let cell = s2::cell::Cell::from(cell_id);
    cell.rect_bound()
}

pub fn s2_tokens_to_s2_rect(tokens: &Vec<String>) -> s2::rect::Rect {
    let mut rect = s2::rect::Rect::empty();
    for token in tokens {
        rect = rect.union(&s2_token_to_s2_rect(token));
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_bounded_by_face_and_level() {
        assert!(is_valid_cell(0, 0, 0, 0));
        assert!(is_valid_cell(5, 30, (1 << 30) - 1, 0));
        assert!(is_valid_cell(1, 12, 3410, 3273));
        assert!(!is_valid_cell(6, 0, 0, 0));
        assert!(!is_valid_cell(0, 31, 0, 0));
        assert!(!is_valid_cell(0, -1, 0, 0));
        assert!(!is_valid_cell(0, 5, -1, 0));
        assert!(!is_valid_cell(0, 5, 0, 32));
        assert!(!is_valid_cell(0, 3, 999, 0));

        // Valid cells round-trip through the cell ID
        let cell = cell_id_from_face_level_col_row(1, 12, 3410, 3273);
        assert_eq!(face_level_col_row_from_cell_id(cell), (1, 12, 3410, 3273));
    }
}
