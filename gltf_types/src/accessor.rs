use super::utils;
use super::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::{Validate, ValidationError};

#[skip_serializing_none]
#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
#[validate(schema(function = "validate_accessor"))]
pub struct Accessor {
    /// The index of the buffer view. When undefined, the accessor **MUST** be
    /// initialized with zeros; `sparse` property or extensions **MAY** override
    /// zeros with actual values.
    #[serde(default)]
    pub buffer_view: Option<BufferViewIndex>,

    /// The offset relative to the start of the buffer view in bytes. This
    /// **MUST** be a multiple of the size of the component datatype. This
    /// property **MUST NOT** be defined when `bufferView` is undefined.
    #[serde(default)]
    pub byte_offset: Option<u32>,

    /// The datatype of the accessor's components. UNSIGNED_INT type **MUST
    /// NOT** be used for any accessor that is not referenced by
    /// `mesh.primitive.indices`.
    pub component_type: ComponentType,

    /// Specifies whether integer data values are normalized (`true`) to [0, 1]
    /// (for unsigned types) or to [-1, 1] (for signed types) when they are
    /// accessed. This property **MUST NOT** be set to `true` for accessors with
    /// `FLOAT` or `UNSIGNED_INT` component type.
    #[serde(default, skip_serializing_if = "utils::is_false")]
    pub normalized: bool,

    /// The number of elements referenced by this accessor, not to be confused
    /// with the number of bytes or number of components.
    #[validate(range(min = 1))]
    pub count: u32,

    /// Specifies if the accessor's elements are scalars, vectors, or matrices.
    pub r#type: Type,

    /// Maximum value of each component in this accessor. Array elements
    /// **MUST** be treated as having the same data type as accessor's
    /// `componentType`. Both `min` and `max` arrays have the same length. The
    /// length is determined by the value of the `type` property; it can be 1,
    /// 2, 3, 4, 9, or 16.
    ///
    /// `normalized` property has no effect on array values: they always correspond
    /// to the actual values stored in the buffer. When the accessor is sparse,
    /// this property **MUST** contain maximum values of accessor data with sparse
    /// substitution applied.
    #[serde(default)]
    #[validate(length(min = 1, max = 16))]
    pub max: Option<Vec<f32>>,

    /// Minimum value of each component in this accessor. Array elements
    /// **MUST** be treated as having the same data type as accessor's
    /// `componentType`. Both `min` and `max` arrays have the same length. The
    /// length is determined by the value of the `type` property; it can be 1,
    /// 2, 3, 4, 9, or 16.
    ///
    /// `normalized` property has no effect on array values: they always correspond
    /// to the actual values stored in the buffer. When the accessor is sparse,
    /// this property **MUST** contain minimum values of accessor data with sparse
    /// substitution applied.
    #[serde(default)]
    #[validate(length(min = 1, max = 16))]
    pub min: Option<Vec<f32>>,

    // TODO: pub sparse: ...
    #[serde(default)]
    pub name: Option<String>,

    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

impl Accessor {
    pub fn byte_offset(&self) -> u32 {
        self.byte_offset.unwrap_or(0)
    }
}

pub fn validate_accessor(_accessor: &Accessor) -> Result<(), ValidationError> {
    // byte_offset needs buffer_view
    // byte_offset must be a multiple of compnent datatype size
    // min.len() == max.len() depends on accessor.type
    Ok(())
}
