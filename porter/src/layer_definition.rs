use crate::inline_owt_referenced_models;
use crate::s2_utils::{
    cell_id_from_face_level_col_row, face_level_col_row_from_cell_id, s2_rect_to_region,
    s2_token_to_s2_rect, s2_tokens_to_s2_rect,
};
use crate::{
    tiles3d,
    tiles3d::{Asset, Content, RootProperty, Tile},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// TODO:
// - Need to buffer the rects from tokens for synthesized tiles so the BVs are coherent
pub const MIN_ELEV: f64 = -9000.0;
pub const MAX_ELEV: f64 = 9000.0;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerDefinition {
    #[serde(skip)]
    pub id: String,
    pub source_uri_content_template: String,
    pub source_s2_content_package_level: i32,
    pub source_s2_content_min_level: i32,
    pub source_s2_content_max_level: i32,
    pub source_s2_content_extension: String,
    pub source_s2_content_coverage_tokens: Vec<String>,
    pub root_geometric_error: f64,
    pub tileset_root_property: tiles3d::RootProperty,
    pub tileset_extensions_used: Vec<String>,
    pub tileset_extensions_required: Vec<String>,
    pub tileset_metadata: Option<tiles3d::Metadata>,
    pub tileset_schema: Option<tiles3d::Schema>,
    // Arbitrary value, helps cache-busting if the underlying content changes
    pub version: i32,
    #[serde(default)]
    pub base_globe_terrain_uri: Option<String>,
    pub asset_id: i64,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub content_transforms: Vec<ContentTransform>,

    // Like: `dtm/{FACE}/{LEVEL}/{COL}/{ROW}.tif`
    #[serde(default, alias = "elevationPngContent")]
    pub elevation_raster_content: Option<String>,

    // Like: `imagery/{FACE}/{LEVEL}/{COL}/{ROW}.jpg`
    #[serde(default)]
    pub imagery_raster_content: Option<String>,

    // Rasters of class IDs by name, each served over WMTS with its own colors.
    // Not part of the 3D Tiles hash.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub class_rasters: BTreeMap<String, ClassRaster>,
}

impl LayerDefinition {
    pub fn geometric_error_for_level(&self, level: i32) -> f64 {
        let scale = 1.0;
        (self.root_geometric_error / 2f64.powi(level.max(0))) * scale
    }

    pub fn resolve_content_uri_template(&self, content_root_token: &str) -> String {
        self.source_uri_content_template
            .replace("{CONTENT_ROOT_TOKEN}", content_root_token)
    }

    /// The source content for the package holding this cell, if the cell is at
    /// or below the package level
    pub fn content_package_uri(&self, face: u8, level: i32, col: i32, row: i32) -> Option<String> {
        if level < self.source_s2_content_package_level {
            return None;
        }
        let token = cell_id_from_face_level_col_row(face, level, col, row)
            .parent(self.source_s2_content_package_level as u64)
            .to_token();
        Some(self.resolve_content_uri_template(&token))
    }

    pub fn hash(&self) -> String {
        // TODO: This is fragile if metadata fields are populated using any hashmap values.
        // Revisit this: canonicalize it somehow before hashing.
        // Class rasters don't change 3D Tiles responses, so editing their colors
        // keeps 3D Tiles cached. They have their own hash.
        let tiles_only = LayerDefinition {
            class_rasters: BTreeMap::new(),
            ..self.clone()
        };
        let json = serde_json::to_vec(&tiles_only).expect("Serialization for hash failed");
        blake3::hash(&json).to_hex()[..16].to_string()
    }

    /// Hash for a class raster's tile URLs: changes with its source or colors
    pub fn class_raster_hash(&self, name: &str) -> Option<String> {
        let class_raster = self.class_rasters.get(name)?;
        let json = serde_json::to_vec(&(self.hash(), name, class_raster))
            .expect("Serialization for hash failed");
        Some(blake3::hash(&json).to_hex()[..16].to_string())
    }

    pub fn coverage_cell_union(&self) -> s2::cellunion::CellUnion {
        let mut ids = vec![];
        for token in &self.source_s2_content_coverage_tokens {
            ids.push(s2::cellid::CellID::from_token(token));
        }
        let mut union = s2::cellunion::CellUnion(ids);
        union.normalize();
        union
    }

    pub fn root_tileset(&self) -> tiles3d::Tileset {
        let bounding_volume = s2_rect_to_region(
            &s2_tokens_to_s2_rect(&self.source_s2_content_coverage_tokens),
            MIN_ELEV,
            MAX_ELEV,
        );

        tiles3d::Tileset {
            asset: Asset::default(),
            geometric_error: self.geometric_error_for_level(0) * 128.0,
            root: tiles3d::Tile {
                bounding_volume: bounding_volume.clone(),
                geometric_error: self.geometric_error_for_level(0) * 128.0,
                children: vec![],
                content: Some(Content {
                    bounding_volume: None,
                    uri: format!("{}/{}/tileset.json", self.id, self.hash()),
                    root_property: RootProperty::default(),
                }),
                refine: Some(tiles3d::RefineMode::Replace),
                root_property: RootProperty::default(),
            },
            root_property: self.tileset_root_property.clone(),
            extensions_used: self.tileset_extensions_used.clone(),
            extensions_required: self.tileset_extensions_required.clone(),
        }
    }

    pub fn content_coverage_tokens_by_s2_face(&self) -> BTreeMap<u8, Vec<String>> {
        let mut tokens = BTreeMap::new();
        for token in &self.source_s2_content_coverage_tokens {
            let cell_id = s2::cellid::CellID::from_token(token);
            tokens
                .entry(cell_id.face())
                .or_insert(vec![])
                .push(token.clone());
        }
        tokens
    }

    pub fn synthesize_s2_root(&self) -> tiles3d::Tileset {
        let populated_faces = self.content_coverage_tokens_by_s2_face();
        let mut children = vec![];
        for (face, tokens) in populated_faces {
            let bounding_volume =
                s2_rect_to_region(&s2_tokens_to_s2_rect(&tokens), MIN_ELEV, MAX_ELEV);

            let child = tiles3d::Tile {
                bounding_volume: bounding_volume.clone(),
                geometric_error: self.geometric_error_for_level(0) * 64.0,
                children: vec![],
                content: Some(tiles3d::Content {
                    uri: format!("t/{}/{}/{}/{}.json", face, 0, 0, 0),
                    bounding_volume: None,
                    root_property: RootProperty::default(),
                }),
                refine: None,
                root_property: RootProperty::default(),
            };
            children.push(child);
        }

        tiles3d::Tileset {
            asset: Asset::default(),
            extensions_required: self.tileset_extensions_required.clone(),
            extensions_used: self.tileset_extensions_used.clone(),
            root_property: RootProperty::default(),
            geometric_error: self.geometric_error_for_level(0) * 128.0,
            root: Tile {
                bounding_volume: self.root_tileset().root.bounding_volume,
                geometric_error: self.geometric_error_for_level(0) * 128.0,
                children,
                content: None,
                refine: None,
                root_property: RootProperty::default(),
            },
        }
    }

    pub fn synthesize_tileset(&self, face: u8, level: i32, col: i32, row: i32) -> tiles3d::Tileset {
        let cell_id = cell_id_from_face_level_col_row(face, level, col, row);
        let token = cell_id.to_token();
        let bounding_volume = s2_rect_to_region(&s2_token_to_s2_rect(&token), MIN_ELEV, MAX_ELEV);
        let content_token = cell_id
            .parent(self.source_s2_content_package_level as u64)
            .to_token();
        let content_union = self.coverage_cell_union();

        let mut children = vec![];
        let mut content = None;

        if level < self.source_s2_content_min_level {
            if self.base_globe_terrain_uri.is_some() && self.source_s2_content_extension == "glb" {
                content = Some(tiles3d::Content {
                    uri: format!("../../../../bgc/{face}/{level}/{col}/{row}.glb",),
                    bounding_volume: None,
                    root_property: RootProperty::default(),
                });
            }
        } else if level <= self.source_s2_content_max_level {
            content = Some(tiles3d::Content {
                uri: format!(
                    "../../../../c/{content_token}/{face}/{level}/{col}/{row}.{}",
                    self.source_s2_content_extension
                ),
                bounding_volume: None,
                root_property: RootProperty::default(),
            });
        }

        if level < self.source_s2_content_max_level {
            for child_id in cell_id.children() {
                // Skip it if there's no content coverage
                if !content_union.intersects_cellid(&child_id) {
                    continue;
                }
                let child_token = child_id.to_token();
                let (_, child_level, child_col, child_row) =
                    face_level_col_row_from_cell_id(child_id);
                let child = tiles3d::Tile {
                    bounding_volume: s2_rect_to_region(
                        &s2_token_to_s2_rect(&child_token),
                        MIN_ELEV,
                        MAX_ELEV,
                    ),
                    children: vec![],
                    content: Some(Content {
                        bounding_volume: None,
                        uri: format!("../../{child_level}/{child_col}/{child_row}.json"),
                        root_property: RootProperty::default(),
                    }),
                    geometric_error: self.geometric_error_for_level(child_level),
                    refine: None,
                    root_property: RootProperty::default(),
                };
                children.push(child);
            }
        }

        let root = tiles3d::Tile {
            bounding_volume,
            children,
            content,
            geometric_error: self.geometric_error_for_level(level),
            refine: None,
            root_property: RootProperty::default(),
        };

        tiles3d::Tileset {
            asset: tiles3d::Asset::default(),
            extensions_required: self.tileset_extensions_required.clone(),
            extensions_used: self.tileset_extensions_used.clone(),
            root_property: RootProperty::default(),
            geometric_error: root.geometric_error,
            root,
        }
    }
}

/// A content transform, given by name alone or as an object with its options:
/// `"inline_owt_referenced_models"` or
/// `{ "name": "inline_owt_referenced_models", ... }`. Each transform owns its
/// options.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "ContentTransformRepr", into = "ContentTransformRepr")]
pub enum ContentTransform {
    InlineOwtReferencedModels(inline_owt_referenced_models::Options),
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum ContentTransformRepr {
    Name(String),
    WithOptions {
        name: String,
        #[serde(flatten)]
        options: serde_json::Map<String, serde_json::Value>,
    },
}

impl TryFrom<ContentTransformRepr> for ContentTransform {
    type Error = String;

    fn try_from(repr: ContentTransformRepr) -> Result<Self, Self::Error> {
        let (name, options) = match repr {
            ContentTransformRepr::Name(name) => (name, serde_json::Map::new()),
            ContentTransformRepr::WithOptions { name, options } => (name, options),
        };
        let options = serde_json::Value::Object(options);
        match name.as_str() {
            inline_owt_referenced_models::NAME => serde_json::from_value(options)
                .map(Self::InlineOwtReferencedModels)
                .map_err(|e| format!("Invalid options for {name}: {e}")),
            _ => Err(format!("Unknown content transform {name}")),
        }
    }
}

impl From<ContentTransform> for ContentTransformRepr {
    fn from(transform: ContentTransform) -> Self {
        let (name, options) = match &transform {
            ContentTransform::InlineOwtReferencedModels(options) => (
                inline_owt_referenced_models::NAME,
                serde_json::to_value(options),
            ),
        };
        // Without options, keep the plain name, so existing layers hash the same
        match options {
            Ok(serde_json::Value::Object(options)) if !options.is_empty() => Self::WithOptions {
                name: name.to_string(),
                options,
            },
            _ => Self::Name(name.to_string()),
        }
    }
}

/// A raster of 8-bit class IDs, like land use and land cover, served over WMTS
/// as a PNG colored by class: `{ "content": "lulc/{FACE}/{LEVEL}/{COL}/{ROW}.png",
/// "colors": { "10": "#006400", "80": "#0064c8cc" } }`. Classes without a color
/// are transparent.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClassRaster {
    pub content: String,
    pub colors: BTreeMap<u8, ClassColor>,
}

impl ClassRaster {
    pub fn palette(&self) -> terrarium::ClassColors {
        let mut palette = [[0; 4]; 256];
        for (&class, color) in &self.colors {
            palette[class as usize] = color.0;
        }
        palette
    }
}

/// RGBA, written `#rrggbb` or `#rrggbbaa`
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ClassColor(pub [u8; 4]);

impl TryFrom<String> for ClassColor {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let hex = s
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6 || hex.len() == 8)
            .filter(|hex| hex.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| format!("Color {s} is not #rrggbb or #rrggbbaa"))?;
        let mut rgba = [255; 4];
        for (i, channel) in rgba.iter_mut().take(hex.len() / 2).enumerate() {
            *channel = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| format!("Color {s} is not #rrggbb or #rrggbbaa"))?;
        }
        Ok(Self(rgba))
    }
}

