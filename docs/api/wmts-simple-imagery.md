# WMTS Simple Imagery

When a layer is configured with `imageryRasterContent`, Porter automatically exposes the imagery content via WMTS Simple.

```
GET /wmts/{layer_id}/{z}/{x}/{y}.jpg
```

## Response

`200 OK` · `application/octet-stream`

The response body is a JPEG-encoded image.
