# OGC API - Features

When a vector layer is configured, Porter automatically exposes the content via an OGC API - Features endpoint. The layer ID from its definition is used as the collection ID.

```
GET /features/collections/{layer_id}/items?bbox={min_lon},{min_lat},{max_lon},{max_lat}
```

## Response

`200 OK` · `application/json`

The response body is a GeoJSON document containing features in the requested `bbox`.
