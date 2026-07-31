import "./style.css";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import S2Grid from "vgrid-maplibre/S2/S2Grid";

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
    featureLayer: params.get("featureLayer") || "none",
    baseLayer: params.get("baseLayer") || "imagery",
    hillshade: params.has("hillshade") ? params.get("hillshade") === "true" : true,
    grid: params.get("grid") === "true",
  };
}

function updateFeatureStyle() {
  if (!map.getLayer("features-poly") || currentFeatureLayer === "none") return;

  // Generate a deterministic hash from the layer name
  let hash = 0;
  for (let i = 0; i < currentFeatureLayer.length; i++) {
    hash = currentFeatureLayer.charCodeAt(i) + ((hash << 5) - hash);
  }

  // Use the hash to pick a nice hue (0-360) and keep saturation/lightness pleasing
  const hue = Math.abs(hash % 360);
  const color = `hsl(${hue}, 75%, 45%)`;

  if (currentFeatureLayer.toLowerCase().includes("navmesh")) {
    // Colorize navmesh by terrain type, this should be moved into layer definitions and exposed instead!
    const terrainExpression = [
      "match",
      ["get", "TERRAIN_TYPE"],
      0, "transparent", // None
      1, "#a3e4d7",     // Land
      2, "#c3b091",     // Hill
      3, "#808b96",     // Mountain
      4, "#3498db",     // Water
      5, "#e67e22",     // Urban
      6, "#34495e",     // Road
      7, "#2980b9",     // Ocean
      8, "#7fb3d5",     // Bathymetry littoral
      9, "#5499c7",     // Bathy ocean
      10, "#154360",    // Bathy deep ocean
      11, "#1a5276",    // Ocean shipping lanes
      12, "#229954",    // Forest
      13, "#5d6d7e",    // Impassable mountains
      14, "#82e0aa",    // Terrain tree cover sparse
      15, "#196f3d",    // Dense
      16, "#f5b041",    // Suburban
      17, "#d35400",    // Downtown
      18, "#f1c40f",    // Cropland
      19, "#b9770e",    // Shrubland
      20, "#d98880",    // Bare
      21, "#ffffff",    // Snow ice
      22, "#5dade2",    // Water coastal
      23, "#2e86c1",    // Water shallow
      24, "#1b4f72",    // Water deep
      25, "#17a589",    // Marsh
      26, "#717d7e",    // Steep rugged
      27, "#424949",    // Extreme
      28, "#a93226",    // Road interstate
      29, "#c0392b",    // Road highway
      30, "#7f8c8d",    // Road minor
      31, "#85c1e9",    // Inland water
      32, "#117864",    // Swamp
      color // fallback to deterministic color
    ];
    map.setPaintProperty("features-poly", "fill-color", terrainExpression);
    map.setPaintProperty("features-line", "line-color", terrainExpression);
    map.setPaintProperty("features-point", "circle-color", terrainExpression);
  } else {
    map.setPaintProperty("features-poly", "fill-color", color);
    map.setPaintProperty("features-line", "line-color", color);
    map.setPaintProperty("features-point", "circle-color", color);
  }
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
  if (currentFeatureLayer !== "none") {
    params.set("featureLayer", currentFeatureLayer);
  } else {
    params.delete("featureLayer");
  }
  if (currentBaseLayer !== "imagery") {
    params.set("baseLayer", currentBaseLayer);
  } else {
    params.delete("baseLayer");
  }
  if (!hillshadeVisible) {
    params.set("hillshade", "false");
  } else {
    params.delete("hillshade");
  }
  if (gridVisible) {
    params.set("grid", "true");
  } else {
    params.delete("grid");
  }
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}?${params}`,
  );
}

const initial = getInitialView();
let currentFeatureLayer = initial.featureLayer;
let currentFeatureUrl = null;
let currentBaseLayer = initial.baseLayer;
let hillshadeVisible = initial.hillshade;
let gridVisible = initial.grid;

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
        layout: { visibility: initial.baseLayer === "osm" ? "visible" : "none" },
      },
      {
        id: "imagery",
        type: "raster",
        source: "imagery",
        layout: { visibility: initial.baseLayer === "imagery" ? "visible" : "none" },
      },
      {
        id: "hillshade",
        type: "hillshade",
        source: "hillshadeDem",
        layout: { visibility: initial.hillshade ? "visible" : "none" },
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

  map.addSource("features", {
      type: "geojson",
      data: { type: "FeatureCollection", features: [] },
  });

  map.addLayer({
    id: "features-poly",
    type: "fill",
    source: "features",
    filter: ["==", ["geometry-type"], "Polygon"],
    paint: {
      "fill-color": "#e55",
      "fill-opacity": 0.4,
    },
  });

  map.addLayer({
    id: "features-line",
    type: "line",
    source: "features",
    filter: ["any", ["==", ["geometry-type"], "LineString"], ["==", ["geometry-type"], "Polygon"]],
    layout: {
      "line-cap": "round",
      "line-join": "round",
    },
    paint: {
      "line-color": "#e55",
      "line-width": 2,
    },
  });

  map.addLayer({
    id: "features-line-hitbox",
    type: "line",
    source: "features",
    filter: ["any", ["==", ["geometry-type"], "LineString"], ["==", ["geometry-type"], "Polygon"]],
    layout: {
      "line-cap": "round",
      "line-join": "round",
    },
    paint: {
      "line-color": "transparent",
      "line-width": 15,
    },
  });

  map.addLayer({
    id: "features-point",
    type: "circle",
    source: "features",
    filter: ["==", ["geometry-type"], "Point"],
    paint: {
      "circle-color": "#e55",
      "circle-radius": 4,
      "circle-stroke-width": 1,
      "circle-stroke-color": "#fff"
    },
  });

  map.addLayer({
    id: "features-point-hitbox",
    type: "circle",
    source: "features",
    filter: ["==", ["geometry-type"], "Point"],
    paint: {
      "circle-color": "transparent",
      "circle-radius": 15,
    },
  });

  const featureLayers = ["features-poly", "features-line", "features-line-hitbox", "features-point", "features-point-hitbox"];

  updateFeatureStyle();

  map.on("click", (e) => {
    const bbox = [
      [e.point.x - 5, e.point.y - 5],
      [e.point.x + 5, e.point.y + 5]
    ];
    const features = map.queryRenderedFeatures(bbox, {
      layers: featureLayers,
    });
    if (!features.length) return;

    const feature = features[0];
    const props = feature.properties;

    let tableHtml = '<table class="feature-table"><tbody>';
    for (const [key, value] of Object.entries(props)) {
      tableHtml += `<tr><th>${key}</th><td>${value}</td></tr>`;
    }
    tableHtml += '</tbody></table>';

    new maplibregl.Popup({ maxWidth: "300px" })
      .setLngLat(e.lngLat)
      .setHTML(tableHtml)
      .addTo(map);
  });

  for (const layer of featureLayers) {
    map.on("mouseenter", layer, () => {
      map.getCanvas().style.cursor = "pointer";
    });
    map.on("mouseleave", layer, () => {
      map.getCanvas().style.cursor = "";
    });
  }

  refresh();
  map.on("moveend", refresh);
});

let abortCtrl = null;

async function refresh() {
  if (currentFeatureLayer === "none" || !currentFeatureUrl || map.getZoom() < 13.5) {
    if (map.getSource("features")) {
      map.getSource("features").setData({ type: "FeatureCollection", features: [] });
    }
    return;
  }

  const b = map.getBounds();
  const bbox = [b.getWest(), b.getSouth(), b.getEast(), b.getNorth()].join(",");

  if (abortCtrl) abortCtrl.abort();
  abortCtrl = new AbortController();

  try {
    const res = await fetch(
      `${currentFeatureUrl}?bbox=${bbox}`,
      { signal: abortCtrl.signal }
    );
    const geojson = await res.json();
    map.getSource("features").setData(geojson);
  } catch (e) {
    if (e.name !== "AbortError") console.error(e);
  }
}

function setBaseLayer(name) {
  currentBaseLayer = name;
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
  updateURL();
}

function toggleHillshade() {
  hillshadeVisible = !hillshadeVisible;
  const next = hillshadeVisible ? "visible" : "none";
  map.setLayoutProperty("hillshade", "visibility", next);
  document
    .getElementById("hillshade-toggle")
    .classList.toggle("active", hillshadeVisible);
  updateURL();
}

var s2Grid = null;
function toggleGrid() {
  gridVisible = !gridVisible;
  if (!gridVisible) {
      s2Grid.remove();
  } else {
      s2Grid.show();
  }
  if (map.getLayer('s2-labels')) {
    map.setLayoutProperty('s2-labels', 'visibility', gridVisible ? 'visible' : 'none');
  }
  document
    .getElementById("grid-toggle")
    .classList.toggle("active", gridVisible);
  updateURL();
}

const controls = document.createElement("div");
controls.id = "layer-controls";
controls.innerHTML = `
  <div id="base-toggle" style="display: flex; align-items: center; gap: 4px;">
    <label style="width: 80px;">Base Layer</label>
    <button data-layer="imagery" class="${initial.baseLayer === 'imagery' ? 'active' : ''}">Imagery</button>
    <button data-layer="osm" class="${initial.baseLayer === 'osm' ? 'active' : ''}">OSM</button>
    <button data-layer="none" class="${initial.baseLayer === 'none' ? 'active' : ''}">None</button>
  </div>
  <div id="options-toggle" style="display: flex; align-items: center; gap: 4px; margin-top: 5px;">
    <label style="width: 80px;">Options</label>
    <button id="hillshade-toggle" class="${initial.hillshade ? 'active' : ''}">Hillshade</button>
    <button id="grid-toggle" class="${initial.grid ? 'active' : ''}">S2 grid</button>
  </div>
  <div id="feature-layer-control" style="display: flex; align-items: center; gap: 4px; margin-top: 5px;">
    <label style="width: 80px;">Features</label>
    <select id="feature-layer-select" style="flex-grow: 1; padding: 4px; border: 1px solid #ccc; border-radius: 4px; font-size: 13px;">
    </select>
  </div>
