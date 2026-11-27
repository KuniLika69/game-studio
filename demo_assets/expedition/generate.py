"""Original mesh and compact pixel atlas for Studio's two coastal projects."""

import importlib.util
from math import cos, exp, pi, sin, sqrt
from pathlib import Path
from random import Random
import struct
import zlib

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("pbr_mesh", here.parent / "rain/build.py")
pbr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pbr)


def delta():
    random = Random(228)
    mesh = pbr.Mesh()
    soil = pbr.scanned_maps("forest_ground_06", 1024)
    gravel = pbr.scanned_maps("dry_river_pebbles", 1024)
    meadow = mesh.material("Почва под растительностью", (.54, .66, .39), 1,
                           texture=soil, tile_size=2.7)
    path = mesh.material("Утоптанная тропа", (.79, .74, .64), 1,
                         texture=soil, tile_size=2.3)
    shore_stone = mesh.material("Речная галька", (.57, .59, .49), .82,
                                texture=gravel, tile_size=1.67)
    bedrock = mesh.material("Сланец", (.58, .63, .61), .93, texture=gravel, tile_size=2.6)
    bark = mesh.material("Кора", (.50, .40, .28), .94, texture=pbr.wood(), tile_size=2.2)
    greens = [mesh.material("Хвоя и листья", color, .94, double_sided=True) for color in
              [(.080, .14, .070), (.108, .19, .085), (.150, .23, .083), (.20, .28, .105)]]
    straw = mesh.material("Сухие стебли", (.39, .31, .16), .91, double_sided=True)
    seeds = mesh.material("Соцветия рогоза", (.20, .12, .056), .93)

    def shore(z):
        return 4.2 + 1.4 * sin(z * .21) + .42 * cos(z * .68)

    def height(x, z):
        c = shore(z)
        left = max(0, abs(min(x, 0)) - 2.8)
        right = max(0, x - (c + 8.0))
        if x > c and right < 1.0:
            return .02 - .16 * min(1, (x - c) * .53)
        peaks = .65 + .5 * cos(z * .17) + .16 * sin(z * .79)
        contours = min(4., left * .17 + right * .25) * peaks
        enclosure = exp(-((z + 31) * .058) ** 2) * min(8, (left + right) * .31)
        relief = min(1, (left + right) * .18) * .22 * sin(x * .72 + z * .38) * cos(z * .81)
        return .015 + contours + enclosure + relief

    def normal(x, z):
        dx = height(x - .16, z) - height(x + .16, z)
        dz = height(x, z - .16) - height(x, z + .16)
        width = sqrt(dx * dx + dz * dz + .32 ** 2)
        return dx / width, .32 / width, dz / width

    # A finer grid preserves a smooth horizon without changing the walkable flat route.
    grid = [[((x * .5, height(x * .5, z * .5), z * .5), normal(x * .5, z * .5))
             for x in range(-64, 67)] for z in range(-108, 27)]
    for j, (low, high) in enumerate(zip(grid, grid[1:])):
        z = -53.75 + j * .5
        for i in range(len(low) - 1):
            x = -31.75 + i * .5
            edge = 1.45 + .19 * sin(z * .67) + random.uniform(-.13, .13)
            if abs(x - .72 * cos(z * .44)) < edge:
                material = path
            elif shore(z) - .48 < x < shore(z) + 9.2:
                material = shore_stone
            elif height(x, z) + .24 * sin(z * .61 + x) > 2.15:
                material = bedrock
            else:
                material = meadow
            points, normals = zip(low[i], high[i], high[i + 1], low[i + 1])
            mesh.face(points, material, normals=normals)

    def blade(x, y, z, angle, length, width, material):
        dx, dz = cos(angle), sin(angle)
        bend = length * random.uniform(.23, .64)
        mid = (x + dx * bend * .38, y + length * .64, z + dz * bend * .38)
        points = [(x - dz * width * .33, y, z + dx * width * .33),
                  (x + dz * width * .33, y, z - dx * width * .33),
                  (mid[0] + dz * width * .5, mid[1], mid[2] - dx * width * .5),
                  (x + dx * bend, y + length, z + dz * bend),
                  (mid[0] - dz * width * .5, mid[1], mid[2] + dx * width * .5)]
        mesh.face(points, material)

    for _ in range(350):
        z = random.uniform(-39, 10)
        bank = shore(z)
        x = random.choice([bank - .73, bank + 9.15]) + random.uniform(-.62, .58)
        y = height(x, z)
        length = random.uniform(.42, 1.0)
        for _ in range(random.randint(4, 8)):
            blade(x, y, z, random.uniform(0, 2 * pi), length * random.uniform(.44, .88),
                  random.uniform(.027, .052), random.choice(greens + [straw]))
        if random.random() < .35:
            mesh.cylinder((x, y, z), [(0, .010), (length, .008)], straw, 5)
            mesh.cylinder((x, y + length, z), [(0, .012), (.015, .025), (.16, .022),
                                               (.18, .009)], seeds, 6)

    for _ in range(410):
        x, z = random.uniform(-13, 16), random.uniform(-42, 10)
        if abs(x - .72 * cos(z * .44)) < 1.55 or shore(z) - .15 < x < shore(z) + 8.85:
            continue
        y = height(x, z)
        for _ in range(5):
            blade(x, y, z, random.uniform(0, 2 * pi), random.uniform(.12, .36),
                  random.uniform(.021, .046), random.choice(greens + [straw]))

    for _ in range(165):
        z = random.uniform(-47, 11)
        x = random.choice([shore(z) - 1.24, shore(z) + 9.6, -7.8]) + random.uniform(-1.1, 1.1)
        y = height(x, z)
        radius = random.uniform(.13, .66)
        stretch = random.uniform(.5, 1.85)
        radii = [radius * random.uniform(.72, 1.2) for _ in range(7)]
        peak = (x + radius * .19, y + radius * random.uniform(.42, .86), z - radius * .15)
        twist = random.uniform(0, 2 * pi)
        ring = [(x + cos(twist + 2 * pi * i / 7) * r, y + r * .22,
                 z + sin(twist + 2 * pi * i / 7) * r * stretch) for i, r in enumerate(radii)]
        for a, b in zip(ring, ring[1:] + ring[:1]):
            mesh.face([a, peak, b], bedrock)
            mesh.face([a, b, (b[0] * .82 + x * .18, y - radius * .17, b[2] * .82 + z * .18),
                       (a[0] * .82 + x * .18, y - radius * .17, a[2] * .82 + z * .18)], bedrock)

    # Irregular branch sprays leave gaps through the canopy and reveal textured trunks.
    for _ in range(91):
        x = random.choice([-1, 1]) * random.uniform(7.8, 30.0)
        z = random.uniform(-52, 9)
        if 0 < x < shore(z) + 9.25:
            continue
        y = height(x, z)
        size = random.uniform(2.8, 6.8)
        mesh.cylinder((x, y, z), [(0, size * .042), (.2, size * .029),
                                  (size * .77, size * .013), (size, .011)], bark, 8)
        spin = random.uniform(0, 2 * pi)
        for tier in range(6):
            spread = (.35 - tier * .043) * size * random.uniform(.79, 1.12)
            branch_y = y + (.29 + tier * .133) * size
            for limb in range(5):
                angle = spin + tier * .56 + limb * 2 * pi / 5 + random.uniform(-.16, .16)
                dx, dz = cos(angle), sin(angle)
                tip = (x + dx * spread, branch_y - spread * .12, z + dz * spread)
                mesh.face([(x, branch_y + .043, z), (x, branch_y - .047, z), tip], bark)
                for notch in range(2, 7):
                    t = notch / 7
                    base = (x + dx * spread * t, branch_y - spread * t * .12,
                            z + dz * spread * t)
                    for side in [-1, 1]:
                        width = spread * (1.16 - t) * random.uniform(.39, .68)
                        a = (base[0] + dx * width * .9, base[1] + width * .02,
                             base[2] + dz * width * .9)
                        b = (a[0] - dz * width * side, a[1] - width * .46,
                             a[2] + dx * width * side)
                        crest = (base[0] + dx * width * .62 - dz * width * side * .42,
                                 base[1] + width * .26,
                                 base[2] + dz * width * .62 + dx * width * side * .42)
                        mesh.face([base, a, crest], random.choice(greens))
                        mesh.face([base, crest, b], random.choice(greens))

    mesh.save("delta", here)


