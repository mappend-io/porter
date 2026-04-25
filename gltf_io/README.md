# First `Buffer` in `gltf.buffers` inside GLB notes

https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#glb-stored-buffer

> Any glTF buffer with undefined buffer.uri property that is not the first
  element of buffers array does not refer to the GLB-stored BIN chunk, and the
  behavior of such buffers is left undefined to accommodate future extensions
  and specification versions.
