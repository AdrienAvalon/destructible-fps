"""Run only through the isolated tooling smoke launcher, on a factory-startup scene."""

import json
import os
from pathlib import Path
import struct

import bpy

output = Path(os.environ["FPS_TOOL_OUTPUT"])
cube = bpy.data.objects["Cube"]
cube.dimensions = (2, 3, 4)
bpy.context.view_layer.objects.active = cube
bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
material = bpy.data.materials.new("tool-smoke-concrete")
material.use_nodes = True
shader = material.node_tree.nodes.get("Principled BSDF")
shader.inputs["Base Color"].default_value = (0.32, 0.28, 0.24, 1)
shader.inputs["Roughness"].default_value = 0.75
cube.data.materials.clear()
cube.data.materials.append(material)
bpy.ops.object.select_all(action="DESELECT")
cube.select_set(True)
asset = output / "tool-smoke.glb"
bpy.ops.export_scene.gltf(filepath=str(asset), export_format="GLB", use_selection=True, export_yup=True)

# Verify the actual GLB container and serialized coordinate convention, not just operator success.
data = asset.read_bytes()
assert struct.unpack_from("<4sII", data) == (b"glTF", 2, len(data))
length, kind = struct.unpack_from("<I4s", data, 12)
assert kind == b"JSON"
gltf = json.loads(data[20:20 + length])
primitive = gltf["meshes"][0]["primitives"][0]
position = gltf["accessors"][primitive["attributes"]["POSITION"]]
assert position["min"] == [-1, -2, -1.5]  # Blender Z-up -> glTF Y-up.
assert position["max"] == [1, 2, 1.5]
assert "TEXCOORD_0" in primitive["attributes"]
assert gltf["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"] == 0.75

bpy.ops.object.delete(use_global=False)
bpy.ops.import_scene.gltf(filepath=str(asset))
meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
assert len(meshes) == 1
restored = meshes[0]
bpy.context.view_layer.update()
assert all(abs(actual - expected) < 1e-5 for actual, expected in zip(restored.dimensions, (2, 3, 4)))
assert len(restored.data.polygons) == 12
assert len(restored.data.uv_layers) == 1
assert len(restored.data.materials) == 1

# CPU-only, tiny offline rendering proves Blender works, not the game's visual quality.
scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = 8
scene.render.threads_mode = "FIXED"
scene.render.threads = 4
scene.render.resolution_x = 256
scene.render.resolution_y = 256
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = "PNG"
scene.render.filepath = str(output / "tool-smoke.png")
bpy.ops.render.render(write_still=True)
assert (output / "tool-smoke.png").stat().st_size > 1000
(output / "blender-result.json").write_text(json.dumps({
    "version": bpy.app.version_string, "glb_bytes": len(data),
    "dimensions_m": list(restored.dimensions), "triangles": 12,
    "uv_layers": 1, "gltf_y_up_verified": True,
    "scope": "authoring tool only; no game GLB importer yet",
}, indent=2) + "\n")
