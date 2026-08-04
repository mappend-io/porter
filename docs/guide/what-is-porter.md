# What is Porter?

Porter is a 3D Tiles 1.1 tile server that exposes virtual tilesets backed by S3 or local filesystem content, organized via S2 cell tokens. The source tilesets may be stored in 3TZ archives. Porter serves tilesets over HTTP for use with CesiumJS, Cesium for Unreal, Cesium for Unity, and other 3D Tiles consumers, and includes a bundled CesiumJS viewer for quick inspection.

It also transforms data on the fly to expose it via compatibility endpoints for non-3D Tiles consumers. Today, Mapzen Terrarium RGB elevation and WMTS Simple imagery layers are exposed.

3D Tiles glTF content can also be transformed on the fly before it sent to the consumer.
