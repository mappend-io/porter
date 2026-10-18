# Metrics

Porter can expose Prometheus metrics on a separate listener. Metrics are
disabled unless `--metrics-listen-addr` (or `METRICS_LISTEN_ADDR`) is set:

```bash
porter --layer-config-uri file:///some/path/to/layers/ --metrics-listen-addr 0.0.0.0:9000
```

Then scrape http://localhost:9000/metrics.

All metrics carry a global `app="porter"` label. All counters are monotonic and
reset when the process restarts.

## HTTP requests

Request metrics are provided by
[axum-prometheus](https://crates.io/crates/axum-prometheus) and are labeled by
`method`, `endpoint` (the matched route template, e.g.
`/terrarium/{id}/{zoom}/{x}/{y}`, not the raw path) and `status`:

- `axum_http_requests_total`: Count of handled requests (counter)
- `axum_http_requests_duration_seconds`: Request latency (histogram). Buckets
  are `0.001`, `0.0025`, `0.005`, `0.01`, `0.025`, `0.05`, `0.1`, `0.25`,
  `0.5`, `1`, `2.5`, `5`, `10` and `30` seconds.
- `axum_http_requests_pending`: Requests currently in flight (gauge, no
  `status` label)

NOTE: The bundled CesiumJS and MapLibreGL viewer routes are not included in
these metrics.

## Tile and content endpoints

- `porter_content_3dtiles_tiles_total`: 3D Tiles content payloads served from
  source data
- `porter_low_res_globe_passthrough_tiles_total`: Payloads served from a
  layer's `baseGlobeTerrainUri` backfill
- `porter_archive_entry_passthrough_total`: Payloads sent with their archive
  compression intact rather than decompressed, labeled by `encoding` (`zstd` or
  `gzip`). Counted in addition to the two counters above.
- `porter_terrarium_tiles_total`: Mapzen Terrarium elevation tiles served,
  labeled by `zoom_level`. Empty fallback tiles are counted too.
- `porter_wmts_simple_tiles_total`: WMTS Simple imagery tiles served, labeled
  by `zoom_level`. Empty fallback tiles are counted too.
- `porter_wmts_class_tiles_total`: WMTS Simple class raster tiles served,
  labeled by `zoom_level`. Empty fallback tiles are counted too.

## Content transforms

These are only emitted for `.glb` payloads from layers whose
`contentTransforms` include `inline_owt_referenced_models`:

- `porter_transform_inline_owt_referenced_models_tiles_total`: Tiles that had
  referenced models inlined
- `porter_transform_inline_owt_referenced_models_unique_models_total`: Sum of
  unique models referenced across those tiles. Divide by the tiles total to get
  the average number of unique models per tile.
- `porter_transform_inline_owt_referenced_models_tilesets_total`: Tile JSON
  built from source points, for layers with a `tileLevel`

## OGC API - Features

- `porter_features_items_queries_total`: Calls to
  `/features/collections/{id}/items`
- `porter_features_items_features_total`: Features returned across all items
  queries, after any `limit` is applied

## Source I/O

These track reads against backing source data. See [Caching
behavior](/guide/how-it-works#caching-behavior) for how the caches behind them
work.

- `porter_s3_range_reads_total`: Successful ranged `GetObject` calls to S3
- `porter_s3_range_read_bytes_total`: Bytes read from S3
- `porter_file_range_reads_total`: Ranged reads from local files. Failed reads
  are counted too.
- `porter_file_range_read_bytes_total`: Bytes requested from local files,
  clamped to the file size. Failed and short reads are counted at the requested
  length.

### Block cache

The block cache only sits in front of S3 sources, so local file reads never
show up here. Each lookup is for one 1MiB block:

- `porter_caching_range_reader_queries`: Block lookups
- `porter_caching_range_reader_hits`: Lookups served from the cache, including
  lookups that waited on another request's in-flight fetch of the same block
- `porter_caching_range_reader_misses`: Lookups that went to S3
- `porter_caching_range_reader_large_reads`: Reads larger than ~17MiB that
  bypassed the block cache entirely. These do not count toward queries, hits or
  misses.

Hit rate is `hits / queries`. A low hit rate under steady traffic suggests
increasing `BLOCK_CACHE_SIZE`.

### Archive entry cache

Referenced models are read as exact byte ranges of their archive entries rather
than through the block cache, and the compressed entries are cached. Each lookup
is for one entry:

- `porter_archive_entry_cache_queries`: Entry lookups
- `porter_archive_entry_cache_hits`: Lookups served from the cache
- `porter_archive_entry_cache_misses`: Lookups that needed a read
- `porter_archive_entry_range_reads_total`: Range reads issued for missed
  entries. Entries close together in the archive share a read, so this is
  usually lower than misses.

A low hit rate under steady traffic suggests increasing `ENTRY_CACHE_SIZE`.

### Raster source cache

Imagery, elevation and class tiles are cut from decoded source rasters, which
are cached. Each lookup is for one source:

- `porter_raster_source_cache_queries`: Source lookups
- `porter_raster_source_cache_hits`: Lookups served from the cache, including
  lookups that waited on another tile's in-flight decode of the same source
- `porter_raster_source_cache_misses`: Lookups that read and decoded the
  source, or found it missing

A low hit rate under steady traffic suggests increasing
`RASTER_SOURCE_CACHE_SIZE`.

### Archive prefetch

- `porter_archive_prefetches_total`: Background archive opens started while
  serving a generated tileset above a layer's content levels. An archive that
  is already open is not read again, so this counts tileset requests, not S3
  reads.
