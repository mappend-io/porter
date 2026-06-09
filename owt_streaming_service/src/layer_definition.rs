use crate::s2_utils::*;
use crate::{
    tiles3d,
    tiles3d::{Asset, Content, RootProperty, Tile},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// TODO:
// - Need to buffer the rects from tokens for synthesized tiles so the BVs are coherent
const MIN_ELEV: f64 = -9000.0;
const MAX_ELEV: f64 = 9000.0;

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
    pub content_transforms: BTreeSet<String>,

    // Like: `dtm/{FACE}/{LEVEL}/{COL}/{ROW}.tif`
    #[serde(default, alias = "elevationPngContent")]
    pub elevation_raster_content: Option<String>,
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

    pub fn hash(&self) -> String {
        // TODO: This is fragile if metadata fields are populated using any hashmap values.
        // Revisit this: canonicalize it somehow before hashing.
        let json = serde_json::to_vec(self).expect("Serialization for hash failed");
        blake3::hash(&json).to_hex()[..16].to_string()
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
