"""Build original reusable meshes for the world palette."""
import json
import math
import random
import runpy
import struct
from pathlib import Path

HERE = Path(__file__).resolve().parent
RAIN = runpy.run_path(str(HERE.parent / "rain" / "build.py"))
Mesh = RAIN["Mesh"]
TAU = math.tau


def ground_cover():
    for style in ("natural", "lowpoly"):
        for flowers in (False, True):
            m = Mesh()
            rng = random.Random(356)
            greens = [m.material("Листва", rgb, .87, double_sided=True) for rgb in
                      [(.11, .21, .056), (.17, .32, .073), (.22, .37, .106), (.26, .42, .12)]]
            pollen = m.material("Пыльца", (.73, .41, .043), .69)
            petals = [m.material("Лепестки", rgb, .82, double_sided=True) for rgb in
                      [(.83, .78, .68), (.56, .23, .62), (.87, .54, .66)]]
            for _ in range(155 if style == "natural" else 43):
                angle = rng.uniform(0, TAU)
                radius = math.sqrt(rng.random()) * .83
                x, z = math.sin(angle) * radius, math.cos(angle) * radius
                a = rng.uniform(0, TAU)
                dx, dz = math.cos(a), math.sin(a)
                length = rng.uniform(.12, .41)
                width = rng.uniform(.007, .024) if style == "natural" else rng.uniform(.025, .045)
                points = [(x - dz * width, 0, z + dx * width), (x + dz * width, 0, z - dx * width),
                          (x + dx * length * .19, length, z + dz * length * .19)]
                if style == "natural":
                    points.insert(2, (x + dx * length * .08 + dz * width, length * .59,
                                      z + dz * length * .08 - dx * width))
                m.face(points, rng.choice(greens))
            if flowers:
                for i in range(39):
                    a = rng.uniform(0, TAU)
                    radius = math.sqrt(rng.random()) * .73
                    x, z = math.sin(a) * radius, math.cos(a) * radius
                    top = rng.uniform(.29, .62)
                    size = rng.uniform(.022, .049)
                    m.cylinder((x, 0, z), [(0, .004), (top, .003)], greens[0], 4)
                    m.cylinder((x, top, z), [(0, size * .28), (.006, size * .34)], pollen, 7)
                    n = 7 if style == "natural" else 5
                    for j in range(n):
                        angle = TAU * (j / n + (i % 8) * .09)
                        dx, dz = math.cos(angle), math.sin(angle)
                        m.face([(x, top + .002, z), (x + (dx - dz * .48) * size * .67,
                                top + .015, z + (dz + dx * .48) * size * .67),
                                (x + dx * size, top - .005, z + dz * size),
                                (x + (dx + dz * .48) * size * .67, top + .014,
                                 z + (dz - dx * .48) * size * .67)], petals[i % 3])
            m.save(f"{'flowers' if flowers else 'grass'}-{style}", HERE)


def street():
    for style in ("natural", "lowpoly"):
        m = Mesh()
        maps = RAIN["scanned_maps"]
        brick = m.material("Кирпич", (.82, .72, .62), .87,
                           texture=maps("brick_wall_09", 512) if style == "natural" else None, tile_size=2)
        stone = m.material("Тёмная черепица", (.22, .25, .29), .88,
                           texture=maps("plaster_stone_wall_02", 512) if style == "natural" else None, tile_size=1.3)
        trim = m.material("Столярные детали", (.48, .31, .17), .77)
        glass = m.material("Стекло", (.31, .39, .41), .20, .38)
        m.box((0, 1.22, 0), (3.8, 2.44, 2.96), brick, .035)
        m.box((0, .13, 0), (3.92, .26, 3.04), stone, .016)
        for side in [-1, 1]:
            eave = side * 1.66
            roof = [(-2.01, 2.42, eave), (2.01, 2.42, eave), (2.01, 3.46, 0), (-2.01, 3.46, 0)]
            m.face(roof if side > 0 else roof[::-1], stone)
            x = side * 1.918
            gable = [(x, 2.42, -1.48), (x, 2.42, 1.48), (x, 3.46, 0)]
            m.face(gable if side < 0 else gable[::-1], brick)
            m.box((0, 2.426, eave), (4.08, .066, .121), trim, .011)
        m.box((.99, 3.10, -.54), (.33, 1.62, .36), brick, .024)
        m.box((.99, 3.93, -.54), (.43, .14, .46), stone, .018)
        m.box((0, .96, 1.50), (.72, 1.90, .10), trim, .018)
        m.box((.23, 1.02, 1.57), (.036, .12, .05), stone, .007)
        for x in [-1.19, 1.19]:
            m.box((x, 1.37, 1.52), (.74, .91, .12), trim, .016)
            m.box((x, 1.37, 1.592), (.59, .76, .034), glass, .007)
            m.box((x, 1.37, 1.626), (.028, .76, .023), trim, .004)
            m.box((x, .91, 1.60), (.84, .075, .23), stone, .009)
        m.save(f"house-{style}", HERE)
        m = Mesh()
        road = m.material("Дорожное покрытие", (.47, .49, .52), .78,
                          texture=maps("cobblestone_pavement", 512) if style == "natural" else None,
                          tile_size=2.5)
        edge = m.material("Каменные края", (.34, .37, .40), .87)
        m.horizontal(-1.6, 1.6, -2, 2, .018, road)
        for x in [-1.69, 1.69]:
            for index in range(8):
                m.box((x, .035, -1.75 + .50 * index), (.16, .08, .492), edge, .014)
        m.save(f"road-{style}", HERE)


