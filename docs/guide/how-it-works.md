# Caching behavior

All file I/O routes through the `ResourceLoader` to enable use of either
`file:` or `s3:` URIs. To keep performance reasonable when using relatively
high-latency S3 source data, Porter maintains several bounded in-memory caches.
Most use QuickCache, which evicts with a CLOCK-PRO-style policy.

- **Range readers** (QuickCache, up to 8k instances). Local readers are cheap
  to create (an open and a stat) but hold an open file handle while cached, so
  size your file descriptor limit accordingly. S3 readers are opened with a
  single suffix-range `GetObject` for the last 2MiB of the object, which gives
  its size, and the whole blocks in it are added to the block cache. Archives
  are read from the end first, so their footer is already cached once the
  reader is open. Caching readers avoids that request on every read.
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

Files read from 3TZ archives are usually compressed already (deflate, or zstd
in newer data). When no content transform applies and the client accepts the
archive's encoding, the compressed bytes are sent as they are: zstd entries as
`Content-Encoding: zstd` (when their window is at most 8MB, per RFC 9659), and
deflate entries wrapped as `Content-Encoding: gzip`. Otherwise they are
decompressed, and compressed again if the client accepts another encoding.
