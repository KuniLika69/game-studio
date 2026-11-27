"""Generate Mira's self-contained GLB from the repository's licensed Emma base."""

import io
import json
import math
from pathlib import Path
import struct

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
BASE = HERE.parent / "emma.glb"
COMPONENTS = {5121: "u1", 5123: "<u2", 5125: "<u4", 5126: "<f4"}
COMPONENT_COUNT = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


class Model:
    def __init__(self):
        source = BASE.read_bytes()
        metadata_size = struct.unpack_from("<I", source, 12)[0]
        self.doc = json.loads(source[20:20 + metadata_size])
        self.blob = bytearray(source[28 + metadata_size:])

    def append_view(self, data):
        self.blob.extend(b"\0" * ((-len(self.blob)) % 4))
        view = len(self.doc["bufferViews"])
        self.doc["bufferViews"].append(
            {"buffer": 0, "byteOffset": len(self.blob), "byteLength": len(data)}
        )
        self.blob.extend(data)
        return view

    def array(self, index):
        accessor = self.doc["accessors"][index]
        view = self.doc["bufferViews"][accessor["bufferView"]]
        width = COMPONENT_COUNT[accessor["type"]]
        return np.frombuffer(
            self.blob,
            dtype=COMPONENTS[accessor["componentType"]],
            offset=view.get("byteOffset", 0) + accessor.get("byteOffset", 0),
            count=accessor["count"] * width,
        ).copy().reshape(-1, width)

    def replace_array(self, index, values):
        accessor = self.doc["accessors"][index]
        view = self.doc["bufferViews"][accessor["bufferView"]]
        data = np.asarray(values, dtype=COMPONENTS[accessor["componentType"]]).tobytes()
        offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        self.blob[offset:offset + len(data)] = data
        for key, operation in (("min", np.min), ("max", np.max)):
            if key in accessor:
                accessor[key] = operation(values, axis=0).astype(float).tolist()

    def new_array(self, values, component, kind, bounds=False):
        values = np.asarray(values, dtype=COMPONENTS[component])
        accessor = {
            "bufferView": self.append_view(values.tobytes()),
            "componentType": component,
            "count": len(values),
            "type": kind,
        }
        if bounds:
            accessor["min"] = values.min(axis=0).astype(float).tolist()
            accessor["max"] = values.max(axis=0).astype(float).tolist()
        self.doc["accessors"].append(accessor)
        return len(self.doc["accessors"]) - 1

    def image(self, index):
        view_index = self.doc["images"][index]["bufferView"]
        view = self.doc["bufferViews"][view_index]
        offset = view.get("byteOffset", 0)
        return Image.open(io.BytesIO(self.blob[offset:offset + view["byteLength"]])).convert("RGB")

    def texture(self, image, title):
        stream = io.BytesIO()
        if image.mode == "RGBA" or image.mode == "L":
            image.save(stream, format="PNG", optimize=True)
            mime = "image/png"
        else:
            image.save(stream, format="JPEG", quality=95, subsampling=0)
            mime = "image/jpeg"
        image_index = len(self.doc["images"])
        self.doc["images"].append({
            "bufferView": self.append_view(stream.getvalue()),
            "mimeType": mime,
            "name": title,
        })
        self.doc["textures"].append({"source": image_index, "sampler": 0, "name": title})
        return len(self.doc["textures"]) - 1

    def add_head_mesh(self, name, position, normals, uv, indices, material):
        joints = self.doc["skins"][0]["joints"]
        head = next(i for i in joints if self.doc["nodes"][i].get("name") == "mixamorig:Head")
        weights = np.zeros((len(position), 4))
        weights[:, 0] = 1
        joint_ids = np.zeros((len(position), 4), dtype=np.uint8)
        joint_ids[:, 0] = joints.index(head)
        attributes = {
            "POSITION": self.new_array(position, 5126, "VEC3", bounds=True),
            "NORMAL": self.new_array(normals, 5126, "VEC3"),
            "TEXCOORD_0": self.new_array(uv, 5126, "VEC2"),
            "JOINTS_0": self.new_array(joint_ids, 5121, "VEC4"),
            "WEIGHTS_0": self.new_array(weights, 5126, "VEC4"),
        }
        self.doc["meshes"].append({
            "name": name,
            "primitives": [{
                "attributes": attributes,
                "indices": self.new_array(np.asarray(indices).ravel(), 5123, "SCALAR"),
                "material": material,
            }],
        })
        self.doc["nodes"].append({"name": name, "mesh": len(self.doc["meshes"]) - 1, "skin": 0})
        root = self.doc["scenes"][0]["nodes"][0]
        self.doc["nodes"][root]["children"].append(len(self.doc["nodes"]) - 1)

    def compact_material_images(self):
        texture_refs = []
        for material in self.doc["materials"]:
            for properties in (material, material.get("pbrMetallicRoughness", {})):
                texture_refs.extend(value for name, value in properties.items() if name.endswith("Texture"))
        used_textures = sorted({ref["index"] for ref in texture_refs})
        texture_remap = {old: new for new, old in enumerate(used_textures)}
        self.doc["textures"] = [self.doc["textures"][old] for old in used_textures]
        for ref in texture_refs:
            ref["index"] = texture_remap[ref["index"]]
        used_images = sorted({texture["source"] for texture in self.doc["textures"]})
        image_remap = {old: new for new, old in enumerate(used_images)}
        self.doc["images"] = [self.doc["images"][old] for old in used_images]
        for texture in self.doc["textures"]:
            texture["source"] = image_remap[texture["source"]]

        refs = self.doc["images"] + self.doc["accessors"]
        used_views = sorted({ref["bufferView"] for ref in refs})
        view_remap = {old: new for new, old in enumerate(used_views)}
        old_views, old_blob = self.doc["bufferViews"], self.blob
        self.doc["bufferViews"], self.blob = [], bytearray()
        for index in used_views:
            view = old_views[index]
            start = view.get("byteOffset", 0)
            new = self.append_view(old_blob[start:start + view["byteLength"]])
            self.doc["bufferViews"][new].update({key: value for key, value in view.items() if key not in ("byteOffset", "byteLength")})
        for ref in refs:
            ref["bufferView"] = view_remap[ref["bufferView"]]

    def write(self):
        self.compact_material_images()
        metadata = self.doc["asset"]
        metadata["generator"] = "Robot Studio | Mira character showcase"
        metadata["extras"]["title"] = "Mira"
        metadata["extras"]["modifications"] += (
            " Mira uses a custom face profile, high-resolution skin and iris materials,"
            " a fitted scalp groom with tapered loose fibres, a shorter asymmetric ponytail,"
            " pine twill workwear with stitched panels and a brass ear cuff."
        )
        self.blob.extend(b"\0" * ((-len(self.blob)) % 4))
        self.doc["buffers"][0]["byteLength"] = len(self.blob)
        header = json.dumps(self.doc, separators=(",", ":")).encode("utf-8")
        header += b" " * ((-len(header)) % 4)
        with (HERE / "mira.glb").open("wb") as result:
            result.write(struct.pack("<4sIIII", b"glTF", 2, 28 + len(header) + len(self.blob),
                                     len(header), 0x4E4F534A))
            result.write(header)
            result.write(struct.pack("<II", len(self.blob), 0x004E4942))
            result.write(self.blob)


