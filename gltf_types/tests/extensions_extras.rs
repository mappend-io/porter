use serde_json::json;

// extensions/extras must round-trip at every level, and be omitted when absent
#[test]
fn extensions_and_extras_round_trip() {
    let src = json!({
        "asset": { "version": "2.0", "extras": { "a": 1 } },
        "extensionsUsed": ["EXT_test"],
        "buffers": [{ "byteLength": 4, "extensions": { "EXT_test": { "b": 2 } } }],
        "bufferViews": [{ "buffer": 0, "byteLength": 4, "extras": "bv" }],
        "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "SCALAR", "extras": [1, 2] }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "extensions": { "EXT_test": {} } }], "extras": { "m": true } }],
        "materials": [{
            "pbrMetallicRoughness": { "baseColorTexture": { "index": 0, "extras": { "t": 1 } }, "extras": { "p": 1 } },
            "normalTexture": { "index": 0, "extensions": { "EXT_test": { "n": 1 } } },
            "occlusionTexture": { "index": 0, "extras": { "o": 1 } },
            "extensions": { "EXT_test": { "mat": 1 } }
        }],
        "textures": [{ "source": 0, "sampler": 0, "extras": { "tex": 1 } }],
        "images": [{ "uri": "a.png", "extensions": { "EXT_test": { "img": 1 } } }],
        "samplers": [{ "extras": { "s": 1 } }],
        "nodes": [{ "mesh": 0, "extensions": { "EXT_test": { "node": 1 } } }],
        "scenes": [{ "nodes": [0], "extras": { "scene": 1 } }],
        "scene": 0,
        "extensions": { "EXT_test": { "doc": 1 } },
        "extras": { "doc": "x" }
    });

    let doc: gltf_types::Document = serde_json::from_str(&src.to_string()).unwrap();
    assert_eq!(doc.extras, Some(json!({ "doc": "x" })));
    assert_eq!(
        doc.nodes[0].extensions,
        Some(json!({ "EXT_test": { "node": 1 } }))
    );
    assert_eq!(doc.asset.extras, Some(json!({ "a": 1 })));

    let out = serde_json::to_value(&doc).unwrap();
    for ptr in [
        "/asset/extras",
        "/buffers/0/extensions",
        "/bufferViews/0/extras",
        "/accessors/0/extras",
        "/meshes/0/extras",
        "/meshes/0/primitives/0/extensions",
        "/materials/0/extensions",
        "/materials/0/pbrMetallicRoughness/extras",
        "/materials/0/pbrMetallicRoughness/baseColorTexture/extras",
        "/materials/0/normalTexture/extensions",
        "/materials/0/occlusionTexture/extras",
        "/textures/0/extras",
        "/images/0/extensions",
        "/samplers/0/extras",
        "/nodes/0/extensions",
        "/scenes/0/extras",
        "/extensions",
        "/extras",
    ] {
        assert_eq!(out.pointer(ptr), src.pointer(ptr), "mismatch at {ptr}");
    }

    // Absent values are not serialized as null
    assert!(out.pointer("/nodes/0/extras").is_none());
    assert!(out.pointer("/buffers/0/extras").is_none());
}
