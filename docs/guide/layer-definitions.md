# Layer definitions

Layer definitions must use identifier-friendly names (i.e. only alphanumeric,
`-` and `_` symbols are allowed). Place layer definitions in a directory (or S3
bucket), one definition per file.

The layer definition describes how the virtual layer exposed by Porter should
locate source content. To accomplish this, several key elements are necessary:

- `sourceUriContentTemplate`: An `s3:` or `file:` URI pointing to backing source
  data. This is a templated string, the `{CONTENT_ROOT_TOKEN}` is replaced by
  the S2 token at the `sourceS2ContentPackageLevel`.
  - See [URI notes](/guide/usage#uri-notes) for more details
- `sourceS2ContentPackageLevel`: All tiled source data must exist at this
  uniform S2 level
- `sourceS2ContentMinLevel`: The lowest level of complete content in source data
- `sourceS2ContentMaxLevel`: The highest level of complete content in source
  data
- `sourceS2ContentExtension`: Either `glb` or `geojson`
- `sourceS2ContentCoverageTokens`: An array of S2 tokens describing the area
  covered by source data. This *could* be each populated S2 L7, but it is better
  to provide a normalized cell union to roll up larger areas with fewer tokens.
  Use `["1", "3", "5", "7", "9", "b"]` to represent the entire globe.
- `baseGlobeTerrainUri`: Optional, only use for terrain layers. This provides
  backfill for lower S2 levels for navigation.
- `rootGeometricError`: Porter does not touch source data until a viewer requests
  it. This hint helps populate the virtual tileset ancestors above the content.
- `tilesetExtensionsRequired`: Set to `["MAXAR_content_geojson"]` if exposing a
  vector dataset, otherwise leave it as an empty array
- `description`: An optional string to include in the layer list.
- `assetId`: For emulation, a numeric ID for this asset. Be sure to use a unique
  value for each layer.
- `contentTransforms`: A list of content transformations to apply. Each entry
  is a transform name, or an object with the name and that transform's options,
  e.g. `{ "name": "inline_owt_referenced_models" }`. Unknown names or options
  are an error. Available transforms:
  - `inline_owt_referenced_models`: Source tiles are GeoJSON points
    referencing models (`OWT_MDL`); each `.glb` tile is served as one model
    combining every referenced model at its point.
    Options:
    - `tileLevel`: Serve tiles at this S2 level instead of the source content
      level (`sourceS2ContentMaxLevel`). Each tile holds the models whose
      points fall in its cell, and Porter builds the tile JSON from the source
      level down from the source points: only cells with models get a tile,
      bounded by their points (padded 200m horizontally for model reach, and
      by each feature's `HGT` vertically when present). Models still load at
      the distance set by `rootGeometricError`, but only near cells that have
      them, and each tile is smaller and arrives sooner. For example,
      `{ "name": "inline_owt_referenced_models", "tileLevel": 14 }` splits L12
      building tiles into up to 16 L14 tiles.
- `elevationRasterContent`: Path to Mapzen Terrarium-encoded RGB PNG or F32 TIFF
  rasters within the content, if present. If defined, a Mapzen Terrarium
  endpoint will be exposed. The tokens `{FACE}`, `{LEVEL}`, `{COL}` and `{ROW}`
  will be substituted. Example: `dtm/{FACE}/{LEVEL}/{COL}/{ROW}.tif`.
- `imageryRasterContent`: Path to JPG rasters within the content, if present. If
  defined, a WMTS Simple endpoint will be exposed. The tokens `{FACE}`, `{LEVEL}`, `{COL}` and
  `{ROW}` will be substituted. Example:
  `imagery/{FACE}/{LEVEL}/{COL}/{ROW}.jpg`.
- `classRasters`: Rasters of 8-bit class IDs within the content, like land use
  and land cover, by name. Each is exposed as its own WMTS Simple endpoint of
  PNG tiles colored by class, so one source can be served with different
  colors for different consumers. Each entry has:
  - `content`: Path to 8-bit grayscale PNG rasters with `.aux.xml` sidecars,
    with the same tokens as `imageryRasterContent`. Example:
    `lulc/{FACE}/{LEVEL}/{COL}/{ROW}.png`.
  - `colors`: Class ID to `#rrggbb` or `#rrggbbaa`. Classes without a color,
    and areas without source data, are transparent.

  Tiles are served at `wmts/{id}/{name}/{hash}/{z}/{x}/{y}.png`, listed in
  `/layers`. The hash changes with the colors or the layer's source, so edits
  reach clients despite the long-lived cache headers. Class rasters are left out
  of the 3D Tiles hash, so editing them keeps cached 3D Tiles valid. Example,
  with two of the OWT `owt_lulc` classes:

  ```json
  "classRasters": {
      "water": {
          "content": "lulc/{FACE}/{LEVEL}/{COL}/{ROW}.png",
          "colors": { "80": "#0064c8", "81": "#00c8ff80" }
      }
  }
  ```

  CesiumJS doesn't need this to show land cover on the 3D terrain: the terrain
  glTF already carries it (see the bundled viewer's `lulc` option in
  [Usage](/guide/usage)).

The remaining fields can be set as described in the sample below and are
reserved for future use.

A sample layer definition:

```json
{
    "description": "A sample terrain layer",
    "sourceUriContentTemplate": "s3://bucket/prefix/{CONTENT_ROOT_TOKEN}/terrain.3tz",
    "sourceS2ContentPackageLevel": 7,
    "sourceS2ContentMinLevel": 7,
    "sourceS2ContentMaxLevel": 12,
    "sourceS2ContentExtension": "glb",
    "sourceS2ContentCoverageTokens": ["1", "3", "5", "7", "9", "b"],
    "baseGlobeTerrainUri": "s3://bucket/prefix/base_globe/terrain.3tz",
    "elevationRasterContent": "dtm/{FACE}/{LEVEL}/{COL}/{ROW}.tif",
    "imageryRasterContent": "imagery/{FACE}/{LEVEL}/{COL}/{ROW}.jpg",
    "rootGeometricError": 131072,
    "tilesetRootProperty": {},
    "tilesetExtensionsUsed": [],
    "tilesetExtensionsRequired": [],
    "tilesetMetadata": {},
    "tilesetSchema": {},
    "version": 0,
    "assetId": 0
}
```

A more complex layer that inlines referenced models:

```json
{
    "description": "A sampler layer with inlined building models",
    "sourceUriContentTemplate": "s3://bucket/prefix/{CONTENT_ROOT_TOKEN}/BuildingPnt.3tz",
    "sourceS2ContentPackageLevel": 7,
    "sourceS2ContentMinLevel": 12,
    "sourceS2ContentMaxLevel": 12,
    "sourceS2ContentExtension": "glb",
    "sourceS2ContentCoverageTokens": ["1", "3", "5", "7", "9", "b"],
    "rootGeometricError": 16384,
    "tilesetRootProperty": {},
    "tilesetExtensionsUsed": [],
    "tilesetExtensionsRequired": [],
    "tilesetMetadata": {},
    "tilesetSchema": {},
    "version": 0,
    "assetId": 1,
    "contentTransforms": ["inline_owt_referenced_models"]
}
```

A directory containing several layers. The layer identifier is derived from the
filename without the `.json` suffix.

```text
$ ls /layers
my-layer.json
my-other-layer.json
```

## Changing layer definitions

Layer definitions are loaded on-demand. They are cached in memory by Porter for
`--layer-definition-ttl` (5m by default). Changing an existing layer definition
means the change will not necessarily get picked up right away if it has
recently been used. This helps reduce the load on the config storage layer and
improve response times.

If a layer has never been accessed, or is a new layer entirely, it will be
picked up right away.
