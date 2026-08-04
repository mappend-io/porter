# Mapzen Terrarium Elevation

When a layer is configured with `elevationRasterContent`, Porter automatically exposes the elevation content via Mapzen Terrarium elevation endpiont.

```
GET /terrarium/{layer_id}/{z}/{x}/{y}.png
```

## Response

`200 OK` · `application/octet-stream`

The response body is a PNG-encoded image. The elevation encoding is described in [Mapzen Terrarium](https://github.com/tilezen/joerd/blob/master/docs/formats.md#terrarium).
