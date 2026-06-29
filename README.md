# OWT Streaming Service (OSS)

OSS is a 3D Tiles 1.1 tile server that exposes virtual tilesets backed by S3 or
local filesystem content, organized via S2 cell tokens. The source tilesets may
be stored in 3TZ archives. OSS serves tilesets over HTTP for use with
CesiumJS, Cesium for Unreal, Cesium for Unity, and other 3D Tiles consumers,
and includes a bundled CesiumJS viewer for quick inspection.

It also transforms data on the fly to expose it via compatibility endpoints for
non-3D Tiles consumers. Today, Mapzen Terrarium RGB elevation and WMTS Simple
imagery layers are exposed.

3D Tiles glTF content can also be transformed on the fly before it sent to the
consumer.

## Quick start

- Create [layer definitions](#Layer-definitions), one per file, at `/path/to/layers`
- Run `owt_streaming_service --layer-config-uri file:///path/to/layers/`
- Open `http://localhost:3200/?layers=my-layer,my-other-layer` to use the [bundled viewer](#Bundled-viewer)
- OR point CesiumJS, Cesium Native or other 3D Tiles consumers to `http://localhost:3200/my-layer`
- Open `http://localhost:3200/layers` to discover layers and endpoints

## Command line reference

```text
Usage: owt_streaming_service [OPTIONS] --layer-config-uri <LAYER_CONFIG_URI>

Options:
      --log-level <LOG_LEVEL>
          Log level [env: RUST_LOG=] [default: owt_streaming_service=info]
      --pretty-log
          Use pretty logging instead of JSON [env: PRETTY_LOG=true]
      --listen-addr <LISTEN_ADDR>
          Listen address [env: LISTEN_ADDR=] [default: 0.0.0.0:3200]
      --base-url <BASE_URL>
          Public base url [env: BASE_URL=]
      --cors-origin <CORS_ORIGIN>
          Allow CORS from a specific origin, or "*" for any [env: CORS_ORIGIN=*]
      --metrics-listen-addr <METRICS_LISTEN_ADDR>
          Prometheus metrics listen address [env: METRICS_LISTEN_ADDR=]
      --layer-config-uri <LAYER_CONFIG_URI>
          Location of layer configuration JSON documents [env: LAYER_CONFIG_URI=]
      --layer-definition-ttl <LAYER_DEFINITION_TTL>
          [env: LAYER_DEFINITION_TTL=] [default: 5m]
      --block-cache-size <BLOCK_CACHE_SIZE>
          [env: BLOCK_CACHE_SIZE=] [default: 2GiB]
  -h, --help
          Print help
```

## URI notes

Both filesystem and S3 paths are supported, but in all cases full URIs must be
supplied. Raw filesystem paths are never allowed to be passed to
`--layer-config-uri` or in `sourceUriContentTemplate`.

Valid URIs:

- `file:///path/to/layers/` (note trailing slash indicates this is a directory)
- `s3://bucket/my-prefix/layers/` (note trailing slash)

Invalid URIs:

- `/path/to/layers/`
- `/path/to/layers` (technically valid, but won't do what is expected, add
  trailing `/`)

## Configuration

The options named in the usage section above may be specified on the command
line, the environment or a `.env` file from the current working directory.

The syntax for a `.env` file consists of key-value pairs. For example:

```text
LISTEN_ADDR=0.0.0.0:3200
LAYER_CONFIG_URI=file:///path/to/layers/
# Or, if using S3:
#LAYER_CONFIG_URI=s3://my-bucket/prefix/layers/
METRICS_LISTEN_ADDR=0.0.0.0:9000
```

Note that while options may be provided several ways, the precedence is
(highest-first):

- Explicit command line options
- Environment variables
- `.env` variables

## Bundled CesiumJS viewer

OSS includes a bundled CesiumJS environment. Pass query parameters in the
URL to load one or more layers:

```text
http://localhost:3200/?layers=my-layer,my-other-layer
```

NOTE: When a base globe is supplied in a layer configuration to supply backfill,
add a query parameter to the viewer URL `noglobe` to prevent the default imagery
from interfering.

```text
http://localhost:3200/?layers=my-layer,my-other-layer&noglobe
```

## Use with CesiumJS, Cesium Native and other 3D Tiles tooling

OSS exposes standard 3D Tiles 1.1 tilesets over HTTP. To use with CesiumJS,
for example:

```js
async function loadLayer(id) {
  const tileset = await Cesium3DTileset.fromUrl(`http://localhost:3200/${id}`);
  viewer.scene.primitives.add(tileset);
  return tileset;
}
```

## Mapzen Terrarium endpoint

If `elevationRasterContent` is provided in a layer, a Mapzen Terrarium endpoint
will be exposed at:

```text
http://localhost:3200/terrarium/my-layer/{z}/{x}/{y}.png
```

The images are compatible with [Terrarium RGB
encoding](https://github.com/tilezen/joerd/blob/master/docs/formats.md#terrarium)
in Web Mercator projection (EPSG:3857).

## WMTS Simple imagery endpoint

If `imageryRasterContent` is provided in a layer, a WMTS Simple imagery endpoint
will be exposed at:

```text
http://localhost:3200/wmts/my-layer/{z}/{x}/{y}.jpg
```

The images are 256x256px in Web Mercator projection (EPSG:3857).

## Bundled MaplibreGL JS viewer for Mapzen Terrarium/WMTS imagery tiles

OSS includes a bundled MaplibreGL environment. Elevation and imagery data may be
previewed using the URL format:

```text
http://localhost:3200/terrarium_viewer/my-layer
```

## Layer definitions

Layer definitions must use identifier-friendly names (i.e. only alphanumeric,
`-` and `_` symbols are allowed). Place layer definitions in a directory (or S3
bucket), one definition per file.

The layer definition describes how the virtual layer exposed by OSS should
locate source content. To accomplish this, several key elements are necessary:

- `sourceUriContentTemplate`: An `s3:` or `file:` URI pointing to backing source
  data. This is a templated string, the `{CONTENT_ROOT_TOKEN}` is replaced by
  the S2 token at the `sourceS2ContentLevel`.
  - See [URI notes](#URI-notes) for more details
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
- `rootGeometricError`: OSS does not touch source data until a viewer requests
  it. This hint helps populate the virtual tileset ancestors above the content.
- `tilesetExtensionsRequired`: Set to `["MAXAR_content_geojson"]` if exposing a
  vector dataset, otherwise leave it as an empty array
- `description`: An optional string to include in the layer list.
- `assetId`: For emulation, a numeric ID for this asset. Be sure to use a unique
  value for each layer.
- `contentTransforms`: A list of content transformations to apply
- `elevationRasterContent`: Path to Mapzen Terrarium-encoded RGB PNG or F32 TIFF
  rasters within the content, if present. If defined, a Mapzen Terrarium
  endpoint will be exposed. The tokens `{FACE}`, `{LEVEL}`, `{COL}` and `{ROW}`
  will be substituted. Example: `dtm/{FACE}/{LEVEL}/{COL}/{ROW}.tif`.
- `imageryRasterContent`: Path to JPG rasters within the content, if present. If
  defined, a WMTS Simple endpoint. The tokens `{FACE}`, `{LEVEL}`, `{COL}` and
  `{ROW}` will be substituted. Example:
  `imagery/{FACE}/{LEVEL}/{COL}/{ROW}.jpg`.

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

Layer definitions are loaded on-demand. They are cached in memory by OSS
for `--layer-definition-ttl` (5m by default). Changing an existing layer
definition means the change will not necessarily get picked up right away if it
has recently be used. This helps reduce the load on the config storage layer and
improve response times.

If a layer has never been accessed, or is a new layer entirely, it will be
picked up right away.