def natural_props():
    rng = Random(712)
    tree = pbr.Mesh()
    bark = tree.material("Серая кора ольхи", (.69, .58, .47), .9,
                         texture=pbr.wood(), tile_size=1.8)
    leaf_materials = [tree.material("Листья ольхи", color, .81, double_sided=True)
                      for color in [(.107, .21, .064), (.148, .27, .082),
                                    (.123, .25, .094), (.189, .31, .114)]]

    def shoot(points, radii, sides=7):
        rings = [[(x + radius * cos(a * 2 * pi / sides), y,
                   z + radius * sin(a * 2 * pi / sides)) for a in range(sides)]
                 for (x, y, z), radius in zip(points, radii)]
        for low, high in zip(rings, rings[1:]):
            for i in range(sides):
                j = (i + 1) % sides
                tree.face([low[j], low[i], high[i], high[j]], bark)

    shoot([(0, 0, 0), (-.04, .19, .01), (.016, .87, .01), (.09, 2.21, -.1),
           (-.10, 3.08, -.02), (-.03, 3.98, .07)], [.186, .119, .104, .083, .052, .009], 11)
    for i in range(19):
        t = i / 18
        direction = i * 2.399963
        dx, dz = cos(direction), sin(direction)
        y = 2.37 + 1.22 * t + rng.uniform(-.10, .13)
        reach = 1.24 * sin(.35 + 2.68 * t) + .32
        reach *= rng.uniform(.82, 1.11)
        tip = (dx * reach, min(4.15, y + rng.uniform(.30, .62)), dz * reach)
        knee = (tip[0] * .5, y + .32, tip[2] * .46)
        shoot([(.013, y, -.022), knee, tip], [.053 * (1 - .45 * t), .032, .006])
        for j in range(5):
            f = .35 + j * .147
            mid = (dx * reach * f, y + (tip[1] - y) * f, dz * reach * f)
            side = (-1) ** j
            plume = .34 * (1 - .4 * t)
            twig_tip = (mid[0] - dz * side * plume, mid[1] + .23, mid[2] + dx * side * plume)
            shoot([mid, twig_tip], [.010, .002], 5)
            for k in range(39):
                spread = rng.uniform(.06, .36)
                angle = rng.uniform(0, 2 * pi)
                bx, bz = cos(angle), sin(angle)
                start = (twig_tip[0] + bx * spread, twig_tip[1] + rng.uniform(-.19, .22),
                         twig_tip[2] + bz * spread)
                length = rng.uniform(.09, .17)
                sx, sy, sz = start
                bend = rng.uniform(-.065, .031)
                end = (sx + bx * length, sy + bend, sz + bz * length)
                mx, my, mz = sx + bx * length * .48, sy + bend * .32 + .011, sz + bz * length * .48
                width = length * rng.uniform(.18, .34)
                tree.face([start, (mx - bz * width, my - .009, mz + bx * width),
                           end, (mx + bz * width, my - .012, mz - bx * width)],
                          rng.choice(leaf_materials))
    tree.save("alder", here)

    rock = pbr.Mesh()
    stone = rock.material("Слоистый речной камень", (.67, .69, .66), .94,
                          texture=pbr.scanned_maps("dry_river_pebbles", 512), tile_size=1.4)
    rings = [[(cos(.12 * level + 2 * pi * i / 9) * width * rng.uniform(.82, 1.12),
               y + rng.uniform(-.038, .038),
               sin(.12 * level + 2 * pi * i / 9) * width * rng.uniform(.68, .91))
              for i in range(9)] for level, (y, width) in enumerate(
                  [(.02, .38), (.14, .53), (.32, .48), (.53, .24)])]
    for low, high in zip(rings, rings[1:]):
        for i in range(9):
            j = (i + 1) % 9
            rock.face([low[j], low[i], high[i]], stone)
            rock.face([low[j], high[i], high[j]], stone)
    for i in range(9):
        rock.face([rings[-1][i], (-.12, .56, .053), rings[-1][(i + 1) % 9]], stone)
    rock.save("slate", here)