def smooth(low, high, value):
    t = np.clip((value - low) / (high - low), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def unit(vector):
    return vector / np.maximum(np.linalg.norm(vector, axis=-1, keepdims=True), 1e-9)


def profile(points):
    x, y, z = points.T
    jaw = np.exp(-((y - 1.534) / 0.043) ** 2) * smooth(-0.040, 0.029, z)
    nose = np.exp(-((y - 1.582) / 0.018) ** 2 - (x / 0.019) ** 2) * smooth(0.026, 0.060, z)
    tail = (1 - smooth(1.400, 1.618, y)) * (1 - smooth(-0.135, -0.102, z))
    return points + np.column_stack((
        x * jaw * 0.068 + 0.024 * tail,
        0.055 * tail - 0.004 * jaw,
        0.020 * tail - 0.0045 * nose,
    ))


def reshape_head(model):
    for mesh in model.doc["meshes"][:2]:
        for primitive in mesh["primitives"]:
            if primitive["material"] in (2, 5):
                continue
            attributes = primitive["attributes"]
            before = model.array(attributes["POSITION"])
            after = profile(before)
            derivatives = np.stack([
                (profile(before + shift) - profile(before - shift)) / 0.0002
                for shift in np.eye(3, dtype=np.float32) * 0.0001
            ], axis=-1)
            normals = model.array(attributes["NORMAL"])
            normals = unit((np.linalg.inv(derivatives).transpose(0, 2, 1) @ normals[..., None])[..., 0])
            model.replace_array(attributes["POSITION"], after)
            model.replace_array(attributes["NORMAL"], normals)
            for target in primitive.get("targets", []):
                if "POSITION" in target:
                    delta = model.array(target["POSITION"])
                    model.replace_array(target["POSITION"], profile(before + delta) - after)


def image_grid(size):
    position = (np.arange(size, dtype=np.float32) + 0.5) / size
    return np.meshgrid(position, position, sparse=True)


def face_material(model, rng):
    size = 2048
    x, y = image_grid(size)
    original = model.image(1).resize((size, size), Image.Resampling.LANCZOS)
    source = np.asarray(original, dtype=np.float32) / 255.0
    r, g, b = np.moveaxis(source, -1, 0)
    # The central face unwrap has the non-hair skin area in these coordinates.
    cheeks = np.exp(-((np.abs(x - 0.5) - 0.115) / 0.10) ** 2 - ((y - 0.345) / 0.079) ** 2)
    nose = np.exp(-((x - 0.5) / 0.04) ** 2 - ((y - 0.32) / 0.061) ** 2)
    skin = smooth(0.43, 0.60, r) * smooth(0.30, 0.50, b) * (1 - smooth(0.54, 0.62, y))
    dots = Image.new("L", (size, size))
    draw = ImageDraw.Draw(dots)
    for sign in (-1, 1):
        for _ in range(64):
            px = size * (0.5 + sign * rng.uniform(0.014, 0.160))
            py = size * (0.332 + rng.normal(0.0, 0.031))
            radius = rng.uniform(0.75, 2.05)
            draw.ellipse((px-radius, py-radius, px+radius, py+radius),
                         fill=int(rng.integers(45, 206)))
    freckles = np.asarray(dots.filter(ImageFilter.GaussianBlur(0.65)), dtype=np.float32) / 255
    pores = rng.normal(0.0, 1.0, (size, size)).astype(np.float32)
    source += ((cheeks * 0.70 + nose * 0.30) * skin)[..., None] * [0.025, -0.012, -0.015]
    source -= (freckles * skin)[..., None] * [0.043, 0.081, 0.088]
    source += (pores * skin * 0.0035)[..., None]

    # Blend the photo's exposed temple and edge hairs into the groom colour.
    temple = smooth(0.118, 0.28, np.abs(x - 0.5))
    hairline = 0.112 + 0.20 * temple
    hair = 1 - smooth(hairline - 0.013, hairline + 0.021, y)
    hair = np.maximum(hair, smooth(0.62, 0.68, y) * smooth(0.51, 0.60, x))
    source *= 1 - hair[..., None] * [0.46, 0.49, 0.50]

    iris_x, iris_y = (x - 0.263) / 0.020, (y - 0.933) / 0.023
    radius = np.sqrt(iris_x ** 2 + iris_y ** 2)
    angle = np.arctan2(iris_y, iris_x)
    iris = smooth(0.24, 0.48, radius) * (1 - smooth(0.76, 1.0, radius))
    pigment = 0.018 * np.cos(43 * angle + radius * 6) + 0.012 * np.sin(67 * angle - radius * 9)
    iris *= (1 - smooth(0.45, 0.62, g)) * smooth(0.06, 0.20, g)
    source += iris[..., None] * (np.array([-0.032, 0.049, 0.043]) + pigment[..., None])
    face = Image.fromarray(np.uint8(np.clip(source, 0, 1) * 255))
    texture = model.texture(face, "Mira | face and iris")
    for index in (0, 1, 2):
        material = model.doc["materials"][index]["pbrMetallicRoughness"]
        material["baseColorTexture"]["index"] = texture
    model.doc["materials"][2]["pbrMetallicRoughness"]["roughnessFactor"] = 0.25

    normal = np.asarray(model.image(0).resize((size, size), Image.Resampling.LANCZOS), dtype=np.float32)
    pore_y, pore_x = np.gradient(pores)
    normal[..., 0] += pore_x * skin * 2.3
    normal[..., 1] -= pore_y * skin * 2.3
    detail = model.texture(Image.fromarray(np.uint8(np.clip(normal, 0, 255))), "Mira | skin microdetail")
    model.doc["materials"][0]["normalTexture"] = {"index": detail, "scale": 0.69}

    surface = np.asarray(model.image(2).resize((size, size), Image.Resampling.BILINEAR), dtype=np.float32).copy()
    lips = np.exp(-((x - 0.50) / 0.061) ** 4 - ((y - 0.411) / 0.018) ** 4)
    surface[..., 1] += skin * (3.0 * pores + 13 * cheeks - 23 * nose - 26 * lips)
    roughness = Image.fromarray(np.uint8(np.clip(surface, 0, 255))).resize((1024, 1024), Image.Resampling.LANCZOS)
    texture = model.texture(roughness, "Mira | skin roughness")
    model.doc["materials"][0]["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"] = texture
    return face


def garment_seams(size):
    welt, thread, hardware = [Image.new("L", (size, size)) for _ in range(3)]
    welts, stitches, metal = map(ImageDraw.Draw, (welt, thread, hardware))
    paths = [
        [(0.496, 0.548), (0.495, 0.606), (0.494, 0.678), (0.493, 0.744)],
        [(0.505, 0.548), (0.505, 0.608), (0.503, 0.677), (0.503, 0.744)],
        [(0.495, 0.053), (0.496, 0.124), (0.497, 0.200), (0.495, 0.270), (0.488, 0.334)],
        [(0.36, 0.518), (0.36, 0.545), (0.375, 0.550), (0.403, 0.553), (0.414, 0.543), (0.416, 0.520)],
        [(0.578, 0.520), (0.578, 0.543), (0.591, 0.552), (0.622, 0.552), (0.637, 0.544), (0.637, 0.518)],
        [(0.243, 0.293), (0.246, 0.353), (0.246, 0.423), (0.243, 0.489)],
        [(0.754, 0.293), (0.750, 0.354), (0.751, 0.423), (0.755, 0.489)],
        [(0.354, 0.739), (0.412, 0.741), (0.467, 0.744), (0.527, 0.743), (0.593, 0.739), (0.645, 0.737)],
        [(0.362, 0.899), (0.376, 0.844), (0.400, 0.827), (0.441, 0.815), (0.487, 0.812),
         (0.518, 0.812), (0.565, 0.817), (0.603, 0.832), (0.628, 0.856), (0.639, 0.902)],
    ]
    for path in paths:
        points = np.asarray(path) * size
        welts.line([tuple(point) for point in points], fill=202, width=3, joint="curve")
        length = np.concatenate(([0.0], np.cumsum(np.linalg.norm(np.diff(points, axis=0), axis=1))))
        for start in np.arange(2.0, length[-1] - 2.0, 6.7):
            marks = np.column_stack([np.interp([start, start + 2.0], length, values) for values in points.T])
            stitches.line([tuple(mark) for mark in marks], fill=222, width=2)
    for y in np.arange(0.562, 0.738, 0.0029):
        x = np.interp(y, [0.55, 0.74], [0.502, 0.500])
        metal.line((size * (x - 0.0008), size * y, size * (x + 0.001), size * y), fill=146, width=2)
    return tuple(np.asarray(mask, dtype=np.float32) / 255 for mask in (welt, thread, hardware))


def cloth_material(model, rng):
    size = 2048
    raw = np.asarray(model.image(5).resize((size, size), Image.Resampling.LANCZOS), dtype=np.float32) / 255
    r, g, b = np.moveaxis(raw, -1, 0)
    x, y = image_grid(size)
    body = smooth(0.27, 0.32, x) * (1 - smooth(0.67, 0.72, x))
    sleeve = smooth(0.18, 0.22, x) * (1 - smooth(0.78, 0.82, x))
    sleeve = sleeve * smooth(0.23, 0.28, y) * (1 - smooth(0.49, 0.55, y))
    fabric = np.maximum(body, sleeve) * smooth(0.029, 0.090, r - g) * (1 - smooth(0.027, 0.105, g - b))
    collar = smooth(0.767, 0.788, y) * body
    luma = r * 0.24 + g * 0.65 + b * 0.11
    warp = np.cos(math.tau * (x * 277 + y * 4))
    twill = np.cos(math.tau * (y * 245 + x * 136) + 0.67 * warp)
    weave = 0.7 * twill + 0.3 * warp
    wear = 0.5 + 0.5 * np.sin(x * 149 + y * 37 + np.cos(y * 112))
    finish = np.stack((luma * 0.42 + 0.045, luma * 0.81 + 0.064, luma * 0.70 + 0.047), axis=-1)
    lining = np.stack((luma * 0.43 + 0.058, luma * 0.47 + 0.065, luma * 0.43 + 0.060), axis=-1)
    finish = finish * (1 - collar[..., None]) + lining * collar[..., None]
    welt, stitch, zipper = garment_seams(size)
    finish += (weave * 0.009 + wear * 0.005 + rng.normal(0, 0.0025, (size, size)))[..., None]
    finish -= welt[..., None] * [0.021, 0.035, 0.025]
    finish += stitch[..., None] * [0.062, 0.079, 0.062] + zipper[..., None] * [0.061, 0.051, 0.026]
    mixed = raw * (1 - fabric[..., None]) + finish * fabric[..., None]
    result = Image.fromarray(np.uint8(np.clip(mixed, 0, 1) * 255))
    texture = model.texture(result, "Mira | stitched pine twill")
    top = model.doc["materials"][6]["pbrMetallicRoughness"]
    top["baseColorTexture"]["index"] = texture
    top["baseColorFactor"] = [1.0, 1.0, 1.0, 1.0]
    top["roughnessFactor"] = 1.0

    def half(channel):
        return channel.reshape(1024, 2, 1024, 2).mean(axis=(1, 3))

    bump = half((0.030 * weave + 0.15 * stitch - 0.095 * welt + 0.075 * zipper) * fabric)
    dy, dx = np.gradient(bump)
    normal = np.asarray(model.image(4), dtype=np.float32)
    normal[..., 0] -= 142 * dx
    normal[..., 1] += 142 * dy
    cloth_normal = model.texture(Image.fromarray(np.uint8(np.clip(normal, 0, 255))), "Mira | twill and seam relief")

    surface = np.asarray(model.image(6), dtype=np.float32)
    rough = 204 + weave * 11 + collar * 18 - zipper * 113
    surface[..., 1] = surface[..., 1] * (1 - half(fabric)) + half(rough * fabric)
    cloth_surface = model.texture(Image.fromarray(np.uint8(np.clip(surface, 0, 255))), "Mira | fabric roughness and zipper")
    for index in (6, 7, 8, 9):
        material = model.doc["materials"][index]
        material["normalTexture"]["index"] = cloth_normal
        material["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"] = cloth_surface


def hair_material(model):
    raw = np.asarray(model.image(3), dtype=np.float32) / 255
    raw *= np.array([0.59, 0.54, 0.53], dtype=np.float32)
    texture = model.texture(Image.fromarray(np.uint8(raw * 255)), "Mira | auburn hair")
    for index in (3, 4):
        material = model.doc["materials"][index]["pbrMetallicRoughness"]
        material["baseColorTexture"]["index"] = texture
        material["roughnessFactor"] = 0.73


def intersect_scalp(model, origin, directions):
    head = model.doc["meshes"][0]["primitives"][0]
    indices = model.array(head["indices"]).reshape(-1, 3)
    triangles = model.array(head["attributes"]["POSITION"])[indices]
    atlas = model.array(head["attributes"]["TEXCOORD_0"])[indices]
    # The same primitive also contains ponytail islands and the neck.
    keep = (atlas[:, :, 1].max(axis=1) < 0.47) & (triangles[:, :, 1].max(axis=1) > 1.54)
    triangles = triangles[keep]
    atlas = atlas[keep]
    edges = triangles[:, 1:] - triangles[:, :1]
    to_origin = origin - triangles[:, 0]
    q = np.cross(to_origin, edges[:, 0])
    d = np.sum(q * edges[:, 1], axis=1)

    result, source_uv = [], []
    for ray in np.array_split(directions.reshape(-1, 3), 24):
        cross = np.cross(ray[:, None], edges[None, :, 1])
        determinant = np.sum(cross * edges[None, :, 0], axis=2)
        inverse = np.divide(1, determinant, out=np.zeros_like(determinant), where=np.abs(determinant) > 1e-9)
        u = np.sum(cross * to_origin, axis=2) * inverse
        v = (ray @ q.T) * inverse
        distance = d * inverse
        hit = (u >= -1e-6) & (v >= -1e-6) & (u + v <= 1 + 1e-6) & (distance > 0.05) & (distance < 0.144)
        distance = np.where(hit, distance, np.inf)
        nearest = np.argmin(distance, axis=1)
        ray_id = np.arange(len(ray))
        result.extend(distance[ray_id, nearest])
        u_hit = u[ray_id, nearest, None]
        v_hit = v[ray_id, nearest, None]
        coords = atlas[nearest]
        projected = coords[:, 0] * (1 - u_hit - v_hit) + coords[:, 1] * u_hit + coords[:, 2] * v_hit
        source_uv.extend(projected)
    return np.asarray(result).reshape(directions.shape[:2]), np.asarray(source_uv).reshape(*directions.shape[:2], 2)


def wrap_texture(texture, x, y):
    origin_x, origin_y = np.floor(x).astype(int), np.floor(y).astype(int)
    last_y = np.clip(origin_y + 1, 0, texture.shape[0] - 1)
    next_x = (origin_x + 1) % texture.shape[1]
    xfrac, yfrac = (x - origin_x)[..., None], (y - origin_y)[..., None]
    corners = (texture[origin_y, origin_x], texture[origin_y, next_x],
               texture[last_y, origin_x], texture[last_y, next_x])
    upper = corners[0] * (1 - xfrac) + corners[1] * xfrac
    lower = corners[2] * (1 - xfrac) + corners[3] * xfrac
    return upper * (1 - yfrac) + lower * yfrac


def project_for_hairline(face, source_uv, found, u, v):
    safe = np.where(found[..., None], source_uv, -1)
    for axis in (0, 1):
        neighbor = np.roll(safe, -1, axis=axis)
        seam = (np.abs(safe - neighbor).max(axis=2) > 0.20) | (neighbor[..., 0] < 0)
        found &= ~seam
    photo_uv = wrap_texture(safe, u * safe.shape[1], v * (safe.shape[0] - 1))
    photo_uv = np.clip(photo_uv, 0, 1) * (face.width - 1)
    photo = wrap_texture(np.asarray(face), photo_uv[..., 0], photo_uv[..., 1])
    coverage = wrap_texture(found[..., None].astype(float), u * safe.shape[1], v * (safe.shape[0] - 1))[..., 0]
    return photo, smooth(0.72, 0.98, coverage)


def scalp_material(model, rng, face, source_uv, found):
    width, height = 2048, 1024
    u, v = np.meshgrid((np.arange(width) + 0.5) / width, (np.arange(height) + 0.5) / height, sparse=True)
    phase = math.tau * u
    flow = phase + v * (0.093 * np.sin(phase * 3 + v * 2) + 0.056 * np.sin(phase * 7 - v * 4))
    flow += 0.018 * np.sin(phase * 13 + v * 8) * (1 - v)
    root = smooth(0.0, 0.23, v) * (1 - smooth(0.77, 1.0, v) * 0.17)
    locks = 0.31 * np.cos(flow * 21 + 1.05 * np.sin(phase * 8 + 3 * v))
    locks += 0.21 * np.cos(flow * 37 + v * 4.9) + 0.15 * np.cos(flow * 45 + v * 9.4)
    fibres = 0.23 * np.sin(flow * 139 + np.cos(flow * 23)) + 0.18 * np.sin(flow * 211 + v * 6.3)
    fibres += 0.10 * np.cos(flow * 333 + np.sin(flow * 29) * 0.9)
    fibres = fibres * (0.65 + 0.35 * np.cos(phase * 5 + v * 2.3) ** 2)
    grain = rng.uniform(-0.022, 0.022, (height, width))
    pigment = (locks * 0.28 + fibres * 0.42 + grain) * root
    diffuse = np.array([105, 70, 54]) + (pigment * 103 + root * 12)[..., None] * [1.0, 0.74, 0.58]
    photo, projection = project_for_hairline(face, source_uv, found, u, v)
    boundary = (1 - smooth(0.095, 0.30, v)) * projection
    texture_mix = boundary + 0.34 * projection * (1 - boundary) * smooth(0.18, 0.39, v)
    diffuse = diffuse * (1 - texture_mix[..., None]) + photo * texture_mix[..., None]
    # Preserve the original quiet baby-hair transition, with an irregular set of tips above it.
    fringe = 0.032 + 0.012 * np.sin(phase * 21) + 0.006 * np.sin(phase * 107 + 0.3)
    fringe += 0.008 * np.sin(phase * 77 + 0.8 * np.sin(phase * 17))
    alpha = 1 - projection * (1 - smooth(fringe - 0.016, fringe + 0.019, v))
    diffuse = np.dstack((diffuse, alpha * 255))
    base = model.texture(Image.fromarray(np.uint8(np.clip(diffuse, 0, 255))), "Mira | chestnut hair fibres")

    fibre_height = (locks * 0.28 + fibres * 0.25) * root
    across = (np.roll(fibre_height, -1, axis=1) - np.roll(fibre_height, 1, axis=1)) * -0.61
    along = -np.gradient(fibre_height, axis=0) * 0.33
    normal = unit(np.stack((across, along, np.ones_like(across)), axis=2))
    fibre_map = Image.fromarray(np.uint8((normal * 0.5 + 0.5) * 255)).resize((1024, 512), Image.Resampling.LANCZOS)
    normal = model.texture(fibre_map, "Mira | hair fibre normal")
    material = len(model.doc["materials"])
    model.doc["materials"].append({
        "name": "Studio | scalp",
        "alphaMode": "MASK",
        "alphaCutoff": 0.45,
        "normalTexture": {"index": normal, "scale": 0.82},
        "pbrMetallicRoughness": {
            "baseColorTexture": {"index": base},
            "metallicFactor": 0,
            "roughnessFactor": 0.73,
        },
    })
    return material


def create_scalp(model, rng, face):
    columns, rows = 128, 48
    u = np.arange(columns) / columns
    v = np.linspace(0.0, 1.0, rows)[:, None]
    theta = u * math.tau
    sector = np.abs((theta + math.pi) % math.tau - math.pi)
    # The front edge follows the photographed hairline; the rear tucks under the tail.
    hairline = np.interp(sector, [0, 0.39, 0.74, 1.12, 1.48, 1.8, 2.28, 2.68, math.pi],
                         [2.29, 2.30, 2.14, 1.88, 1.73, 1.67, 1.49, 1.30, 1.19])
    hairline += 0.014 * np.sin(theta * 3 + 0.7) + 0.007 * np.sin(theta * 17)
    theta = theta + v ** 0.8 * (0.19 + 0.11 * np.sin(theta * 3 - 1.3))
    phi = hairline * (1 - v) + v * 0.018
    origin = np.array([0, 1.644, -0.043])
    pole = np.array([0, 0.4, -0.916515])
    crest = np.array([0, 0.916515, 0.4])
    around_pole = np.cos(theta)[..., None] * crest + np.sin(theta)[..., None] * [1, 0, 0]
    directions = np.cos(phi)[..., None] * pole + np.sin(phi)[..., None] * around_pole
    radii, source_uv = intersect_scalp(model, origin, directions)
    found = np.isfinite(radii)
    # Bridge the rear split in the source head by extending its flanking contours.
    for col in range(columns):
        known = found[:, col]
        if not np.any(known):
            raise ValueError(f"no head intersection along scalp column {col}")
        radii[~known, col] = np.interp(v[~known, 0], v[known, 0], radii[known, col])
    raw = radii.copy()
    for _ in range(3):
        radii = (2 * radii + np.roll(radii, 1, axis=1) + np.roll(radii, -1, axis=1)) * 0.25
        radii[1:-1] = (2 * radii[1:-1] + radii[2:] + radii[:-2]) * 0.25
    radii = np.maximum(radii, raw - 0.0007)

    combed = (0.56 * np.sin(theta * 13 + v * 3) + 0.26 * np.cos(theta * 26 - v * 4.4))
    lift = smooth(0.0, 0.22, v) * (1 - 0.90 * smooth(0.76, 1.0, v))
    radii += 0.00065 + lift * (0.0044 + 0.00125 * combed)
    position = origin + directions * radii[..., None]
    along = np.gradient(position, axis=0)
    across = np.roll(position, -1, axis=1) - np.roll(position, 1, axis=1)
    normals = unit(np.cross(along, across))
    normals *= np.sign(np.sum(normals * directions, axis=2))[..., None]
    normals = unit(normals * 0.75 + directions * 0.25)
    position = np.concatenate((position, position[:, :1]), axis=1)
    normals = np.concatenate((normals, normals[:, :1]), axis=1)
    uv = np.stack(np.meshgrid(np.linspace(0.0, 1.0, columns + 1), v[:, 0]), axis=2)
    first = np.arange((rows - 1) * (columns + 1)).reshape(rows - 1, columns + 1)[:, :-1].ravel()
    next_row = first + columns + 1
    indices = np.stack((first, first + 1, next_row, next_row, first + 1, next_row + 1), axis=1).reshape(-1, 3)
    # Close the tiny ring beneath the existing ponytail attachment.
    apex = np.mean(position[-1, :-1], axis=0) + pole * 0.0004
    positions = np.vstack((position.reshape(-1, 3), apex))
    normals = np.vstack((normals.reshape(-1, 3), pole))
    uv = np.vstack((uv.reshape(-1, 2), [0.5, 1.0]))
    ring = (rows - 1) * (columns + 1) + np.arange(columns)
    cap = np.stack((ring, ring + 1, np.full(columns, len(positions) - 1)), axis=1)
    indices = np.vstack((indices, cap)).reshape(-1, 3)
    a, b, c = positions[indices[len(indices) // 3]]
    if np.dot(np.cross(b - a, c - a), a - origin) < 0:
        indices = indices[:, ::-1]
    material = scalp_material(model, rng, face, source_uv, found)
    model.add_head_mesh("Mira | fitted scalp", positions, normals, uv, indices, material)
    return position[:, :-1], normals[:-1].reshape(rows, columns + 1, 3)[:, :-1]


def short_hair_guides(scalp, rng, time):
    position, normals = scalp
    rows, columns = position.shape[:2]
    for patch in (0.15, 0.245, 0.35, 0.64, 0.755, 0.85):
        for _ in range(4):
            u = patch + rng.uniform(-0.026, 0.026)
            u = u + rng.uniform(-0.045, 0.047) * time + 0.008 * np.sin(math.pi * time)
            v = rng.uniform(0.145, 0.245) * (1 - time) - rng.uniform(0.014, 0.061) * time
            phase = (u % 1.0) * columns
            y = np.maximum(v, 0) * (rows - 1)
            point = wrap_texture(position, phase, y)
            normal = unit(wrap_texture(normals, phase, y))
            # The free end follows the scalp tangent a few millimetres past the hairline.
            edge = wrap_texture(position, phase, np.zeros_like(v))
            at_row_one = wrap_texture(position, phase, np.ones_like(v))
            point += (at_row_one - edge) * (np.minimum(v, 0) * (rows - 1))[..., None]
            curl = (0.00035 + rng.uniform(0.0012, 0.0036) * np.sin(time * math.pi * 0.76))[..., None]
            yield point + normal * curl, normal, rng.uniform(0.00055, 0.0016)


def cubic_hair(points, facing, width, time):
    p0, p1, p2, p3 = np.asarray(points, dtype=np.float32)
    t = time[..., None]
    curve = p0 * (1 - t)**3 + p1 * (3 * t * (1 - t)**2) + p2 * (3 * t**2 * (1 - t)) + p3 * t**3
    return curve, np.broadcast_to(facing, curve.shape), width


def create_wisps(model, rng, scalp):
    width, height = 160, 384
    x, y = np.meshgrid(np.linspace(0, 1, width), np.linspace(0, 1, height))
    channels = np.zeros((height, width, 4), dtype=np.uint8)
    signal = np.zeros((height, width))
    glint = np.zeros_like(signal)
    for _ in range(24):
        center = rng.uniform(0.08, 0.92)
        phase = rng.uniform(0, math.tau)
        form = center + 0.022 * np.sin(y * 11 + phase) * y
        fiber = np.exp(-((x - form) / rng.uniform(0.007, 0.022)) ** 2)
        fiber *= 1 - smooth(rng.uniform(0.48, 0.91), 0.998, y)
        signal = np.maximum(signal, fiber)
        glint += fiber * rng.uniform(0.04, 0.13)
    for channel, floor in enumerate((82, 49, 35)):
        channels[..., channel] = np.uint8(np.clip(floor + glint * (123 - 24 * channel), 0, 255))
    channels[..., 3] = np.uint8(signal * smooth(0.0, 0.12, y) * 255)
    texture = model.texture(Image.fromarray(channels), "Mira | fine loose fibres")
    time = np.linspace(0.0, 1.0, 18)
    short = list(short_hair_guides(scalp, rng, time))
    short.extend([
        cubic_hair([[-0.065,1.671,-0.024], [-0.079,1.672,-0.031], [-0.085,1.637,-0.024], [-0.084,1.616,-0.038]],
                   [-0.75, 0.0, 0.66], 0.00215, time),
        cubic_hair([[0.065,1.673,-0.038], [0.080,1.668,-0.046], [0.089,1.646,-0.058], [0.085,1.621,-0.064]],
                   [0.75, 0.0, 0.66], 0.00190, time),
    ])
    tail_path = np.array([[0.012,1.632,-0.193], [0.046,1.607,-0.233], [0.051,1.533,-0.211], [0.057,1.487,-0.199]])
    tail = [cubic_hair(tail_path, [0.15, 0.0, 1.0], 0.003, time)]
    for _ in range(5):
        offset = rng.uniform(-0.01, 0.01, (4, 3)) * np.array([0.72, 0.43, 0.78])
        tail.append(cubic_hair(tail_path + offset, [0.1, 0.0, 1.0], rng.uniform(0.0007, 0.0014), time))
    for name, region, guides in [
        ("Mira | short loose strands", "Studio | hair wisp", short),
        ("Mira | fine tail ends", "Studio | hair tip", tail),
    ]:
        material = len(model.doc["materials"])
        model.doc["materials"].append({
            "name": region,
            "doubleSided": True,
            "alphaMode": "MASK",
            "alphaCutoff": 0.43,
            "pbrMetallicRoughness": {
                "baseColorTexture": {"index": texture},
                "metallicFactor": 0,
                "roughnessFactor": 0.58,
            },
        })
        position, normals, uv, indices = [], [], [], []
        for centers, facing, width in guides:
            tangent = unit(np.gradient(centers, axis=0))
            sides = unit(np.cross(tangent, facing))
            surface = unit(np.cross(sides, tangent))
            start = len(position)
            for i, (t, center, side, normal) in enumerate(zip(time, centers, sides, surface)):
                span = (width * (1 - t)**0.72 + 0.00009) * (0.61 + smooth(0, 0.22, t) * 0.39)
                position.extend((center-side*span, center+side*span))
                normals.extend((normal, normal))
                uv.extend(([0,t],[1,t]))
                j = start + 2*i
                if i < len(time) - 1:
                    indices.extend((j,j+1,j+2,j+2,j+1,j+3))
        model.add_head_mesh(name, position, normals, uv, indices, material)


def create_ear_cuff(model):
    material = len(model.doc["materials"])
    model.doc["materials"].append({
        "name": "Mira | weathered brass ear cuff",
        "pbrMetallicRoughness": {
            "baseColorFactor": [0.56, 0.32, 0.12, 1],
            "metallicFactor": 0.78,
            "roughnessFactor": 0.34,
        },
    })
    position, normals, uv, indices = [], [], [], []
    center = np.array([-0.087, 1.591, -0.057])
    for i in range(21):
        angle = -0.54 + i * 5.38 / 20
        radial = np.array([0.0, math.cos(angle), math.sin(angle)])
        for j in range(7):
            around = j * math.tau / 6
            normal = radial * math.cos(around) + [math.sin(around), 0, 0]
            position.append(center + radial * 0.0083 + normal * 0.00145)
            normals.append(normal)
            uv.append([i / 20, j / 6])
            p = i*7+j
            if i < 20 and j < 6:
                indices.extend((p,p+7,p+1,p+1,p+7,p+8))
    model.add_head_mesh("Mira | brass ear cuff", position, normals, uv, indices, material)


if __name__ == "__main__":
    rng = np.random.default_rng(71218)
    model = Model()
    reshape_head(model)
    face = face_material(model, rng)
    cloth_material(model, rng)
    hair_material(model)
    scalp = create_scalp(model, rng, face)
    create_wisps(model, rng, scalp)
    create_ear_cuff(model)
    model.write()

