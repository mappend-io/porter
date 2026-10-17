# Caching behavior

All file I/O routes through the `ResourceLoader` to enable use of either
`file:` or `s3:` URIs. To keep performance reasonable when using relatively
high-latency S3 source data, Porter maintains several bounded in-memory caches.
Most use QuickCache, which evicts with a CLOCK-PRO-style policy.

- **Range readers** (QuickCache, up to 8k instances). Local readers are cheap
  to create (an open and a stat) but hold an open file handle while cached, so
  size your file descriptor limit accordingly. S3 readers need a `HeadObject`
  call to fetch the object size, so caching them avoids a round trip per
  request.
- **Archive indexes** (QuickCache, ~2GiB). Built from the `@3dtilesIndex1@`
  entry of each 3TZ archive the first time it is opened, and used to look up
  files inside the archive by path hash. Cached separately from blocks so that
  block churn doesn't evict them.
- **Block cache** (QuickCache, `BLOCK_CACHE_SIZE`, default `2GiB`). S3 objects
  are read in 1MiB blocks, which are cached and shared across all readers. This
  is where most memory goes; make it as large as your hardware allows. The
  block size is tuned for AWS S3 and gives useful read-ahead, which cuts down
  the number of requests to S3. Reads larger than ~17MiB bypass the block cache
  to avoid polluting it. Local files are not block cached.
- **Archive entry cache** (QuickCache, `ENTRY_CACHE_SIZE`, default `1GiB`).
  Content transforms that read many small entries at once, such as inlining
  referenced models, bypass the block cache. A tile's models are scattered
  throughout their archive, so whole blocks would mostly hold unrelated
  entries. Instead each entry is read as an exact byte range (from its offset
  in the archive index to the next entry's), with entries close together
  sharing a request, and the still-compressed entries are cached here.
- **Layer definitions** (moka, up to 1024 entries, TTL `LAYER_DEFINITION_TTL`,
  default `5m`). Layer definitions are always read fresh from the source,
  skipping the reader and block caches, so an updated definition takes effect
  once its TTL expires.

# Response compression

Responses are compressed when the client sends an `Accept-Encoding` that
allows it, preferring zstd over gzip, at the fastest compression levels.
Combined models and GeoJSON compress roughly 4x. PNG and JPEG tiles are already
compressed and are sent as-is.
