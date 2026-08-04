# 3D Tiles

When a layer is configured, Porter automatically exposes the content via a 3D Tiles endpoint.

```
GET /{layer_id}
```

## Response

`200 OK` · `application/json`

The response body is a 3D Tiles 1.1 tileset document.