impl From<ClassColor> for String {
    fn from(ClassColor([r, g, b, a]): ClassColor) -> Self {
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

impl LayerDefinition {
    /// Checks that need more than one field, or that would otherwise fail
    /// (or panic) at request time
    pub fn validate(&self) -> Result<(), String> {
        let (package, min, max) = (
            self.source_s2_content_package_level,
            self.source_s2_content_min_level,
            self.source_s2_content_max_level,
        );
        if !(0 <= package && package <= min && min <= max && max <= 30) {
            return Err(format!(
                "Content levels must satisfy 0 <= package {package} <= min {min} <= max {max} <= 30"
            ));
        }
        if !matches!(self.source_s2_content_extension.as_str(), "glb" | "geojson") {
            return Err(format!(
                "sourceS2ContentExtension {:?} must be glb or geojson",
                self.source_s2_content_extension
            ));
        }
        // Only file: and s3: sources can be read
        let sources = [
            (
                "sourceUriContentTemplate",
                Some(self.resolve_content_uri_template("0")),
            ),
            ("baseGlobeTerrainUri", self.base_globe_terrain_uri.clone()),
        ];
        for (field, uri) in sources
            .iter()
            .filter_map(|(f, u)| u.as_ref().map(|u| (f, u)))
        {
            let scheme = iri_string::types::UriAbsoluteStr::new(uri)
                .map_err(|e| format!("{field} {uri:?} is not an absolute URI: {e}"))?
                .scheme_str()
                .to_string();
            if !matches!(scheme.as_str(), "file" | "s3") {
                return Err(format!("{field} {uri:?} must be a file: or s3: URI"));
            }
        }
        if let Some(options) = self.inline_owt_referenced_models() {
            options.validate(self.source_s2_content_max_level)?;
        }
        for name in self.class_rasters.keys() {
            let url_safe = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
            if name.is_empty() || !name.chars().all(url_safe) {
                return Err(format!(
                    "Class raster name {name:?} must be letters, digits, _ or -"
                ));
            }
        }
        Ok(())
    }

    // find_map is for when there is more than one transform
    #[allow(clippy::unnecessary_find_map)]
    pub fn inline_owt_referenced_models(&self) -> Option<&inline_owt_referenced_models::Options> {
        self.content_transforms
            .iter()
            .find_map(|transform| match transform {
                ContentTransform::InlineOwtReferencedModels(options) => Some(options),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_transforms_accept_names_and_options() {
        let parse = |json: &str| serde_json::from_str::<Vec<ContentTransform>>(json);

        let by_name = parse(r#"["inline_owt_referenced_models"]"#).unwrap();
        let with_options = parse(r#"[{"name": "inline_owt_referenced_models"}]"#).unwrap();
        assert_eq!(by_name, with_options);
        assert_eq!(
            by_name,
            vec![ContentTransform::InlineOwtReferencedModels(
                inline_owt_referenced_models::Options::default()
            )]
        );

        // Serializes back to the plain name, which keeps layer hashes stable
        assert_eq!(
            serde_json::to_string(&with_options).unwrap(),
            r#"["inline_owt_referenced_models"]"#
        );

        assert!(parse(r#"["no_such_transform"]"#).is_err());
        assert!(parse(r#"[{"name": "inline_owt_referenced_models", "typo": 1}]"#).is_err());
    }

    #[test]
    fn class_raster_colors_parse_and_round_trip() {
        let parse = |json: &str| serde_json::from_str::<ClassRaster>(json);

        let raster =
            parse(r##"{"content": "lulc.png", "colors": {"10": "#006400", "80": "#0064C8cc"}}"##)
                .unwrap();
        let palette = raster.palette();
        assert_eq!(palette[10], [0, 100, 0, 255]);
        assert_eq!(palette[80], [0, 100, 200, 204]);
        assert_eq!(palette[0], [0, 0, 0, 0]);
        assert_eq!(
            serde_json::to_string(&raster).unwrap(),
            r##"{"content":"lulc.png","colors":{"10":"#006400","80":"#0064c8cc"}}"##
        );

        assert!(parse(r##"{"content": "x", "colors": {"256": "#000000"}}"##).is_err());
        assert!(parse(r##"{"content": "x", "colors": {"1": "000000"}}"##).is_err());
        assert!(parse(r##"{"content": "x", "colors": {"1": "#00000"}}"##).is_err());
        assert!(parse(r##"{"content": "x", "colors": {"1": "#gg0000"}}"##).is_err());
        assert!(parse(r##"{"content": "x", "colors": {"1": "#+f0000"}}"##).is_err());
        assert!(parse(r##"{"content": "x", "colors": {}, "typo": 1}"##).is_err());
    }

    #[test]
    fn validate_checks_levels_extension_and_sources() {
        let layer = |edit: &dyn Fn(&mut serde_json::Value)| {
            let mut json = serde_json::json!({
                "sourceUriContentTemplate": "s3://bucket/{CONTENT_ROOT_TOKEN}/terrain.3tz",
                "sourceS2ContentPackageLevel": 7,
                "sourceS2ContentMinLevel": 7,
                "sourceS2ContentMaxLevel": 12,
                "sourceS2ContentExtension": "glb",
                "sourceS2ContentCoverageTokens": ["1"],
                "rootGeometricError": 131072,
                "tilesetRootProperty": {},
                "tilesetExtensionsUsed": [],
                "tilesetExtensionsRequired": [],
                "version": 0,
                "assetId": 1,
            });
            edit(&mut json);
            serde_json::from_value::<LayerDefinition>(json)
                .unwrap()
                .validate()
        };
        let set = |key: &'static str, value: serde_json::Value| {
            move |json: &mut serde_json::Value| json[key] = value.clone()
        };

        assert!(layer(&|_| {}).is_ok());
        assert!(layer(&set("baseGlobeTerrainUri", "file:///data/globe.3tz".into())).is_ok());
        assert!(
            layer(&set(
                "sourceUriContentTemplate",
                "s3://bucket/one.3tz".into()
            ))
            .is_ok()
        );

        assert!(layer(&set("sourceS2ContentMinLevel", 6.into())).is_err());
        assert!(layer(&set("sourceS2ContentMaxLevel", 6.into())).is_err());
        assert!(layer(&set("sourceS2ContentMaxLevel", 31.into())).is_err());
        assert!(layer(&set("sourceS2ContentExtension", "gltf".into())).is_err());
        assert!(
            layer(&set(
                "sourceUriContentTemplate",
                "https://host/x.3tz".into()
            ))
            .is_err()
        );
        assert!(layer(&set("sourceUriContentTemplate", "/data/x.3tz".into())).is_err());
        assert!(layer(&set("baseGlobeTerrainUri", "http://host/globe.3tz".into())).is_err());
    }

    #[test]
    fn class_rasters_have_their_own_hash() {
        let mut layer: LayerDefinition = serde_json::from_value(serde_json::json!({
            "sourceUriContentTemplate": "s3://bucket/{CONTENT_ROOT_TOKEN}/terrain.3tz",
            "sourceS2ContentPackageLevel": 7,
            "sourceS2ContentMinLevel": 7,
            "sourceS2ContentMaxLevel": 12,
            "sourceS2ContentExtension": "glb",
            "sourceS2ContentCoverageTokens": ["1"],
            "rootGeometricError": 131072,
            "tilesetRootProperty": {},
            "tilesetExtensionsUsed": [],
            "tilesetExtensionsRequired": [],
            "version": 0,
            "assetId": 1,
        }))
        .unwrap();
        let tiles_hash = layer.hash();
        assert_eq!(layer.class_raster_hash("lulc"), None);

        let raster = |color: &str| ClassRaster {
            content: "lulc/{FACE}/{LEVEL}/{COL}/{ROW}.png".to_string(),
            colors: BTreeMap::from([(10, ClassColor::try_from(color.to_string()).unwrap())]),
        };
        layer
            .class_rasters
            .insert("lulc".to_string(), raster("#006400"));
        let lulc_hash = layer.class_raster_hash("lulc").unwrap();
        assert_eq!(layer.hash(), tiles_hash);

        layer
            .class_rasters
            .insert("lulc".to_string(), raster("#00ff00"));
        assert_ne!(layer.class_raster_hash("lulc").unwrap(), lulc_hash);
        assert_eq!(layer.hash(), tiles_hash);

        layer.version = 1;
        assert_ne!(layer.hash(), tiles_hash);
        assert_ne!(layer.class_raster_hash("lulc").unwrap(), lulc_hash);

        layer
            .class_rasters
            .insert("lulc/x".to_string(), raster("#00ff00"));
        assert!(layer.validate().is_err());
    }
}
