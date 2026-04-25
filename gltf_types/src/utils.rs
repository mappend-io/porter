use std::hash::Hash;
use validator::ValidationError;

pub fn is_false(b: &bool) -> bool {
    !b
}

pub fn validate_unique_ids<T: Hash + Eq>(items: &Vec<T>) -> Result<(), ValidationError> {
    let mut seen = std::collections::HashSet::new();
    for item in items {
        if !seen.insert(item) {
            let mut err = ValidationError::new("DUPLICATE_ID");
            err.message = Some("Ids in this array must be unique".into());
            return Err(err);
        }
    }
    Ok(())
}
