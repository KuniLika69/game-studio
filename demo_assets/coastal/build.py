"""Build the coastal hamlet's editable houses and its tiled material maps."""
from pathlib import Path
import random
import runpy
import math

from PIL import Image, ImageEnhance, ImageOps

HERE = Path(__file__).resolve().parent
RAIN = runpy.run_path(str(HERE.parent / 'rain' / 'build.py'))
Mesh, maps = RAIN['Mesh'], RAIN['scanned_maps']


def ground_maps():
    for key, scan_name in [('meadow', 'forest_ground_06'), ('sand', 'dry_river_pebbles'),
                           ('timber', 'wood_floor_worn')]:
        src = maps(scan_name, 1024)
        image = src['baseColorTexture']
        if key == 'meadow':
            value = ImageEnhance.Contrast(ImageOps.grayscale(image)).enhance(0.72)
            base = ImageOps.colorize(value, (29, 40, 28), (147, 153, 103))
            image = Image.blend(base, image, 0.27)
        elif key == 'sand':
            value = ImageEnhance.Contrast(ImageOps.grayscale(image)).enhance(0.45)
            image = ImageOps.colorize(value, (83, 65, 49), (199, 177, 144))
        image.save(HERE / f'{key}-color.jpg', quality=93, subsampling=0)
        src['normalTexture'].save(HERE / f'{key}-normal.jpg', quality=94, subsampling=0)
        src['metallicRoughnessTexture'].getchannel('G').save(HERE / f'{key}-rough.jpg', quality=93)


def house(index):
    mesh = Mesh()
    rng = random.Random(3843 + index)
    stone_tex = maps('plaster_stone_wall_02', 1024)
    brick_tex = maps('brick_wall_09', 1024)
    wood_tex = maps('wood_floor_worn', 1024)
    d = [2.87, 3.06, 2.64][index]
    w = [3.68, 3.55, 3.81][index]
    eaves = [2.18, 2.53, 2.22][index]
    attic = [1.03, 1.22, 0.78][index]
    eave_z = d / 2 + .22
    slab = mesh.material('Фундамент из прибрежного камня', (.52, .55, .49), .99,
                         texture=stone_tex, tile_size=1.64)
    wall = mesh.material('Стены', [(1., .91, .77), (.70, .65, .58), (.49, .66, .59)][index],
                         .96, texture=(stone_tex, brick_tex, wood_tex)[index], tile_size=(2.1, 2.1, 1.35)[index])
    roof = [mesh.material('Сланец', shade, .94, texture=stone_tex, tile_size=.68)
            for shade in [(.24, .27, .29), (.26, .29, .32), (.32, .34, .33), (.20, .24, .26)]]
    wood = mesh.material('Деревянные рамы', (.58, .45, .33), .90, texture=wood_tex, tile_size=.95)
    paint = mesh.material('Краска на створках', [(.36, .53, .52), (.24, .33, .44), (.43, .28, .20)][index],
                          .78, texture=wood_tex, tile_size=1.32)
    metal = mesh.material('Оцинкованная сталь', (.21, .24, .24), .54, .55)
    panes = mesh.material('Стекло', (.12, .18, .18), .19, .25)
    light = mesh.material('Тёплая занавеска', (.43, .26, .13), .49, .14,
                         emissive=(.20, .089, .025))
    mesh.box((0, .12, 0), (w + .15, .42, d + .14), slab, .024)
    mesh.box((0, (eaves + .28) / 2, 0), (w, eaves - .28, d), wall, .018)
    for side in [-1, 1]:
        x = side * w / 2
        gable = [(x, eaves, -d / 2), (x, eaves, d / 2), (x, eaves + attic, 0)]
        mesh.face(gable if side < 0 else gable[::-1], wall)
        for row in range(12):
            # Each course overlaps the preceding one and has its own staggered joints.
            t0, t1 = row / 12, min(1.0, (row + 1.13) / 12)
            z0, z1 = side * eave_z * (1 - t0), side * eave_z * (1 - t1)
            y0, y1 = eaves + attic * t0 + .016, eaves + attic * t1
            lo, hi = -w / 2 - .18, w / 2 + .18
            step = .285
            start = lo - (step / 2 if row % 2 else 0)
            while start < hi - .025:
                a, b = max(lo, start) + .004, min(hi, start + step) - .004
                mesh.face([(a, y0, z0), (b, y0, z0), (b, y1, z1), (a, y1, z1)][::side],
                          rng.choice(roof))
                mesh.face([(a, y0 - .014, z0), (b, y0 - .014, z0), (b, y0, z0), (a, y0, z0)][::side], roof[-1])
                start += step
        mesh.box((0, eaves - .018, side * eave_z), (w + .39, .105, .083), wood, .007)
        mesh.box((0, eaves - .086, side * (eave_z + .037)), (w + .37, .058, .089), metal, .014)
        x = side * (w / 2 - .12)
        mesh.cylinder((x, .26, side * (d / 2 + .045)), [(0., .032), (eaves - .31, .032)], metal, 9)
    mesh.box((0, eaves + attic + .008, 0), (w + .40, .080, .162), roof[1], .019)
    chx = -.70 if index % 2 else .98
    mesh.box((chx, eaves + attic - .047, -.39), (.39, .97, .39), wall, .014)
    mesh.box((chx, eaves + attic + .47, -.39), (.52, .12, .51), slab, .013)
    mesh.cylinder((chx, eaves + attic + .52, -.39), [(0., .096), (.15, .10)], metal, 12)

    def window(x, y, z, side, width=.59, height=.72, warm=False, shutters=False):
        mesh.box((x, y, z), (width + .13, height + .14, .079), wood, .013)
        mesh.box((x, y, z + side * .049), (width, height, .025), light if warm else panes, .004)
        for axis, size in [(0, (.029, height, .034)), (1, (width, .032, .034))]:
            mesh.box((x, y, z + side * .070), size, wood, .003)
        mesh.box((x, y - height / 2 - .09, z + side * .055), (width + .19, .067, .215), slab, .009)
        for sign in [-1, 1] if shutters else []:
            sx = x + sign * (width * .65 + .09)
            mesh.box((sx, y, z), (.26, height + .08, .07), paint, .009)
            for j in range(5):
                mesh.box((sx, y - .26 + j * .12, z + side * .039), (.22, .019, .027), wood, .004)

    door_x = [.44, -.26, .05][index]
    mesh.box((door_x, 1.04, d / 2 + .034), (.86, 1.70, .13), wood, .016)
    for piece in range(8):
        mesh.box((door_x - .327 + piece * .093, 1.04, d / 2 + .107),
                 (.089, 1.60, .057), paint, .004)
    mesh.box((door_x + .265, 1.01, d / 2 + .148), (.062, .108, .049), metal, .008)
    mesh.box((door_x, .11, d / 2 + .19), (1.12, .20, .46), slab, .022)
    mesh.box((door_x, .032, d / 2 + .40), (1.17, .105, .27), slab, .018)
    window(-1.17, 1.33, d / 2 + .036, 1, warm=index != 2, shutters=True)
    window(1.21, 1.34, d / 2 + .036, 1, width=.44, height=.55, warm=index == 2)
    window(-.92, 1.36, -d / 2 - .036, -1, warm=index == 1)
    window(.91, 1.37, -d / 2 - .036, -1, warm=index != 1)
    for side in [-1, 1]:
        mesh.box((side * (w / 2 + .034), 1.40, .08), (.079, .79, .66), wood, .009)
        mesh.box((side * (w / 2 + .081), 1.40, .08), (.03, .64, .52), light, .007)
        mesh.box((side * (w / 2 + .10), 1.40, .08), (.042, .028, .52), wood, .003)
        mesh.box((side * (w / 2 + .085), 1.01, .08), (.22, .066, .72), slab, .011)
        mesh.box((side * (w / 2 + .03), eaves + attic * .36, 0.0), (.073, .29, .40), wood, .010)
        mesh.box((side * (w / 2 + .071), eaves + attic * .36, 0.0), (.035, .19, .30), panes, .004)
    mesh.save(f'cottage-{index + 1}', HERE)


