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
    DirectionalLight,
} from "cesium";
import "cesium/Build/Cesium/Widgets/widgets.css";
import "./style.css";
import baseImageUrl from "./assets/world.topo.bathy.200406.3x5400x2700.jpg";

const params = new URLSearchParams(globalThis.location.search);
const showGlobe = !params.has("noglobe");

function getInitialView() {
    return {
        lon: Number.parseFloat(params.get("lon")),
        lat: Number.parseFloat(params.get("lat")),
        height: Number.parseFloat(params.get("height")),
        heading: params.has("heading") ? Number.parseFloat(params.get("heading")) : 0,
        pitch: params.has("pitch") ? Number.parseFloat(params.get("pitch")) : -90,
        roll: params.has("roll") ? Number.parseFloat(params.get("roll")) : 0,
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

viewer.scene.light = new DirectionalLight({
    direction: viewer.camera.directionWC,
    intensity: 4,
});

viewer.scene.preRender.addEventListener((scene) => {
    Cartesian3.clone(scene.camera.directionWC, scene.light.direction);
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

if (!Number.isNaN(initial.lon) && !Number.isNaN(initial.lat) && !Number.isNaN(initial.height)) {
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

    const urlParams = new URLSearchParams(globalThis.location.search);
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

    globalThis.history.replaceState(null, "", `${globalThis.location.pathname}?${urlParams}`);
}

viewer.camera.moveEnd.addEventListener(() => {
    updateURL();
});

function updateTimeAndHeight() {
    const cartographic = viewer.camera.positionCartographic;
    if (!cartographic) return;

    const longitudeDeg = CesiumMath.toDegrees(cartographic.longitude);

    // Offset UTC so solar noon (12:00 PM) aligns with the viewed longitude
    const solarOffsetHours = longitudeDeg / 15;
    // Pick an equinox date so both northern and southern hemispheres are evenly lit
    const now = new Date("2023-03-21T12:00:00Z");
    now.setUTCHours(12 - solarOffsetHours);

    viewer.clock.currentTime = JulianDate.fromDate(now);

    const heightEl = document.getElementById("height");
    if (heightEl) {
        heightEl.textContent = cartographic.height.toFixed(2);
    }
}

viewer.camera.changed.addEventListener(updateTimeAndHeight);

// Call once on startup so the sky/globe isn't dark before the first movement
updateTimeAndHeight();

const handler = new ScreenSpaceEventHandler(viewer.scene.canvas);
let pickPositionFailureLogged = false;

handler.setInputAction((movement) => {
    const endPosition = movement?.endPosition;
    if (!endPosition) return;

    let position;

    // Try to pick 3D tiles or terrain first
    if (viewer.scene.pickPositionSupported) {
        try {
            position = viewer.scene.pickPosition(endPosition);
        } catch (e) {
            if (!pickPositionFailureLogged) {
                pickPositionFailureLogged = true;
                console.warn("pickPosition failed, falling back to the ellipsoid", e);
            }
        }
    }

    // Fallback to the ellipsoid if we are looking at the bare globe (or globe is hidden)
    if (!position) {
        position = viewer.camera.pickEllipsoid(
            endPosition,
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
    if (tilesets.length && Number.isNaN(initial.lon)) {
        viewer.zoomTo(tilesets[0]);
    }
}

loadData();
