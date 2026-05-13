import "./style.css";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";

const id = window.location.pathname.split("/").filter(Boolean)[1];
const TILE_URL = `/terrarium/${id}/{z}/{x}/{y}.png`;

const map = new maplibregl.Map({
  container: "map",
  aroundCenter: false,
  style: {
    version: 8,
    sources: {
      // osm: {
      //   type: "raster",
      //   tiles: ["https://a.tile.openstreetmap.org/{z}/{x}/{y}.png"],
      //   tileSize: 256,
      //   attribution: "&copy; OpenStreetMap Contributors",
      //   maxzoom: 19,
      // },
      hillshadeDem: {
        type: "raster-dem",
        tiles: [TILE_URL],
        tileSize: 256,
        encoding: "terrarium",
        maxzoom: 16,
      },
      terrainDem: {
        type: "raster-dem",
        tiles: [TILE_URL],
        tileSize: 256,
        encoding: "terrarium",
        maxzoom: 16,
      },
    },
    layers: [
      // When OSM is disabled, we need some background fill
      {
        id: "background",
        type: "background",
        paint: { "background-color": "#e8dcc8" },
      },
      // {
      //   id: "osm",
      //   type: "raster",
      //   source: "osm",
      // },
      {
        id: "hillshade",
        type: "hillshade",
        source: "hillshadeDem",
        paint: { "hillshade-shadow-color": "#473B24" },
      },
    ],
  },
  center: [120.6063, 24.0493],
  zoom: 12,
  maxPitch: 85,
});

map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }));
map.addControl(new maplibregl.ScaleControl());

map.on("load", () => {
  map.setTerrain({ source: "terrainDem", exaggeration: 0.5 });

  map.setSky({
    "sky-color": "#196bc4",
    "sky-horizon-blend": 0.5,
    "horizon-color": "#e8dcc8",
    "horizon-fog-blend": 0.5,
    "fog-color": "#d8d8d8",
    "fog-ground-blend": 0.5,
  });
});

map.on("mousemove", (e) => {
  document.getElementById("coords").textContent =
    `${e.lngLat.lng.toFixed(5)}, ${e.lngLat.lat.toFixed(5)}`;
});

map.on("zoom", () => {
  document.getElementById("zoom").textContent = map.getZoom().toFixed(2);
});

map.on("error", (e) => console.error("map error:", e.error));
