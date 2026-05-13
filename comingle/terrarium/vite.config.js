export default {
  base: "/terrarium_viewer/",
  build: {
    // maplibre-gl is large, but we want it, so raise the warning threshold
    chunkSizeWarningLimit: 2000, // kb
  },
};
