use super::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use validator::Validate;

#[skip_serializing_none]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(default, rename_all = "camelCase")]
pub struct Image {
    pub uri: Option<String>,
    pub mime_type: Option<MimeType>,
    pub buffer_view: Option<BufferViewIndex>,
    pub name: Option<String>,
    #[serde(flatten)]
    #[validate(nested)]
    pub property: Property,
}

// TODO: Validate: mimeType must be set if bufferView is defined
// TODO: Validate: bufferView must not be set when uri is defined