def planet():
    m = Mesh()
    bands = [m.material("Минеральные облака", (.24 + .075 * a, .19 + .054 * a, .28 + .067 * a), .81)
             for a in range(7)]
    ring = m.material("Пылевое кольцо", (.59, .45, .36), .87, double_sided=True)
    n, rows = 64, 40

    def p(v, s):
        a = TAU * s / n
        b = math.pi * v / rows
        return math.cos(a) * math.sin(b), 1 + math.cos(b), math.sin(a) * math.sin(b)

    for v in range(rows):
        for s in range(n):
            pts = [p(v, s), p(v, s + 1), p(v + 1, s + 1), p(v + 1, s)]
            if v == 0:
                pts.pop(0)
            elif v == rows - 1:
                pts.pop(2)
            normals = [(x, y - 1, z) for x, y, z in pts]
            stripe = round(3 + 1.7 * math.sin(v * .66 + math.sin(s * TAU / n * 2) * .81)
                           + math.cos(v * 1.21))
            m.face(pts, bands[stripe], normals=normals)
    for a, b in [(1.24, 1.36), (1.38, 1.47), (1.52, 1.67)]:
        for j in range(n):
            pts = []
            for i, r in [(j, a), (j, b), (j + 1, b), (j + 1, a)]:
                x = r * math.cos(i * TAU / n)
                z = r * math.sin(i * TAU / n)
                pts.append((x, 1 + z * .26, z * .966))
            m.face(pts, ring)
    m.save("planet", HERE)


