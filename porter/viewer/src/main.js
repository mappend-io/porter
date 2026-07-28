import {
  Viewer,
  Cesium3DTileset,
  Terrain,
  EllipsoidTerrainProvider,
  Rectangle,
  ImageryLayer,
  SingleTileImageryProvider,
  NearFarScalar,
  Math as CesiumMath,
  JulianDate,
  Cartographic,
  Cartesian3,
  ScreenSpaceEventHandler,
  ScreenSpaceEventType,
} from "cesium";
import "cesium/Build/Cesium/Widgets/widgets.css";
import "./style.css";
import baseImageUrl from "./assets/world.topo.bathy.200406.3x5400x2700.jpg";

const params = new URLSearchParams(window.location.search);
const showGlobe = !params.has("noglobe");

function getInitialView() {
  return {
    lon: parseFloat(params.get("lon")),
    lat: parseFloat(params.get("lat")),
    height: parseFloat(params.get("height")),
    heading: params.has("heading") ? parseFloat(params.get("heading")) : 0,
    pitch: params.has("pitch") ? parseFloat(params.get("pitch")) : -90,
    roll: params.has("roll") ? parseFloat(params.get("roll")) : 0,
  };
}

const initial = getInitialView();

const viewer = new Viewer("cesiumContainer", {
  terrain: new Terrain(new EllipsoidTerrainProvider()),
  baseLayer: showGlobe
    ? ImageryLayer.fromProviderAsync(
        SingleTileImageryProvider.fromUrl(baseImageUrl, {
          rectangle: Rectangle.fromDegrees(-180, -90, 180, 90),
        }),
      )
    : false,
  animation: false,
  timeline: false,
  baseLayerPicker: false,
  geocoder: false,
  scene3DOnly: true,
});

viewer.cesiumWidget.creditContainer.style.display = "none";

if (showGlobe) {
  // Fade the globe out as we approach it to avoid conflicting with the 3D Tiles layers
  viewer.scene.globe.depthTestAgainstTerrain = false;
  viewer.scene.globe.translucency.enabled = true;
  viewer.scene.globe.translucency.frontFaceAlphaByDistance = new NearFarScalar(
    1000.0,
    0.0,
    10000.0,
    0.5,
  );
} else {
  viewer.scene.globe.show = false;
}

if (!isNaN(initial.lon) && !isNaN(initial.lat) && !isNaN(initial.height)) {
  viewer.camera.setView({
    destination: Cartesian3.fromDegrees(initial.lon, initial.lat, initial.height),
    orientation: {
      heading: CesiumMath.toRadians(initial.heading),
      pitch: CesiumMath.toRadians(initial.pitch),
      roll: CesiumMath.toRadians(initial.roll),
    },
  });
}

function updateURL() {
  const cartographic = viewer.camera.positionCartographic;
  const lon = CesiumMath.toDegrees(cartographic.longitude);
  const lat = CesiumMath.toDegrees(cartographic.latitude);
  const height = cartographic.height;

  const heading = CesiumMath.toDegrees(viewer.camera.heading);
  const pitch = CesiumMath.toDegrees(viewer.camera.pitch);
  const roll = CesiumMath.toDegrees(viewer.camera.roll);

  const urlParams = new URLSearchParams(window.location.search);
  urlParams.set("lon", lon.toFixed(7));
  urlParams.set("lat", lat.toFixed(7));
  urlParams.set("height", height.toFixed(3));

  // Only include if they aren't defaults
  if (Math.abs(heading) > 0.0001 && Math.abs(heading - 360) > 0.0001) {
    urlParams.set("heading", heading.toFixed(5));
  } else {
    urlParams.delete("heading");
  }

  if (Math.abs(pitch - (-90)) > 0.0001) {
    urlParams.set("pitch", pitch.toFixed(5));
  } else {
    urlParams.delete("pitch");
  }

  if (Math.abs(roll) > 0.0001 && Math.abs(roll - 360) > 0.0001) {
    urlParams.set("roll", roll.toFixed(5));
  } else {
    urlParams.delete("roll");
  }

  window.history.replaceState(null, "", `${window.location.pathname}?${urlParams}`);
}

viewer.camera.moveEnd.addEventListener(() => {
  updateURL();
});

viewer.camera.changed.addEventListener(() => {
  const cartographic = viewer.camera.positionCartographic;
  const longitudeDeg = CesiumMath.toDegrees(cartographic.longitude);

  // Offset UTC so solar noon aligns with the viewed longitude
  const solarOffsetHours = longitudeDeg / 15;
  const now = new Date();
  now.setUTCHours(now.getUTCHours() + solarOffsetHours);

  viewer.clock.currentTime = JulianDate.fromDate(now);
  document.getElementById("height").textContent = cartographic.height.toFixed(2);
});

// Update initial height display
if (document.getElementById("height") && viewer.camera.positionCartographic) {
  document.getElementById("height").textContent = viewer.camera.positionCartographic.height.toFixed(2);
}

const handler = new ScreenSpaceEventHandler(viewer.scene.canvas);
handler.setInputAction((movement) => {
  let position;
  
  // Try to pick 3D tiles or terrain first
  if (viewer.scene.pickPositionSupported) {
    try {
      position = viewer.scene.pickPosition(movement.endPosition);
    } catch (e) {
      // ignore
    }
  }

  // Fallback to the ellipsoid if we are looking at the bare globe (or globe is hidden)
  if (!position) {
    position = viewer.camera.pickEllipsoid(
      movement.endPosition,
      viewer.scene.globe.ellipsoid
    );
  }

  if (position) {
    const carto = Cartographic.fromCartesian(position);
    const lon = CesiumMath.toDegrees(carto.longitude).toFixed(5);
    const lat = CesiumMath.toDegrees(carto.latitude).toFixed(5);
    document.getElementById("coords").textContent = `${lon}, ${lat}`;
  } else {
    document.getElementById("coords").textContent = "—";
  }
}, ScreenSpaceEventType.MOUSE_MOVE);

async function loadLayer(name) {
  const tileset = await Cesium3DTileset.fromUrl(`/${name}`);
  tileset.cacheBytes = 1024 * (1 << 20);
  viewer.scene.primitives.add(tileset);
  return tileset;
}

async function loadData() {
  const names = (params.get("layers") ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

  const tilesets = await Promise.all(names.map(loadLayer));
  if (tilesets.length && isNaN(initial.lon)) {
    viewer.zoomTo(tilesets[0]);
  }
}

loadData();