def dune_grass():
    mesh = Mesh()
    rng = random.Random(76352)
    pigments = [(.055, .086, .032), (.097, .139, .051), (.137, .171, .069),
                (.164, .192, .073), (.259, .216, .089), (.328, .281, .127)]
    blades = [mesh.material(f'Травинка {slot}', rgb, .91, double_sided=True)
              for slot, rgb in enumerate(pigments)]
    for _ in range(1_400):
        a, angle = rng.uniform(0, math.tau), rng.uniform(0, math.tau)
        radius = math.sqrt(rng.random()) * .94
        x, z = math.cos(a) * radius, math.sin(a) * radius
        falloff = .34 + .66 * (1.0 - radius ** 1.35)
        length = rng.uniform(.095, .49) * falloff
        width = rng.uniform(.003, .010) * (length / .24) ** .31
        lean = rng.uniform(.13, .64) * length
        dx, dz = math.cos(angle), math.sin(angle)
        bias = rng.choice([0, 1, 1, 1, 2, 2, 3, 4])

        def point(t, side):
            grow = width * (1.0 - t ** 1.45)
            return (x + dx * lean * t * t - dz * side * grow,
                    length * (t - .10 * t ** 3) + .013,
                    z + dz * lean * t * t + dx * side * grow)

        for j in range(4):
            a, b = j / 4, (j + 1) / 4
            rim = [point(a, -1), point(a, 1), point(b, 1), point(b, -1)]
            mesh.face(rim[:3] if j == 3 else rim, blades[min(5, bias + j // 2)])
        if rng.random() < .07 and length > .16:
            for j in range(4):
                t = .77 + j * .05
                center = point(t, 0)
                side = point(t, (-1 if j % 2 else 1) * 2.4)
                mesh.face([center, side, point(t + .11, 0)], blades[5])
    mesh.save('dune-grass', HERE)


if __name__ == '__main__':
    ground_maps()
    for variant in range(3):
        house(variant)
    dune_grass()
