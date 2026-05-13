# Resources

- https://github.com/tilezen/joerd/blob/master/docs/formats.md
- https://github.com/tilezen/joerd/blob/master/docs/http-status-codes.md
- https://github.com/tilezen/joerd/blob/master/docs/use-service.md
- https://docs.maptiler.com/google-maps-coordinates-tile-bounds-projection/
- https://book.georust.org/youre-projecting.html
- https://www.mapzen.com/blog/terrain-tile-service/
- https://www.mapzen.com/blog/long-term-support-mapzen-maps/
- https://www.mapzen.com/blog/mapping-mountains/
- https://igorgatis.github.io/ws2/

# Optimizations

The source rasters are much larger than the 256x256 outputs. This means that to
serve a single 256x256, we are re-decoding a ~1k image several times over. It's
even worse if the cache layer isn't warm when we do that. Odds are good if we
need one tile, we'll use it several times in quick succession. So maybe caching
the decoded PNG would be smart. But.. in a scaled out environment, there's no
guarantee the same host fields the same request for neighboring tiles. Work
could probably be done to build some affinity in, but this is probably not worth
pursuing.