def fox():
    m = Mesh()
    ginger = (.65, .23, .059)
    pale = (.87, .74, .54)
    soot = (.077, .051, .041)
    material = lambda name, shade: m.material(name, shade, .84)
    core = material("Корпус", ginger)
    breast = material("Грудка", pale)
    m.box((0, .47, 0), (.31, .32, .62), core, .082)
    m.box((0, .49, .27), (.23, .29, .19), breast, .062)
    head = material("Голова", ginger)
    muzzle = material("Морда", pale)
    detail = material("Глаза и нос", soot)
    m.box((0, .042, .063), (.26, .24, .29), head, .064)
    m.box((0, -.018, .222), (.135, .097, .136), muzzle, .032)
    m.box((0, .009, .296), (.077, .053, .048), detail, .016)
    for side in [-1, 1]:
        x = side * .09
        m.box((side * .11, .073, .158), (.041, .031, .046), detail, .009)
        vertices = [(x - .063, .129, .041), (x + .057, .129, .041),
                    (x + side * .034, .343, .008), (x, .139, -.028)]
        for triangle in [(0, 1, 2), (1, 3, 2), (3, 0, 2), (0, 3, 1)]:
            m.face([vertices[i] for i in triangle], head)
    tail = material("Хвост", ginger)
    tip = material("Кончик хвоста", pale)
    m.box((0, .026, -.19), (.24, .25, .40), tail, .066)
    m.box((0, .078, -.39), (.17, .18, .19), tip, .062)
    parts = [("FoxHead", (0, .57, .29), [head, muzzle, detail]),
             ("FoxTail", (0, .45, -.30), [tail, tip])]
    for i, (x, z) in enumerate([(-.121, .224), (.121, .224), (-.121, -.216), (.121, -.216)]):
        upper = material(f"Лапа {i}", ginger)
        paw = material(f"Подушечки {i}", soot)
        m.box((0, -.105, 0), (.097, .243, .101), upper, .022)
        m.box((0, -.26, .020), (.076, .135, .098), paw, .014)
        parts.append((f"FoxLeg{i}", (x, .329, z), [upper, paw]))
    m.save("fox", HERE)

    path = HERE / "fox.glb"
    raw = path.read_bytes()
    n, = struct.unpack_from("<I", raw, 12)
    doc = json.loads(raw[20:20 + n])
    buf = bytearray(raw[28 + n:])
    primitives = doc["meshes"][0]["primitives"]
    doc["meshes"][0]["primitives"] = [p for p in primitives if p["material"] in [core, breast]]
    doc["nodes"][0]["children"] = list(range(1, len(parts) + 1))
    for i, (name, translation, materials) in enumerate(parts, 1):
        doc["meshes"].append(dict(primitives=[p for p in primitives if p["material"] in materials]))
        doc["nodes"].append(dict(name=name, mesh=i, translation=translation))

    def accessor(values, shape):
        flat = [x for row in values for x in row]
        start = len(buf)
        buf.extend(struct.pack(f"<{len(flat)}f", *flat))
        view = len(doc["bufferViews"])
        doc["bufferViews"].append(dict(buffer=0, byteOffset=start, byteLength=len(buf) - start))
        info = dict(bufferView=view, count=len(values), type=shape, componentType=5126)
        if shape == "SCALAR":
            info.update(min=[min(flat)], max=[max(flat)])
        doc["accessors"].append(info)
        return len(doc["accessors"]) - 1

    doc["animations"] = []
    for name, duration, extent in [("Idle", 2.1, 0), ("Walk", .82, .49), ("Run", .57, .74)]:
        times = accessor([[i * duration / 16] for i in range(17)], "SCALAR")
        clip = dict(name=name, samplers=[], channels=[])
        for joint in ([1, 2] if extent == 0 else range(1, 7)):
            quats = []
            for i in range(17):
                phase = TAU * i / 16
                if joint == 2:
                    roll = .16 * math.cos(phase)
                    q = [0, math.sin(roll / 2), 0, math.cos(roll / 2)]
                else:
                    angle = .068 * math.sin(phase) if joint == 1 else extent * math.sin(
                        phase + ([0, math.pi, math.pi * .85, -.15 * math.pi][joint - 3]))
                    q = [math.sin(angle / 2), 0, 0, math.cos(angle / 2)]
                quats.append(q)
            clip["channels"].append(dict(sampler=len(clip["samplers"]),
                                         target=dict(node=joint, path="rotation")))
            clip["samplers"].append(dict(input=times, output=accessor(quats, "VEC4"), interpolation="LINEAR"))
        doc["animations"].append(clip)
    doc["buffers"][0]["byteLength"] = len(buf)
    header = json.dumps(doc, ensure_ascii=False, separators=(",", ":")).encode()
    header += b" " * ((-len(header)) % 4)
    path.write_bytes(struct.pack("<4sII", b"glTF", 2, 28 + len(header) + len(buf))
                     + struct.pack("<I4s", len(header), b"JSON") + header
                     + struct.pack("<I4s", len(buf), b"BIN\0") + buf)


def lowpoly_props():
    m = Mesh()
    bark = m.material("Ствол", (.33, .21, .14), .9)
    leaves = [m.material("Хвоя", color, .94) for color in
              [(.11, .28, .20), (.14, .34, .23), (.18, .41, .27)]]
    m.cylinder((0, 0, 0), [(0, .12), (.7, .092), (2.39, .04)], bark, 6)
    for y, radius, tip, material in zip([.48, .95, 1.41], [.80, .62, .42], [1.81, 2.16, 2.63], leaves):
        m.cylinder((0, y, 0), [(0, radius * .81), (.075, radius), (tip - y, .003)], material, 7)
    m.save("larch-lowpoly", HERE)
    m = Mesh()
    facets = [m.material("Слои валуна", color, .9) for color in
              [(.36, .39, .47), (.43, .45, .50), (.30, .33, .38)]]
    for i in range(7):
        def rim(index, height, size):
            angle = index * TAU / 7
            return math.cos(angle) * size, height, math.sin(angle) * size * .74
        m.face([rim(i + 1, 0, .38), rim(i, 0, .38), rim(i, .22, .54), rim(i + 1, .22, .54)], facets[0])
        m.face([rim(i + 1, .22, .54), rim(i, .22, .54), (-.08, .51, .05)], facets[1 + i % 2])
    m.save("boulder-lowpoly", HERE)


if __name__ == "__main__":
    ground_cover()
    street()
    planet()
    fox()
    lowpoly_props()
