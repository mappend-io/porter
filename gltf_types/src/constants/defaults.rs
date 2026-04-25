use crate::AlphaMode;

pub fn default_alpha_mode() -> AlphaMode {
    AlphaMode::Opaque
}

pub fn is_default_alpha_mode(value: &AlphaMode) -> bool {
    *value == default_alpha_mode()
}

pub fn default_alpha_cutoff() -> f32 {
    0.5
}

pub fn is_default_alpha_cutoff(value: &f32) -> bool {
    *value == default_alpha_cutoff()
}

pub fn default_emissive_factor() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}

pub fn is_default_emissive_factor(value: &[f32; 3]) -> bool {
    *value == default_emissive_factor()
}

pub fn default_rotation() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

pub fn is_default_rotation(value: &[f32; 4]) -> bool {
    *value == default_rotation()
}

pub fn default_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

pub fn is_default_scale(value: &[f32; 3]) -> bool {
    *value == default_scale()
}

pub fn default_translation() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}

pub fn is_default_translation(value: &[f32; 3]) -> bool {
    *value == default_translation()
}
