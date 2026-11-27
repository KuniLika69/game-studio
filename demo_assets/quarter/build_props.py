"""Build the original street furniture and workshop sign used by the 0.13 scene."""

import io
import json
import math
import random
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


OUTPUT = Path(__file__).resolve().parent


def material(name, color, roughness, metal=0.0):
    return dict(
        name=name,
        pbrMetallicRoughness=dict(baseColorFactor=[*color, 1.0],
                                  metallicFactor=metal, roughnessFactor=roughness),
    )


class Mesh:
    def __init__(self):
        self.materials = []
        self.vertices = []

    def add_material(self, name, color, roughness, metal=0.0):
        self.materials.append(material(name, color, roughness, metal))
        self.vertices.append([])
        return len(self.materials) - 1

    def quad(self, points, mat, uv=((0, 1), (1, 1), (1, 0), (0, 0))):
        a, b, c, _ = points
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        n = [u[(i + 1) % 3] * v[(i + 2) % 3] - u[(i + 2) % 3] * v[(i + 1) % 3]
             for i in range(3)]
        length = math.sqrt(sum(x * x for x in n))
        if length:
            normal = [x / length for x in n]
            self.vertices[mat].extend((*points[i], *normal, *uv[i]) for i in (0, 1, 2, 0, 2, 3))

    def box(self, center, size, mat):
        x, y, z = center
        a, b, c = (extent * 0.5 for extent in size)
        vertices = [(x + a * sx, y + b * sy, z + c * sz)
                    for sx, sy, sz in ((-1, -1, -1), (1, -1, -1), (1, 1, -1), (-1, 1, -1),
                                       (-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1))]
        for face in ((1, 0, 3, 2), (4, 5, 6, 7), (0, 4, 7, 3),
                     (5, 1, 2, 6), (3, 7, 6, 2), (0, 1, 5, 4)):
            self.quad([vertices[i] for i in face], mat)

    def loft(self, center, rings, sides, mat, capped=True):
        cx, cy, cz = center
        def point(ring, i):
            y, rx, rz = ring
            angle = math.tau * i / sides
            return (cx + math.cos(angle) * rx, cy + y, cz + math.sin(angle) * rz)
        for bottom, top in zip(rings, rings[1:]):
            for i in range(sides):
                self.quad((point(bottom, i + 1), point(bottom, i), point(top, i), point(top, i + 1)), mat)
        if capped:
            for i in range(sides):
                bottom, top = rings[0], rings[-1]
                low = (cx, cy + bottom[0], cz)
                high = (cx, cy + top[0], cz)
                self.quad((low, point(bottom, i), point(bottom, i + 1), low), mat)
                self.quad((high, point(top, i + 1), point(top, i), high), mat)

    def save(self, name, texture=None):
        doc = dict(asset=dict(version="2.0", generator="Студия игр, уличная мебель"),
                   scene=0, scenes=[dict(nodes=[0])], nodes=[dict(mesh=0, name=name)],
                   materials=self.materials, bufferViews=[], accessors=[], meshes=[])
        data = bytearray()

        def view(blob, stride=None):
            data.extend(bytes((-len(data)) % 4))
            entry = dict(buffer=0, byteOffset=len(data), byteLength=len(blob))
            if stride:
                entry.update(byteStride=stride, target=34962)
            doc["bufferViews"].append(entry)
            data.extend(blob)
            return len(doc["bufferViews"]) - 1

        primitives = []
        for mat, vertices in enumerate(self.vertices):
            if not vertices:
                continue
            buffer = view(b"".join(struct.pack("<8f", *vertex) for vertex in vertices), 32)
            attrs = {}
            for key, offset, vec in (("POSITION", 0, "VEC3"), ("NORMAL", 12, "VEC3"),
                                     ("TEXCOORD_0", 24, "VEC2")):
                attrs[key] = len(doc["accessors"])
                accessor = dict(bufferView=buffer, byteOffset=offset, componentType=5126,
                                count=len(vertices), type=vec)
                if key == "POSITION":
                    accessor.update(min=[min(v[i] for v in vertices) for i in range(3)],
                                    max=[max(v[i] for v in vertices) for i in range(3)])
                doc["accessors"].append(accessor)
            primitives.append(dict(attributes=attrs, material=mat, mode=4))
        doc["meshes"].append(dict(primitives=primitives))
        if texture:
            image = io.BytesIO()
            texture[1].save(image, format="PNG")
            doc["images"] = [dict(bufferView=view(image.getvalue()), mimeType="image/png")]
            doc["textures"] = [dict(source=0)]
            doc["materials"][texture[0]]["pbrMetallicRoughness"]["baseColorTexture"] = dict(index=0)
        data.extend(bytes((-len(data)) % 4))
        doc["buffers"] = [dict(byteLength=len(data))]
        header = json.dumps(doc, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        header += b" " * ((-len(header)) % 4)
        glb = (struct.pack("<4sII", b"glTF", 2, 28 + len(header) + len(data))
               + struct.pack("<I4s", len(header), b"JSON") + header
               + struct.pack("<I4s", len(data), b"BIN\0") + data)
        (OUTPUT / f"{name}.glb").write_bytes(glb)


def lantern():
    obj = Mesh()
    metal = obj.add_material("Графитовая сталь", (0.07, 0.10, 0.11), .42, .70)
    rim = obj.add_material("Литое основание", (.12, .17, .17), .55, .55)
    glass = obj.add_material("Опаловое стекло", (.68, .57, .37), .26, .10)
    obj.materials[glass]["emissiveFactor"] = [.20, .13, .06]
    obj.loft((0, 0, 0), [(0, .18, .18), (.07, .18, .18), (.12, .13, .13),
                            (.25, .11, .11), (.32, .075, .075)], 12, rim)
    obj.loft((0, 0, 0), [(.25, .06, .06), (2.75, .045, .045),
                            (3.42, .039, .039)], 12, metal)
    obj.loft((0, 0, 0), [(3.33, .053, .053), (3.40, .062, .062),
                            (3.51, .025, .025)], 12, rim)
    obj.box((0, 3.34, .26), (.063, .063, .52), metal)
    obj.box((0, 3.29, .51), (.072, .15, .07), metal)
    obj.loft((0, 3.04, .51), [(0, .09, .09), (.12, .145, .145),
                                 (.20, .145, .145)], 8, glass)
    obj.loft((0, 3.21, .51), [(0, .16, .16), (.055, .16, .16), (.13, .067, .067)], 8, metal)
    obj.loft((0, 3.00, .51), [(0, .063, .063), (.04, .10, .10)], 8, rim)
    obj.save("lantern")


def planter():
    obj = Mesh()
    stone = obj.add_material("Шлифованный бетон", (.29, .31, .29), .83)
    rim = obj.add_material("Сколы бетона", (.39, .42, .40), .88)
    earth = obj.add_material("Мульча", (.09, .056, .031), .98)
    greens = [obj.add_material("Листва", color, .69)
              for color in ((.15, .24, .10), (.23, .34, .12), (.34, .44, .17), (.12, .20, .13))]
    obj.box((0, .18, 0), (1.28, .36, .61), stone)
    obj.box((0, .06, 0), (1.35, .10, .68), rim)
    obj.box((0, .37, 0), (1.37, .09, .71), rim)
    obj.box((0, .419, 0), (1.19, .012, .51), earth)
    rng = random.Random(813)
    for _ in range(138):
        x, z = rng.uniform(-.53,.53), rng.uniform(-.19,.19)
        spread, height = rng.uniform(.06,.25), rng.uniform(.15,.45)
        angle = rng.uniform(0, math.tau)
        dx, dz = math.cos(angle), math.sin(angle)
        y = .425
        tip = (x + dx * spread, y + height * .62, z + dz * spread)
        middle = (x + dx * spread * .48, y + height * .82, z + dz * spread * .48)
        width = rng.uniform(.024,.062)
        left = (middle[0] - dz * width, middle[1], middle[2] + dx * width)
        right = (middle[0] + dz * width, middle[1] - .018, middle[2] - dx * width)
        points = [(x, y, z), left, tip, right]
        index = rng.choice(greens)
        obj.quad(points, index)
        obj.quad(points[::-1], index)
    obj.save("planter")


def bench():
    obj = Mesh()
    metal = obj.add_material("Литая сталь", (.08, .11, .12), .43, .69)
    for x in (-.75, .75):
        obj.box((x, .23, .16), (.088, .44, .090), metal)
        obj.box((x, .43, -.19), (.088, .84, .090), metal)
        obj.box((x, .42, 0), (.11, .075, .56), metal)
        obj.box((x, .64, .035), (.084, .052, .48), metal)
    woods = [obj.add_material("Лиственница", color, .65) for color in
             ((.37, .21, .10), (.42, .24, .12), (.33, .18, .08))]
    for i in range(6):
        obj.box((0, .46, -.22 + .082 * i), (1.84, .047, .072), woods[i % 3])
    for i in range(4):
        obj.box((0, .56 + .10 * i, -.21), (1.84, .085, .049), woods[i % 3])
    obj.save("bench")


def sign():
    obj = Mesh()
    edge = obj.add_material("Окрашенная сталь", (.08, .18, .17), .57, .62)
    enamel = obj.add_material("Эмалированная вывеска", (1, 1, 1), .48)
    obj.box((0, 0.22, 0), (2.21, .44, .072), edge)
    obj.quad(((-1.07, .035, .037), (1.07, .035, .037),
              (1.07, .405, .037), (-1.07, .405, .037)), enamel)
    bitmap = Image.new("RGB", (1024, 192), (28, 49, 48))
    draw = ImageDraw.Draw(bitmap)
    font = Path("C:/Windows/Fonts/bahnschrift.ttf")
    draw.rectangle((13, 10, 1010, 181), outline=(144, 159, 126), width=3)
    draw.multiline_text((511, 82), "МАСТЕРСКАЯ   04", font=ImageFont.truetype(f"{font}", 70),
                        anchor="mm", fill=(230, 218, 173))
    draw.text((510, 143), "Л И П О В Ы Й   П Р О Е З Д", font=ImageFont.truetype(f"{font}", 23),
              anchor="mm", fill=(157, 174, 148))
    obj.save("workshop_sign", (enamel, bitmap))


if __name__ == "__main__":
    lantern()
    planter()
    bench()
    sign()
