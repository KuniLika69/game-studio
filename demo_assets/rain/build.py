"""Build the original Rain Workshop meshes and embedded PBR maps."""
import io
import json
import math
import random
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

OUTPUT = Path(__file__).resolve().parent


class Mesh:
    def __init__(self):
        self.materials = []
        self.maps = []
        self.vertices = []
        self.tile_sizes = []

    def material(self, name, color, roughness, metal=0, *, texture=None, emissive=None,
                 tile_size=1.44, double_sided=False):
        material = dict(name=name, pbrMetallicRoughness=dict(
            baseColorFactor=[*color, 1], roughnessFactor=roughness, metallicFactor=metal
        ))
        if emissive:
            material["emissiveFactor"] = list(emissive)
        if double_sided:
            material["doubleSided"] = True
        self.materials.append(material)
        self.maps.append(texture or {})
        self.vertices.append([])
        self.tile_sizes.append(tile_size)
        return len(self.materials) - 1

    def face(self, points, material, uv=None, normals=None):
        a, b, c = points[:3]
        u, v = ([point[i] - a[i] for i in range(3)] for point in (b, c))
        n = [u[(i + 1) % 3] * v[(i + 2) % 3] - u[(i + 2) % 3] * v[(i + 1) % 3]
             for i in range(3)]
        length = math.sqrt(sum(value * value for value in n))
        if length <= 1e-9:
            return
        normal = [value / length for value in n]
        if uv is None:
            axis = max(range(3), key=lambda i: abs(n[i]))
            tex_axes = ((2, 1), (0, 2), (0, 1))[axis]
            tile_size = self.tile_sizes[material]
            uv = [(point[tex_axes[0]] / tile_size, -point[tex_axes[1]] / tile_size)
                  for point in points]
        for i in range(1, len(points) - 1):
            self.vertices[material].extend(
                (*points[j], *(normal if normals is None else normals[j]), *uv[j])
                for j in (0, i, i + 1)
            )

    def horizontal(self, x0, x1, z0, z1, y, material):
        self.face([(x0, y, z0), (x0, y, z1), (x1, y, z1), (x1, y, z0)], material)

    def box(self, at, size, material, bevel=0.012):
        x, y, z = at
        w, h, d = (value / 2 for value in size)
        radius = min(bevel, w * 0.6, h * 0.6, d * 0.6)
        for axis in range(3):
            other = [(axis + 1) % 3, (axis + 2) % 3]
            normal = [0.0, 0.0, 0.0]
            normal[axis] = 1.0
            for sign in [-1, 1]:
                points = []
                for a, b in [(-1, -1), (1, -1), (1, 1), (-1, 1)]:
                    bounds = [w, h, d]
                    local = [0.0, 0.0, 0.0]
                    local[axis] = sign * bounds[axis]
                    local[other[0]] = a * (bounds[other[0]] - radius)
                    local[other[1]] = b * (bounds[other[1]] - radius)
                    points.append(tuple(coord + origin for coord, origin in zip(local, at)))
                self.face(points if sign > 0 else points[::-1], material)
        if radius <= 0:
            return
        # Twelve thin faceted edge strips. Tiny corner triangles close the bevel.
        for axis in range(3):
            a, b = (axis + 1) % 3, (axis + 2) % 3
            bounds = [w, h, d]
            for sa in [-1, 1]:
                for sb in [-1, 1]:
                    points = []
                    for end, a_outer in [(-1, True), (1, True), (1, False), (-1, False)]:
                        local = [0.0, 0.0, 0.0]
                        local[axis] = end * (bounds[axis] - radius)
                        local[a] = sa * (bounds[a] if a_outer else bounds[a] - radius)
                        local[b] = sb * (bounds[b] - radius if a_outer else bounds[b])
                        points.append(tuple(coord + origin for coord, origin in zip(local, at)))
                    self.face(points if sa * sb < 0 else points[::-1], material)
        for sx in [-1, 1]:
            for sy in [-1, 1]:
                for sz in [-1, 1]:
                    signs = [sx, sy, sz]
                    points = [tuple(origin + sign * (bound if j == i else bound - radius)
                                    for j, (origin, sign, bound) in enumerate(zip(at, signs, (w, h, d))))
                              for i in range(3)]
                    self.face(points if sx * sy * sz > 0 else points[::-1], material)

    def cylinder(self, at, rings, material, sides=14):
        x, y, z = at
        def point(ring, index):
            h, radius = ring
            angle = math.tau * index / sides
            return x + math.cos(angle) * radius, y + h, z + math.sin(angle) * radius
        for low, high in zip(rings, rings[1:]):
            for index in range(sides):
                self.face([point(low, index + 1), point(low, index),
                           point(high, index), point(high, index + 1)], material)
        for index in range(sides):
            self.face([(x, y + rings[0][0], z), point(rings[0], index),
                       point(rings[0], index + 1)], material)
            self.face([(x, y + rings[-1][0], z), point(rings[-1], index + 1),
                       point(rings[-1], index)], material)

    def save(self, name, output=OUTPUT):
        doc = dict(asset=dict(version="2.0", generator="Студия игр · оригинальные модели 0.22"),
                   scenes=[dict(nodes=[0])], scene=0, nodes=[dict(mesh=0, name=name)],
                   materials=self.materials, accessors=[], bufferViews=[], images=[], textures=[])
        data = bytearray()

        def view(blob, stride=None):
            data.extend(bytes((-len(data)) % 4))
            item = dict(buffer=0, byteOffset=len(data), byteLength=len(blob))
            if stride:
                item.update(byteStride=stride, target=34962)
            doc["bufferViews"].append(item)
            data.extend(blob)
            return len(doc["bufferViews"]) - 1

        images = {}
        for material, maps in zip(self.materials, self.maps):
            for slot, picture in maps.items():
                key = id(picture)
                if key not in images:
                    encoded = io.BytesIO()
                    if picture.info.get("scan_jpeg"):
                        picture.save(encoded, format="JPEG", quality=94, subsampling=0)
                        mime = "image/jpeg"
                    else:
                        picture.save(encoded, format="PNG")
                        mime = "image/png"
                    doc["images"].append(dict(bufferView=view(encoded.getvalue()), mimeType=mime))
                    doc["textures"].append(dict(source=len(doc["images"]) - 1))
                    images[key] = len(doc["textures"]) - 1
                parent = material["pbrMetallicRoughness"] if slot in (
                    "baseColorTexture", "metallicRoughnessTexture"
                ) else material
                parent[slot] = dict(index=images[key])

        primitives = []
        for index, vertices in enumerate(self.vertices):
            if not vertices:
                continue
            buffer = view(b"".join(struct.pack("<8f", *vertex) for vertex in vertices), 32)
            attrs = {}
            for key, offset, shape in [("POSITION", 0, "VEC3"), ("NORMAL", 12, "VEC3"),
                                       ("TEXCOORD_0", 24, "VEC2")]:
                entry = dict(bufferView=buffer, byteOffset=offset, componentType=5126,
                             count=len(vertices), type=shape)
                if key == "POSITION":
                    entry.update(min=[min(v[i] for v in vertices) for i in range(3)],
                                 max=[max(v[i] for v in vertices) for i in range(3)])
                attrs[key] = len(doc["accessors"])
                doc["accessors"].append(entry)
            primitives.append(dict(attributes=attrs, material=index, mode=4))
        doc["meshes"] = [dict(primitives=primitives)]
        data.extend(bytes((-len(data)) % 4))
        doc["buffers"] = [dict(byteLength=len(data))]
        header = json.dumps(doc, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        header += b" " * ((-len(header)) % 4)
        (output / f"{name}.glb").write_bytes(
            struct.pack("<4sII", b"glTF", 2, 28 + len(header) + len(data))
            + struct.pack("<I4s", len(header), b"JSON") + header
            + struct.pack("<I4s", len(data), b"BIN\0") + data
        )


def scale_image(size, background):
    return Image.new("RGB", (size, size), background)


def mapped(size, height, pigment, roughness, seed, roughness_variation=0.15):
    # Height and albedo use the same geometry. Spread high-frequency grain over the lower-frequency wear.
    height = height.filter(ImageFilter.GaussianBlur(0.85))
    hp = height.load()
    pigment = pigment.load()
    grain = random.Random(seed)
    albedo, normal, surface = [scale_image(size, (0, 0, 0)) for _ in range(3)]
    color, bumps, mr = (image.load() for image in (albedo, normal, surface))
    for y in range(size):
        for x in range(size):
            noise = grain.gauss(0, 2.8)
            coarse = 3.8 * math.cos(x * math.tau / size * 3) * math.sin(y * math.tau / size * 2)
            tone = [channel + noise + coarse for channel in pigment[x, y]]
            color[x, y] = tuple(int(min(255, max(0, value))) for value in tone)
            dx = (hp[(x - 1) % size, y] - hp[(x + 1) % size, y]) * 0.012 + noise * 0.014
            dy = (hp[x, (y - 1) % size] - hp[x, (y + 1) % size]) * 0.012 + noise * 0.009
            length = math.sqrt(1 + dx * dx + dy * dy)
            bumps[x, y] = tuple(int(127.5 * (1 + value / length)) for value in [dx, dy, 1.0])
            dry = min(1, roughness + roughness_variation * (1 - hp[x, y] / 255))
            mr[x, y] = (255, int(255 * dry), 255)
    return dict(baseColorTexture=albedo, normalTexture=normal, metallicRoughnessTexture=surface)


def stone(wet):
    size = 384
    rng = random.Random(432)
    hp = Image.new("L", (size, size), 70)
    pigment = scale_image(size, (45, 52, 57) if wet else (71, 82, 84))
    h, p = ImageDraw.Draw(hp), ImageDraw.Draw(pigment)
    for row in range(8):
        for col in range(-1, 6):
            x = col * 64 + 32 * (row % 2)
            y = row * 48
            shape = (x + 3, y + 2, x + 61, y + 45)
            base = rng.randint(91, 130) if wet else rng.randint(101, 148)
            h.rounded_rectangle(shape, radius=rng.randint(4, 9), fill=rng.randint(155, 227))
            p.rounded_rectangle(shape, radius=6, fill=(base - 6, base, base + (8 if wet else 3)))
            for _ in range(4):
                xx = x + rng.randint(6, 58)
                yy = y + rng.randint(5, 42)
                size_px = rng.randint(1, 4)
                p.ellipse((xx - size_px, yy - 1, xx + size_px, yy + 2),
                          fill=(base - 13, base - 12, base - 4))
    return mapped(size, hp, pigment, 0.22 if wet else 0.64, 56)


def wood():
    size = 288
    hp = Image.new("L", (size, size), 155)
    pigment = scale_image(size, (130, 85, 53))
    h, p = ImageDraw.Draw(hp), ImageDraw.Draw(pigment)
    rng = random.Random(230)
    for col in range(6):
        x = col * 48
        color = rng.randint(115, 151)
        p.rectangle((x, 0, x + 47, size), fill=(color, round(color * .64), round(color * .42)))
        h.line((x, 0, x, size), fill=45, width=2)
        p.line((x, 0, x, size), fill=(76, 48, 34), width=2)
        for fiber in range(16):
            offset = rng.randint(4, 43)
            wave = rng.uniform(0.4, 1.8)
            points = [(x + offset + math.sin(y / 30 + fiber) * wave, y) for y in range(0, size, 3)]
            p.line(points, fill=(color - 12, round(color * .57), round(color * .40)), width=1)
    return mapped(size, hp, pigment, 0.46, 15)


def scanned_maps(name, size=None):
    def load(channel, mode):
        with Image.open(OUTPUT / "scans" / f"{name}-{channel}.jpg") as image:
            parsed = image.convert(mode)
            if size and parsed.width > size:
                parsed.thumbnail((size, size), Image.Resampling.LANCZOS)
            if size and mode == "RGB":
                parsed.info["scan_jpeg"] = True
            return parsed

    roughness = load("rough", "L")
    full = Image.new("L", roughness.size, 255)
    return dict(
        baseColorTexture=load("diff", "RGB"),
        normalTexture=load("nor_gl", "RGB"),
        metallicRoughnessTexture=Image.merge("RGB", (full, roughness, full)),
    )


def ground():
    m = Mesh()
    wet = m.material("Мокрая брусчатка", (.85, .89, .91), .52,
                     texture=scanned_maps("cobblestone_pavement"), tile_size=2.5)
    curb = m.material("Матовый гранит", (.77, .78, .74), 1, 0, texture=stone(False))
    rock = m.material("Дно лужи", (.12, .16, .18), .94)
    dark = m.material("Чугун дождеприёмника", (.12, .15, .15), .33, .63)
    fill = m.material("Тень дождеприёмника", (.037, .040, .040), .98)
    rim = m.material("Намокшая кромка", (.17, .20, .21), .39, .08)
    m.horizontal(-24, -3.0, -6.18, 10.5, .012, wet)
    m.horizontal(3.0, 24, -6.18, 10.5, .012, wet)
    m.horizontal(-3, 3, -3.10, 10.5, .012, wet)
    m.horizontal(-3, 3, -6.18, -5.50, .012, wet)
    cx, cz = 0, -4.30
    outline = []
    rng = random.Random(8137)
    for side in range(4):
        for i in range(12):
            t = i / 12
            point = [(-3, -5.50 + 2.40 * t), (-3 + 6 * t, -3.10),
                     (3, -3.10 - 2.40 * t), (3 - 6 * t, -5.50)][side]
            dx, dz = point[0] - cx, point[1] - cz
            angle = math.atan2(dz, dx)
            ripple = 1 + .09 * math.sin(angle * 5 + .2) + rng.uniform(-.065, .065)
            outline.append((point, (cx + math.cos(angle) * 2.18 * ripple,
                                    cz + math.sin(angle) * .94 * ripple)))
    for (a, b), (c, d) in zip(outline, outline[1:] + outline[:1]):
        m.face([(a[0], .012, a[1]), (c[0], .012, c[1]),
                (d[0], .012, d[1]), (b[0], .012, b[1])], wet)
        m.face([(b[0], .012, b[1]), (d[0], .012, d[1]),
                (cx + .93 * d[0], -.019, cz + .93 * (d[1] - cz)),
                (cx + .93 * b[0], -.019, cz + .93 * (b[1] - cz))], rim)
        m.face([(cx, -.019, cz), (cx + .93 * b[0], -.019, cz + .93 * (b[1] - cz)),
                (cx + .93 * d[0], -.019, cz + .93 * (d[1] - cz))], rock)
    m.horizontal(-24, 24, -24, -6.18, .071, curb)
    for x in range(-24, 24):
        if x in [-4, 3]:
            continue
        m.box((x + .5, .028, -6.18), (.984, .088, .30), curb, .022)
    for x in [-3.5, 3.5]:
        m.box((x, .02, -6.18), (.96, .085, .37), dark, .018)
        m.box((x, .063, -6.18), (.83, .01, .27), fill)
        for j in range(-5, 6):
            m.box((x + j * .072, .079, -6.18), (.036, .025, .26), dark, .005)
    m.save("street")


def sign_texture():
    picture = Image.new("RGB", (1024, 256), (30, 40, 45))
    painter = ImageDraw.Draw(picture)
    font = "C:/Windows/Fonts/bahnschrift.ttf"
    painter.rounded_rectangle((16, 15, 1008, 240), radius=20, outline=(173, 149, 99), width=4)
    painter.line((103, 166, 921, 166), fill=(128, 107, 69), width=2)
    painter.text((512, 102), "С Е В Е Р", anchor="mm", font=ImageFont.truetype(font, 95),
                 fill=(244, 220, 171))
    painter.text((512, 199), "М А С Т Е Р С К А Я   ·   1 9", anchor="mm",
                 font=ImageFont.truetype(font, 34), fill=(197, 197, 171))
    return picture


def facade():
    m = Mesh()
    iron = m.material("Антрацитовая сталь", (.085, .118, .13), .40, .67)
    zinc = m.material("Матовый цинк", (.28, .35, .39), .44, .56)
    stonework = m.material("Тёсаный гранит", (.46, .49, .51), .91,
                           texture=scanned_maps("plaster_stone_wall_02", 512), tile_size=1.45)
    brick = m.material("Обожжённый кирпич", (.68, .66, .63), .91,
                       texture=scanned_maps("brick_wall_09", 1024), tile_size=2.0)
    lower_brick = m.material("Кирпич у мостовой", (.46, .51, .52), .67,
                             texture=m.maps[brick], tile_size=2.0)
    brass = m.material("Потемневшая латунь", (.50, .36, .18), .39, .74)
    plaque = m.material("Печатная вывеска", (1, 1, 1), .48,
                        texture=dict(baseColorTexture=sign_texture()), emissive=(.07, .045, .02))
    glass = m.material("Янтарное стекло", (.86, .47, .11), .28, 0, emissive=(.45, .19, .032))
    roof = m.material("Патинированная медь", (.11, .27, .24), .49, .52)

    def cladding(x0, x1, y0, y1, material):
        z = .008
        m.face([(x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z)], material)

    for left, right in [(-4.09, -.70), (.70, 4.09)]:
        cladding(left, right, .126, .46, lower_brick)
        cladding(left, right, .46, 4.269, brick)
    cladding(-.70, .70, 2.66, 4.269, brick)
    for y, depth in [(.46, .15), (3.87, .14), (4.01, .20)]:
        for left, right in ([(-4.12, -.73), (.73, 4.12)] if y < 1 else [(-4.13, 4.13)]):
            m.box(((left + right) * .5, y, .039), (right - left, .09, depth), stonework, .009)
    m.box((0, 3.36, .17), (3.07, .76, .15), iron, .034)
    m.box((0, 3.36, .25), (2.94, .66, .05), brass, .018)
    m.face([(-1.44, 3.06, .281), (1.44, 3.06, .281),
            (1.44, 3.66, .281), (-1.44, 3.66, .281)],
           plaque, [(0, 1), (1, 1), (1, 0), (0, 0)])
    m.box((0, 2.86, .64), (4.25, .10, 1.28), iron, .024)
    m.box((0, 2.94, .64), (4.21, .12, 1.29), roof, .024)
    for x in [-1.83, -1.2, -.58, 0, .58, 1.2, 1.83]:
        m.box((x, 3.012, .66), (.027, .038, 1.23), zinc, .004)
    for x in [-1.75, 1.75]:
        m.box((x, 2.71, .61), (.20, .11, .25), iron, .015)
        m.box((x, 2.625, .61), (.16, .09, .20), glass, .013)
    for x in [-3.85, -1.35, 1.35, 3.85]:
        m.box((x, 1.77, .045), (.28, 3.16, .19), stonework, .023)
        m.box((x, .39, .055), (.39, .31, .245), stonework, .021)
        m.box((x, 3.21, .070), (.43, .18, .23), stonework, .019)
    for x in [-.76, .76]:
        m.box((x, 1.51, .035), (.09, 2.56, .18), stonework)
    m.box((0, 2.72, .05), (1.72, .13, .21), stonework)
    m.box((0, .117, .17), (1.72, .034, .37), zinc, .007)
    for x in [-3.98, 3.98]:
        m.cylinder((x, .14, .13), [(0, .076), (.13, .073), (.16, .046),
                                  (3.69, .041), (3.77, .076)], zinc)
        for y in [.42, 1.47, 2.5, 3.28]:
            m.box((x, y, .115), (.15, .07, .12), iron)
    m.box((0, 4.09, .08), (8.31, .13, .37), stonework, .035)
    m.box((0, 4.23, .08), (8.37, .13, .40), roof, .031)
    m.save("facade")


def bench():
    m = Mesh()
    timber = m.material("Промасленный дуб", (1, 1, 1), 1, texture=wood())
    iron = m.material("Воронёная сталь", (.12, .16, .17), .34, .68)
    zinc = m.material("Рабочая сталь", (.47, .50, .52), .27, .71)
    ruby = m.material("Красная эмаль", (.35, .075, .045), .43)
    dark = m.material("Масляные пятна", (.13, .087, .051), .57)
    rubber = m.material("Пробка", (.065, .071, .065), .87)
    m.box((0, .88, 0), (2.22, .13, .79), timber, .042)
    for x in [-.93, .93]:
        for z in [-.26, .26]:
            m.box((x, .44, z), (.085, .87, .085), iron)
            m.box((x, .05, z), (.14, .06, .12), rubber)
    m.box((0, .25, .0), (1.90, .055, .66), timber)
    m.box((0, .74, .36), (1.76, .18, .073), iron)
    for x in [-.44, .44]:
        m.box((x, .74, .404), (.75, .135, .08), timber, .016)
        m.box((x, .74, .462), (.23, .031, .041), zinc, .006)
    # A vice, grips and two original tool silhouettes.
    m.box((-.73, 1.00, .11), (.31, .12, .20), ruby)
    m.box((-.73, 1.125, .075), (.29, .18, .11), iron, .022)
    for z in [-.053, .147]:
        m.box((-.73, 1.172, z), (.25, .082, .07), zinc, .009)
    m.box((-.73, 1.065, .28), (.29, .027, .026), zinc, .010)
    m.box((-.90, 1.066, .282), (.051, .053, .048), iron)
    m.box((.31, .955, -.11), (.28, .074, .25), ruby, .010)
    m.box((.31, .999, -.11), (.29, .026, .26), iron, .009)
    m.box((.31, 1.029, -.11), (.09, .037, .018), zinc, .007)
    m.box((.22, .959, .175), (.33, .038, .031), zinc, .010)
    m.box((.061, .961, .175), (.11, .057, .08), zinc, .012)
    m.box((.82, .959, .06), (.24, .037, .040), dark)
    m.box((.675, .958, .06), (.18, .027, .028), zinc)
    m.save("workbench")


def room():
    m = Mesh()
    timber = m.material("Сосновые половицы", (1, .92, .82), .82,
                        texture=scanned_maps("wood_floor_worn"), tile_size=2.0)
    iron = m.material("Оцинкованная рама", (.21, .25, .25), .46, .71)
    dark = m.material("Пропитанная резина", (.095, .11, .115), .93)
    cream = m.material("Краска шкатулок", (.52, .42, .24), .47)
    enamel = m.material("Зелёная эмаль", (.16, .26, .20), .46)
    glass = m.material("Тёплые светильники", (.94, .58, .21), .29, emissive=(.53, .26, .068))
    plaster = m.material("Известковая штукатурка", (.73, .69, .60), 1,
                         texture=scanned_maps("plaster_stone_wall_02", 512), tile_size=2.1)
    dado = m.material("Затёртая краска", (.34, .48, .40), .88,
                      texture=m.maps[plaster], tile_size=2.1)
    for low, high, material in [(.023, 1.13, dado), (1.13, 4.129, plaster)]:
        for x, z0, z1 in [(-3.895, 3.79, -3.793), (3.895, -3.793, 3.79)]:
            m.face([(x, low, z0), (x, low, z1), (x, high, z1), (x, high, z0)], material)
        m.face([(-3.898, low, -3.791), (3.898, low, -3.791),
                (3.898, high, -3.791), (-3.898, high, -3.791)], material)
    for x in [-3.883, 3.883]:
        m.box((x, 1.13, -.001), (.051, .068, 7.68), timber, .009)
    m.box((.025, 1.13, -3.769), (7.79, .068, .051), timber, .008)
    m.horizontal(-3.87, 3.87, -3.84, 3.89, .021, timber)
    for x in [-3.74, 3.74]:
        m.box((x, .08, -.02), (.17, .13, 7.64), timber, .023)
    m.box((0, .05, 3.08), (1.32, .016, .91), dark, .015)
    for y in [.39, .88, 1.39, 1.91]:
        m.box((-2.44, y, -3.50), (2.53, .066, .64), timber)
    for x in [-3.66, -1.24]:
        for z in [-3.77, -3.24]:
            m.box((x, 1.09, z), (.075, 2.08, .075), iron)
    rng = random.Random(72)
    for y in [.45, .94, 1.45]:
        for col in range(8):
            x = -3.56 + col * .32
            if rng.random() < .24:
                continue
            ht = rng.uniform(.15, .33)
            m.box((x, y + ht / 2, -3.41), (.25, ht, .42), rng.choice([enamel, cream, dark]), .025)
            m.box((x, y + .08, -3.184), (.08, .029, .031), iron, .006)
    for x in [-2.1, 2.1]:
        m.box((x, 3.68, .62), (.12, .14, .12), iron)
        m.box((x, 3.51, .62), (.83, .13, .32), iron, .028)
        m.box((x, 3.436, .62), (.76, .038, .255), glass, .019)
    # Conduit, sockets and riveted shelf brackets catch highlights at player height.
    for y in [.39, .88, 1.39, 1.91]:
        for x in [-3.60, -1.29]:
            m.box((x, y - .112, -3.64), (.049, .195, .052), iron, .005)
    m.box((2.26, 2.51, -3.753), (3.16, .024, .035), iron, .006)
    m.box((.704, 2.10, -3.753), (.024, .84, .035), iron, .006)
    m.box((.704, 1.69, -3.748), (.127, .185, .077), dark, .013)
    for x in [.67, .738]:
        m.cylinder((x, 1.776, -3.716), [(0, .021), (.015, .023)], cream, 10)
    m.save("room")


def street_details():
    m = Mesh()
    steel = m.material("Стальная опора", (.08, .13, .16), .46, .57)
    brass = m.material("Подсвеченное стекло", (.86, .62, .34), .4, emissive=(.45, .22, .071))
    slate = m.material("Каменная тумба", (.29, .36, .37), .70)
    greens = [m.material("Мокрые листья", color, .51) for color in
              [(.13, .28, .20), (.15, .32, .18), (.32, .38, .19), (.22, .36, .21)]]
    for x, z in [(4.8, -5.78), (-4.9, -5.83), (-6.7, 3.70), (13.65, -5.80),
                 (-14.6, -5.95)]:
        m.cylinder((x, 0, z), [(0, .21), (.08, .21), (.18, .13), (.44, .115),
                                (.52, .069), (3.32, .047)],  steel)
        m.box((x, 3.365, z + .20), (.074, .067, .46), steel)
        m.cylinder((x, 3.03, z + .40), [(0, .087), (.13, .13), (.225, .123)], brass, 8)
        m.cylinder((x, 3.255, z + .4), [(0, .155), (.068, .146), (.12, .065)], steel, 8)
    rng = random.Random(299)
    for x, z in [(-4.66, -6.76), (4.48, -6.40), (-9.48, -6.53), (9.87, -6.65), (5.74, 2.79)]:
        m.cylinder((x, .07, z), [(0, .37), (.08, .40), (.34, .46), (.41, .46)], slate)
        for _ in range(80):
            angle = rng.random() * math.tau
            dx, dz = math.cos(angle), math.sin(angle)
            length = rng.uniform(.14, .56)
            stem = rng.uniform(.09, .21)
            y = .47
            lead = (x + dx * length, y + rng.uniform(.19, .67), z + dz * length)
            mid = (x + dx * (.22 + stem), y + rng.uniform(.25, .58), z + dz * (.22 + stem))
            rw = rng.uniform(.032, .070)
            points = [(x + dx * stem, y, z + dz * stem), (mid[0] + dz * rw, mid[1], mid[2] - dx * rw),
                      lead, (mid[0] - dz * rw, mid[1] - .025, mid[2] + dx * rw)]
            index = rng.choice(greens)
            m.face(points, index)
            m.face(points[::-1], index)
    m.save("fixtures")


def lamp():
    m = Mesh()
    steel = m.material("Графитовая арматура", (.098, .12, .12), .38, .62)
    glass = m.material("Янтарная колба", (.95, .56, .24), .21, emissive=(.61, .25, .073))
    m.box((0, .09, -.23), (.21, .25, .09), steel, .019)
    m.box((0, .20, -.06), (.068, .048, .43), steel, .011)
    m.cylinder((0, 0, .14), [(.16, .20), (.21, .186), (.28, .093), (.32, .043)], steel, 14)
    m.cylinder((0, 0, .14), [(.018, .074), (.064, .12), (.159, .146)], glass, 14)
    m.save("lamp")


if __name__ == "__main__":
    ground()
    facade()
    bench()
    room()
    street_details()
    lamp()
