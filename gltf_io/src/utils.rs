use anyhow::{Context, Result};
use iri_string::types::{UriAbsoluteStr, UriAbsoluteString, UriReferenceStr};

pub fn resolve_uri(
    base_uri: &UriAbsoluteStr,
    reference_uri: &UriReferenceStr,
) -> Result<UriAbsoluteString> {
    reference_uri
        .resolve_against(base_uri)
        .and_normalize()
        .to_string()
        .try_into()
        .with_context(|| {
            format!(
                "Could not create absolute uri from base={}, reference={}",
                base_uri, reference_uri
            )
        })
}

pub fn is_model_glb(uri: &UriAbsoluteStr, bytes: &bytes::Bytes) -> bool {
    uri.path_str().ends_with(".glb") || bytes.starts_with(b"glTF")
}