`;
document.body.appendChild(controls);

async function loadFeatureLayers() {
  try {
    const res = await fetch("/layers");
    const layers = await res.json();
    const select = document.getElementById("feature-layer-select");

    const noneOpt = document.createElement("option");
    noneOpt.value = "none";
    noneOpt.textContent = "None";
    select.appendChild(noneOpt);

    for (const layer of layers.items) {
      if (!layer.endpoints) continue;
      const ogcEndpoint = layer.endpoints.find((e) => e.type === "ogc_api_features");
      if (ogcEndpoint) {
        const opt = document.createElement("option");
        opt.value = layer.id;
        opt.dataset.uri = ogcEndpoint.uri;
        opt.textContent = layer.description || layer.id;
        if (layer.id === currentFeatureLayer) {
          opt.selected = true;
          currentFeatureUrl = ogcEndpoint.uri;
        }
        select.appendChild(opt);
      }
    }

    if (currentFeatureLayer !== "none" && !currentFeatureUrl) {
        currentFeatureLayer = "none";
        select.value = "none";
        updateURL();
    }

    select.addEventListener("change", (e) => {
      currentFeatureLayer = e.target.value;
      const selectedOpt = e.target.options[e.target.selectedIndex];
      currentFeatureUrl = selectedOpt.dataset.uri || null;
      updateURL();
      updateFeatureStyle();
      refresh();
    });
  } catch (e) {
    console.error("Failed to load feature layers", e);
  }
}

loadFeatureLayers();

document.querySelectorAll("#base-toggle button").forEach((btn) => {
  btn.addEventListener("click", () => setBaseLayer(btn.dataset.layer));
});
document
  .getElementById("hillshade-toggle")
  .addEventListener("click", toggleHillshade);
document
  .getElementById("grid-toggle")
  .addEventListener("click", toggleGrid);

map.on("mousemove", (e) => {
  document.getElementById("coords").textContent =
    `${e.lngLat.lng.toFixed(5)}, ${e.lngLat.lat.toFixed(5)}`;
});
map.on("zoom", () => {
  document.getElementById("zoom").textContent = map.getZoom().toFixed(2);
});
map.on("error", (e) => console.error("map error:", e.error));

map.on('style.load', () => {
  s2Grid = new S2Grid(map, {
    color: 'rgba(255, 255, 0, 0.5)',
    redraw: 'moveend',
    minResolution: 2,
    maxResolution: 7,
  });
  if (!gridVisible) {
    s2Grid.remove();
  }
});

map.on('sourcedata', (e) => {
  if (
    e.sourceId === 's2-grid' &&
    map.getSource('s2-grid') &&
    !map.getLayer('s2-labels')
  ) {
    map.addLayer({
      id: 's2-labels',
      type: 'symbol',
      source: 's2-grid',
      layout: {
        'text-field': ['get', 's2_token'],
        'text-size': 12,
        'visibility': gridVisible ? 'visible' : 'none',
      },
      paint: {
        'text-halo-color': 'white',        // halo color
        'text-halo-width': 1.5,            // halo width
        'text-halo-blur': 0.5              // optional: smooth halo edges
      },
    });
  }
});