def atlas():
    width, height = 10 * 32, 32
    art = bytearray(width * height * 4)
    rnd = Random(173)

    def point(i, x, y, color):
        if 0 <= x < 32 and 0 <= y < 32:
            at = ((i * 32 + x) + y * width) * 4
            art[at:at + 4] = bytes((*color, 255))

    def fill(i, box, color):
        x0, y0, x1, y1 = box
        for x in range(x0, x1):
            for y in range(y0, y1):
                point(i, x, y, color)

    def disk(i, x, y, size, color):
        for dx in range(-size, size + 1):
            for dy in range(-size, size + 1):
                if dx * dx + dy * dy <= size * size:
                    point(i, x + dx, y + dy, color)

    bases = [(15, 38, 58), (25, 71, 91), (66, 112, 106), (110, 93, 76), (32, 47, 62)]
    for tile, base in enumerate(bases):
        fill(tile, (0, 0, 32, 32), base)
        for _ in range(62):
            x, y = rnd.randrange(32), rnd.randrange(32)
            shift = rnd.choice([-8, -3, 4, 9])
            color = tuple(max(0, c + shift) for c in base)
            point(tile, x, y, color)
        if tile < 2:
            for x, y, length in [(3, 6, 8), (16, 14, 9), (6, 23, 6), (24, 27, 3)]:
                fill(tile, (x, y, x + length, y + 1), tuple(c + 11 for c in base))
        elif tile == 3:
            for y in (5, 13, 22, 30):
                fill(tile, (0, y, 32, y + 1), (71, 66, 64))
                fill(tile, (0, y + 1, 32, y + 2), (131, 110, 83))
        elif tile == 4:
            fill(tile, (2, 2, 30, 8), (51, 73, 84))
            fill(tile, (2, 8, 8, 28), (40, 57, 70))
            fill(tile, (24, 8, 30, 29), (27, 38, 50))

    for x, y, size, color in [(16, 19, 11, (34, 59, 70)), (11, 17, 8, (35, 100, 82)), (22, 15, 8, (65, 133, 96)), (16, 9, 7, (108, 150, 102)), (18, 9, 3, (237, 201, 124))]:
        disk(5, x, y, size, color)
    for tile, gold in [(6, (224, 157, 86)), (8, (248, 195, 108))]:
        for x, y, size, color in [(16, 17, 12, (40, 48, 59)), (16, 16, 10, gold), (16, 16, 8, (63, 64, 70)), (16, 16, 6, (87, 158, 164)), (16, 16, 3, (184, 230, 215))]:
            disk(tile, x, y, size, color)
        fill(tile, (14, 3, 18, 7), gold)
        fill(tile, (14, 26, 18, 30), gold)
        if tile == 8:
            fill(tile, (1, 15, 5, 17), (255, 226, 177))
            fill(tile, (27, 15, 31, 17), (255, 226, 177))
    for box, color in [
        ((9, 8, 23, 24), (35, 44, 55)),
        ((10, 11, 22, 23), (202, 116, 63)),
        ((9, 11, 12, 21), (227, 155, 81)),
        ((20, 11, 23, 21), (227, 155, 81)),
        ((13, 9, 19, 17), (241, 190, 104)),
        ((11, 22, 15, 28), (40, 74, 79)),
        ((17, 22, 21, 28), (40, 74, 79)),
        ((11, 26, 16, 29), (31, 51, 64)),
        ((17, 26, 22, 29), (31, 51, 64)),
    ]:
        fill(7, box, color)
    disk(7, 16, 9, 6, (48, 58, 74))
    disk(7, 16, 9, 5, (102, 186, 176))
    fill(7, (13, 7, 18, 9), (188, 230, 203))
    fill(7, (14, 13, 18, 17), (243, 209, 137))

    fill(9, (0, 0, 32, 32), (56, 83, 87))
    for x, y in [(4, 2), (17, 5), (28, 3), (10, 16), (21, 14), (5, 24), (26, 24), (16, 29)]:
        fill(9, (x, y, x + 5, y + 2), (66, 105, 102))
        fill(9, (x + 2, y - 1, x + 3, y + 3), (77, 120, 108))

    def chunk(name, data):
        return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data))

    rows = b"".join(b"\0" + art[y * width * 4:(y + 1) * width * 4] for y in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    (here / "tidal-atlas.png").write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
                                      + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


if __name__ == "__main__":
    delta()
    natural_props()
    atlas()
