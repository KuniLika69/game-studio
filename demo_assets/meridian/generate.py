"""Generate the original low-poly route scenery with only Python's standard library."""

from collections import defaultdict
import json
from math import cos, exp, hypot, pi, sin, sqrt
from pathlib import Path
from random import Random
import struct


def normal(a, b, c):
    x, y, z = (b[i] - a[i] for i in range(3))
    u, v, w = (c[i] - a[i] for i in range(3))
    n = (y * w - z * v, z * u - x * w, x * v - y * u)
    length = sqrt(sum(t * t for t in n)) or 1
    return tuple(t / length for t in n)


def terrain(name, seed, grass, trail, hilliness):
    rng = Random(seed)
    bins = defaultdict(list)

    def triangle(color, a, b, c):
        n = normal(a, b, c)
        bins[color].extend((*p, *n) for p in (a, b, c))

    def route_x(z):
        if name == "ridge":
            return 0.55 * sin(z * 0.25)
        return 0.35 * sin(z * 0.21)

    def height(x, z):
        plateau = min(1, max(0, (abs(x - route_x(z)) - 2.8) / 5))
        if name == "station" and 0 < x < 8 and -9 < z < 3:
            plateau *= 0
        hill = exp(-((abs(x) - 13) / 10) ** 2) * (0.42 + 0.32 * cos(z * .21 + seed))
        far = max(0, min(1, (abs(x) - 19) / 19)) * (2.5 + 1.65 * sin(z * .13))
        return 0.013 + plateau * (hilliness * (hill + far) + 0.24 * sin(x * .55) * sin(z * .7))

    def shade(color, gain):
        return tuple(round(max(0.018, min(.92, c * gain)), 4) for c in color)

    grass_colors = [shade(grass, .78 + i * .036) for i in range(13)]
    path_colors = [shade(trail, .79 + i * .056) for i in range(8)]
    for z in range(-54, 13):
        for x in range(-34, 34):
            xoff = abs(x + .5 - route_x(z + .5))
            on_path = xoff < (2.05 + .29 * cos(z * .55)) or (
                name == "station" and -7 < z < -1 and 0 < x < 7
            )
            palette = path_colors if on_path else grass_colors
            value = int((len(palette) - 1) * (.44 + .20 * sin(x * .37 + z * .48) + .31 * rng.random()))
            a = (x, height(x, z), z)
            b = (x, height(x, z + 1), z + 1)
            c = (x + 1, height(x + 1, z + 1), z + 1)
            d = (x + 1, height(x + 1, z), z)
            triangle(palette[value], a, b, c)
            triangle(palette[min(value + rng.randint(-1, 1), len(palette) - 1)], a, c, d)

    def cone(x, y, z, radius, height_y, color, count):
        crown = (x, y + height_y, z)
        ring = [
            (x + cos(i * 2 * pi / count) * radius, y, z + sin(i * 2 * pi / count) * radius)
            for i in range(count)
        ]
        for i, a in enumerate(ring):
            triangle(shade(color, .9 + .1 * (i % 3)), a, crown, ring[(i + 1) % count])

    for index in range(117):
        x = rng.choice([-1, 1]) * rng.uniform(8.6, 33.0)
        z = rng.uniform(-50, 8)
        y = height(x, z)
        t = rng.uniform(1.6, 4.5)
        tree_color = ((.055, .17, .12), (.092, .22, .14), (.12, .24, .16))[index % 3]
        cone(x, y, z, .13 * t, t, (.15, .105, .058), 6)
        for i in range(3):
            cone(x, y + (.28 + i * .18) * t, z, (.33 - i * .055) * t, .51 * t, tree_color, 7)

    for index in range(204):
        x = rng.choice([-1, 1]) * rng.uniform(2.6, 15)
        z = rng.uniform(-30, 6)
        if name == "station" and 0 < x < 7 and -8 < z < 2:
            continue
        y = height(x, z) + .01
        t = rng.uniform(.06, .25)
        petal = ((.34, .42, .09), (.49, .40, .095), (.51, .29, .25))[index % 3]
        for theta in [rng.random() * 2 * pi, 1.9, 3.3]:
            xd, zd = cos(theta) * t, sin(theta) * t
            a, b = (x - xd * .21, y, z - zd * .21), (x + xd * .21, y, z + zd * .21)
            c = (x + t * .18, y + t, z - t * .19)
            triangle(petal, a, b, c)
            triangle(shade(petal, .84), b, a, c)
    glb(name, bins)


def glb(name, bins, output=None):
    gltf = {
        "asset": {"version": "2.0", "generator": "Studio Games · Meridian procedural terrain"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"name": name, "primitives": []}], "materials": [],
        "bufferViews": [], "accessors": [], "buffers": [],
    }
    binary = bytearray()
    for color, rows in bins.items():
        material = len(gltf["materials"])
        gltf["materials"].append({"pbrMetallicRoughness": {
            "baseColorFactor": [*color, 1], "roughnessFactor": .91, "metallicFactor": 0
        }})
        slot = len(gltf["bufferViews"])
        start = len(binary)
        for line in rows:
            binary.extend(struct.pack("<6f", *line))
        gltf["bufferViews"].append({
            "buffer": 0, "byteOffset": start, "byteLength": len(binary) - start,
            "byteStride": 24, "target": 34962,
        })
        pos = len(gltf["accessors"])
        gltf["accessors"].append({
            "bufferView": slot, "componentType": 5126, "count": len(rows), "type": "VEC3",
            "min": [min(line[i] for line in rows) for i in range(3)],
            "max": [max(line[i] for line in rows) for i in range(3)],
        })
        gltf["accessors"].append({
            "bufferView": slot, "byteOffset": 12, "componentType": 5126,
            "count": len(rows), "type": "VEC3",
        })
        gltf["meshes"][0]["primitives"].append({
            "attributes": {"POSITION": pos, "NORMAL": pos + 1}, "material": material
        })
    gltf["buffers"] = [{"byteLength": len(binary)}]
    text = json.dumps(gltf, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    output = output or Path(__file__).parent / (name + ".glb")
    with output.open("wb") as out:
        out.write(struct.pack("<4sII", b"glTF", 2, total))
        out.write(struct.pack("<I4s", len(text), b"JSON"))
        out.write(text)
        out.write(struct.pack("<I4s", len(binary), b"BIN\x00"))
        out.write(binary)


if __name__ == "__main__":
    terrain("station", 12, (.19, .32, .125), (.47, .38, .235), 1.1)
    terrain("waterfront", 41, (.27, .41, .152), (.50, .435, .27), 1.35)
    terrain("ridge", 27, (.29, .31, .16), (.52, .46, .34), 2.0)
