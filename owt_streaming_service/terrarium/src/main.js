import "./style.css";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";

const id = window.location.pathname.split("/").filter(Boolean)[1];
const TILE_URL = `/terrarium/${id}/{z}/{x}/{y}.png`;
const IMAGERY_URL = `/wmts/${id}/{z}/{x}/{y}.jpg`;

function getInitialView() {
  const params = new URLSearchParams(window.location.search);
  return {
    lon: parseFloat(params.get("lon")) || 120.6063,
    lat: parseFloat(params.get("lat")) || 24.0493,
    zoom: parseFloat(params.get("zoom")) || 12,
    pitch: parseFloat(params.get("pitch")) || 0,
    bearing: parseFloat(params.get("bearing")) || 0,
  };
}

function updateURL() {
  const center = map.getCenter();
  const zoom = map.getZoom();
  const params = new URLSearchParams(window.location.search);
  params.set("lon", center.lng.toFixed(5));
  params.set("lat", center.lat.toFixed(5));
  params.set("zoom", zoom.toFixed(2));
  const pitch = map.getPitch();
  const bearing = map.getBearing();
  // Only include these if they aren't defaults, keeps normal top-down urls clean
  if (pitch > 0.5) {
    params.set("pitch", pitch.toFixed(1));
  } else {
    params.delete("pitch");
  }
  if (Math.abs(bearing) > 0.5) {
    params.set("bearing", bearing.toFixed(1));
  } else {
    params.delete("bearing");
  }
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}?${params}`,
  );
}

const initial = getInitialView();

const map = new maplibregl.Map({
  container: "map",
  aroundCenter: false,
  style: {
    version: 8,
    sources: {
      osm: {
        type: "raster",
        tiles: ["https://a.tile.openstreetmap.org/{z}/{x}/{y}.png"],
        tileSize: 256,
        attribution: "&copy; OpenStreetMap Contributors",
        maxzoom: 19,
      },
      imagery: {
        type: "raster",
        tiles: [IMAGERY_URL],
        tileSize: 256,
        maxzoom: 16,
      },
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
      {
        id: "background",
        type: "background",
        paint: { "background-color": "#e8dcc8" },
      },
      {
        id: "osm",
        type: "raster",
        source: "osm",
        layout: { visibility: "none" },
      },
      {
        id: "imagery",
        type: "raster",
        source: "imagery",
        layout: { visibility: "visible" },
      },
      {
        id: "hillshade",
        type: "hillshade",
        source: "hillshadeDem",
        paint: { "hillshade-shadow-color": "#444444" },
      },
    ],
  },
  center: [initial.lon, initial.lat],
  zoom: initial.zoom,
  pitch: initial.pitch,
  bearing: initial.bearing,
  maxPitch: 85,
});

map.on("moveend", updateURL);
map.on("pitchend", updateURL);
map.on("rotateend", updateURL);
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

function setBaseLayer(name) {
  const baseLayers = ["imagery", "osm"];
  for (const layer of baseLayers) {
    map.setLayoutProperty(
      layer,
      "visibility",
      layer === name ? "visible" : "none",
    );
  }
  document.querySelectorAll("#base-toggle button").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.layer === name);
  });
}

function toggleHillshade() {
  const current = map.getLayoutProperty("hillshade", "visibility");
  const next = current === "none" ? "visible" : "none";
  map.setLayoutProperty("hillshade", "visibility", next);
  document
    .getElementById("hillshade-toggle")
    .classList.toggle("active", next === "visible");
}

const controls = document.createElement("div");
controls.id = "layer-controls";
controls.innerHTML = `
  <div id="base-toggle">
    <label>Base</label>
    <button data-layer="imagery" class="active">Imagery</button>
    <button data-layer="osm">OSM</button>
    <button data-layer="none">None</button>
  </div>
  <div id="hillshade-control">
    <button id="hillshade-toggle" class="active">Hillshade</button>
  </div>
`;
document.body.appendChild(controls);

document.querySelectorAll("#base-toggle button").forEach((btn) => {
  btn.addEventListener("click", () => setBaseLayer(btn.dataset.layer));
});
document
  .getElementById("hillshade-toggle")
  .addEventListener("click", toggleHillshade);

map.on("mousemove", (e) => {
  document.getElementById("coords").textContent =
    `${e.lngLat.lng.toFixed(5)}, ${e.lngLat.lat.toFixed(5)}`;
});
map.on("zoom", () => {
  document.getElementById("zoom").textContent = map.getZoom().toFixed(2);
});
map.on("error", (e) => console.error("map error:", e.error));
