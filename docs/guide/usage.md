# Usage

## Quick start

### Create a layer definition file

Porter needs at least one [layer definition](/guide/layer-definitions) to be useful. A layer definition describes where Porter can locate source content, what area it covers, etc.

### Run Porter

```bash
porter --layer-config-uri file:///some/path/to/layers/
```

### Use one of Porter's built-in previewers to inspect the layer

- Use the bundled CesiumJS viewer: http://localhost:3200/?layer=my-layer 
- Or MapLibreGL JS: http://localhost:3200/terrarium_viewer/my-layer

### Use a 3D Tiles compatible tool to access the data

Point your own CesiumJS, Cesium Native or other 3D Tiles tooling to http://localhost:3200/my-layer.

### Programatically discover available layers

Use `http://localhost:3200/layers` to discover all known layers and endpoints

## Command line reference

```text
Usage: porter [OPTIONS] --layer-config-uri <LAYER_CONFIG_URI>

Options:
      --log-level <LOG_LEVEL>
          Log level [env: RUST_LOG=] [default: porter=info]
      --pretty-log
          Use pretty logging instead of JSON [env: PRETTY_LOG=] [default: false]
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
      --tls-cert <TLS_CERT>
          TLS certificate file path [env: TLS_CERT=]
      --tls-key <TLS_KEY>
          TLS private key file path [env: TLS_KEY=]
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

Porter includes a bundled CesiumJS environment. Pass query parameters in the
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

## Bundled MaplibreGL JS viewer for Mapzen Terrarium/WMTS imagery tiles

Porter includes a bundled MaplibreGL environment. Elevation and imagery data may be
previewed using the URL format:

```text
http://localhost:3200/terrarium_viewer/my-layer
```
