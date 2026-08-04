# Discover layers

Discover available layers and their respective endpoints.

```
GET /layers
```

## Response

`200 OK` · `application/json`

| Field | Type | Description |
| --- | --- | --- |
| `items` | `Layer[]` | All layers available. May be empty. |

### Layer

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Stable identifier. |
| `description` | `string?` | Friendly description. Optional. |
| `endpoints` | `Endpoint[]` | User-consumable entry points. At least one. |

### Endpoint

| Field | Type | Description |
| --- | --- | --- |
| `type` | `string` | See [Endpoint types](#endpoint-types). |
| `uri` | `string` | Absolute URL. |

### Endpoint types

| Identifier | Description |
| --- | --- |
| `3d_tiles` | See [3D Tiles](/api/3d-tiles) |
| `ogc_api_features` | See [OGC API - Features](/api/ogc-api-features) |
| `imagery_wmts_simple_jpg` | See [WMTS Simple Imagery](/api/wmts-simple-imagery) |
| `elevation_mapzen_terrarium_rgb_png` | See [Mapzen Terrarium Elevation](/api/mapzen-terrarium-elevation) |

## Example

::: info Sample response
```json
{
  "items": [
    {
      "id": "terrain",
      "description": "An example terrain layer",
      "endpoints": [
        {
          "type": "3d_tiles",
          "uri": "http://localhost:3200/terrain"
        },
        {
          "type": "elevation_mapzen_terrarium_rgb_png",
          "uri": "http://localhost:3200/terrarium/terrain"
        },
        {
          "type": "imagery_wmts_simple_jpg",
          "uri": "http://localhost:3200/wmts/terrain"
        }
      ]
    },
    {
      "id": "building_footprints",
      "description": "An example feature layer",
      "endpoints": [
        {
          "type": "3d_tiles",
          "uri": "http://localhost:3200/building_footprints"
        },
        {
          "type": "ogc_api_features",
          "uri": "http://localhost:3200/features/collections/building_footprints/items"
        }
      ]
    }
  ]
}
```
:::
