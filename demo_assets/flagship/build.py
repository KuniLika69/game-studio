"""Original quay structures and signage built into the 0.23 workshop chapter."""

import importlib.util
import math
import random
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("rain_mesh", OUT.parent / "rain" / "build.py")
RAIN = importlib.util.module_from_spec(SPEC)
sys.dont_write_bytecode = True
SPEC.loader.exec_module(RAIN)
FONT = "C:/Windows/Fonts/bahnschrift.ttf"


def save(mesh, name):
    original = RAIN.OUTPUT
    try:
        RAIN.OUTPUT = OUT
        mesh.save(name)
    finally:
        RAIN.OUTPUT = original


def sign(title, subtitle, tint):
    image = Image.new("RGB", (768, 192), (15, 22, 26))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle((10, 9, 759, 182), radius=14, outline=tint, width=3)
    draw.text((383, 63), title, fill=tint, font=ImageFont.truetype(FONT, 66), anchor="mm")
    draw.line((48, 107, 719, 107), fill=tint, width=2)
    draw.text((383, 141), subtitle, fill=(210, 208, 186), font=ImageFont.truetype(FONT, 30), anchor="mm")
    return image


def sign_face(mesh, center, w, h, material):
    x, y, z = center
    mesh.face(
        [(x - w/2, y - h/2, z), (x + w/2, y - h/2, z),
         (x + w/2, y + h/2, z), (x - w/2, y + h/2, z)],
        material, [(0, 1), (1, 1), (1, 0), (0, 0)],
    )


def cable(mesh, start, end, sag, radius, material):
    # Each span is octagonal and has a shallow parabolic sag.
    ax, ay, az = start
    bx, by, bz = end
    xz = math.hypot(bx - ax, bz - az)
    nx, nz = (bz - az) / xz, (ax - bx) / xz
    sides = 8
    def ring(t, side):
        a = math.tau * side / sides
        return (
            ax + t * (bx - ax) + nx * radius * math.cos(a),
            ay + t * (by - ay) - 4 * sag * t * (1-t) + radius * math.sin(a),
            az + t * (bz - az) + nz * radius * math.cos(a),
        )
    for i in range(16):
        lo, hi = i / 16, (i + 1) / 16
        for side in range(sides):
            mesh.face([ring(lo, side), ring(lo, side+1), ring(hi, side+1), ring(hi, side)], material)


def roadway():
    m = RAIN.Mesh()
    maps = RAIN.scanned_maps("cobblestone_pavement")
    wet = m.material("Мокрые колеи", (.73, .81, .85), .44, .12, texture=maps, tile_size=2.5)
    stone = m.material("Открытый шероховатый гранит", (.65, .68, .65), .85, 0, texture=maps, tile_size=2.9)
    walk = m.material("Гранит под карнизами", (.73, .71, .64), 1.0, 0,
                      texture=RAIN.stone(False), tile_size=1.75)
    iron = m.material("Чугун и сетка", (.12, .14, .15), .46, .63)
    dark = m.material("Ил и грязь у берега", (.102, .122, .121), .72)
    mark = m.material("Истёртая дорожная эмаль", (.44, .40, .30), .68)
    for a, b in [(-78.5, -24), (24, 78.5)]:
        m.horizontal(a, b, -5.8, -1.7, .012, wet)
        m.horizontal(a, b, -1.7, 9.4, .012, stone)
        m.horizontal(a, b, -19.9, -5.8, .046, walk)
        for j in range(math.ceil(a), math.ceil(b), 2):
            m.box((j + .5, .025, -5.78), (1.97, .085, .29), walk)
        for j in range(math.ceil(a+3), math.ceil(b-2), 7):
            m.horizontal(j, j+1.75, 0.85, 1.0, .017, mark)
            m.box((j, .012, -5.51), (.74, .055, .25), iron)
    for side in [-1, 1]:
        m.horizontal(min(71*side, 78*side), max(71*side, 78*side), -22, -19.9, -.09, dark)
        for i in range(8):
            x = side * (31 + i * 6)
            m.box((x, .27, 5.8), (.16, .54, .17), iron)
            if i != 7:
                m.box((x + side*3.1, .48, 5.8), (6.2, .06, .075), iron)
    save(m, "quay-ground")


def profiles():
    m = RAIN.Mesh()
    sandstone = m.material("Тёплый песчаник", (.43, .43, .37), .87)
    dark = m.material("Антрацитовая рама", (.071, .107, .114), .62, .34)
    zinc = m.material("Окисленная кровля", (.20, .29, .26), .47, .56)
    brass = m.material("Латунный профиль", (.47, .32, .17), .34, .67)
    frost = m.material("Свет витрины", (.81, .42, .21), .27, emissive=(.61, .17, .057))
    blue = m.material("Холодная эмаль", (.37, .77, .84), .30, emissive=(.19, .65, .72))
    tile = m.material("Время работы мастерской", (1, 1, 1), .52,
                      texture=dict(baseColorTexture=sign("ДЕПО", "НАБЕРЕЖНАЯ · СЕВЕР", (217, 182, 133))))
    for center in [-2.30, 2.30]:
        for x in [center-.88, center+.88]:
            m.box((x, 2.24, .149), (.17, 1.82, .32), sandstone, .027)
            m.box((x, 2.24, .325), (.049, 1.65, .051), brass, .006)
        for y in [1.33, 3.15]:
            m.box((center, y, .16), (1.96, .19, .37), sandstone, .021)
        m.box((center, 1.43, .29), (1.76, .045, .32), dark, .013)
        m.box((center, 3.05, .32), (1.73, .038, .054), frost, .006)
        for dx in [-.43, .43]:
            m.box((center+dx, 2.20, .124), (.022, 1.50, .036), dark, .005)
    m.box((-1.8, 4.24, -2.25), (1.15, .81, 2.2), zinc, .082)
    for x in [-3.95, 3.95]:
        m.box((x, 4.12, .18), (.22, .24, .54), brass, .025)
    m.box((3.62, 2.18, .42), (.53, .72, .17), dark, .027)
    sign_face(m, (3.62, 2.18, .511), .49, .68, tile)
    m.box((0, 2.855, 1.305), (3.51, .021, .027), blue, .004)
    # Pipes and small mechanical devices occupy the rear workshop walls.
    for x in [-3.46, -3.13, 2.68]:
        m.cylinder((x, .145, -7.28), [(0, .084), (.09, .091), (.14, .060),
                   (2.91, .057), (2.98, .083), (3.052, .086)], brass)
        for y in [.53, 1.37, 2.11]:
            m.box((x, y, -7.275), (.23, .075, .16), dark, .014)
    for x in [1.85, 2.55, 3.21]:
        m.box((x, .55, -6.53), (.54, .75, .67), dark, .034)
        m.box((x, .58, -6.161), (.46, .04, .036), brass, .009)
        m.box((x, .96, -6.54), (.59, .11, .70), zinc)
    save(m, "workshop-detailed")


def street_scenery():
    m = RAIN.Mesh()
    iron = m.material("Силовые мачты", (.13, .16, .17), .42, .67)
    cap = m.material("Выветрившийся кирпич", (.43, .28, .19), .84)
    bronze = m.material("Козырьки галереи", (.23, .31, .28), .46, .54)
    glaze = m.material("Арматурное стекло", (.21, .40, .43), .38, .29)
    moss = [m.material("Листва на стене", color, .65) for color in [
        (.16, .26, .19), (.22, .35, .23), (.28, .37, .27), (.33, .43, .29)
    ]]
    notices = [
        m.material("Фарфоровая панель", (1, 1, 1), .57, texture=dict(baseColorTexture=sign(title, line, color)), emissive=emission)
        for title, line, color, emission in [
            ("СЕВЕР", "МАСТЕРСКАЯ · 019", (224, 197, 133), (.17, .095, .025)),
            ("ЗАПАД", "ОБВОДНОЙ ЛОТОК", (125, 200, 204), (.041, .16, .17)),
            ("ВОСТОК", "РЕЛЕ СВЕТОВОЙ ЛИНИИ", (234, 154, 89), (.19, .061, .023)),
        ]
    ]
    pylons = [(-62, 5.7, -7.12), (-41, 5.7, -6.90), (-21, 5.7, -7.51), (-6, 5.4, -5.83),
              (5.6, 5.9, -5.93), (22, 6.9, -7.37), (42, 5.7, -6.85), (63, 5.7, -6.95)]
    for x, h, z in pylons:
        m.cylinder((x, .055, z), [(0, .18), (.35, .12), (.45, .082), (h, .053)], iron, 10)
        m.box((x, h-.13, z), (.096, .063, 1.24), iron, .012)
    for start, end in zip(pylons, pylons[1:]):
        for offset in [-.52, .52]:
            cable(m, (start[0], start[1]-.13, start[2]+offset),
                  (end[0], end[1]-.13, end[2]+offset), .58, .016, iron)
    for x, sign_material in [(-36, notices[1]), (3.1, notices[0]), (39.0, notices[2])]:
        m.box((x, 2.65, -6.90), (3.04, .71, .17), bronze, .038)
        sign_face(m, (x, 2.65, -6.805), 2.92, .64, sign_material)
    for x0 in [-58.0, -30.0, 31.5, 57.0]:
        for k in range(4):
            x = x0 + k*1.47
            m.box((x, 1.85, -7.45), (.30, 3.56, .29), cap, .026)
            if k == 3:
                continue
            m.box((x+.74, 2.38, -7.54), (1.19, 1.38, .13), glaze)
            m.box((x+.73, 3.43, -7.41), (1.46, .17, .47), bronze, .028)
    rng = random.Random(8838)
    for x0 in [-47, -19, 12, 33, 55, 66]:
        for _ in range(63):
            x = x0 + rng.uniform(-.55, .55)
            y = rng.uniform(.65, 3.9)
            z = -7.06 + rng.uniform(-.08, .15)
            w = rng.uniform(.06, .21)
            points = [(x, y, z), (x+w, y+.16, z+.02), (x+w*.36, y+.29, z-.045)]
            index = rng.choice(moss)
            m.face(points, index)
            m.face(points[::-1], index)
    save(m, "quay-fronts")


def gauge(east):
    m = RAIN.Mesh()
    stone = m.material("Гранитное основание", (.24, .28, .28), .72)
    iron = m.material("Окрашенная арматура", (.09, .15, .15), .58, .55)
    brass = m.material("Латунная шайба", (.49, .35, .16), .29, .73)
    glass = m.material("Сигнальное стекло", (.83, .47, .16) if east else (.20, .67, .70), .30,
                       emissive=(.53, .20, .029) if east else (.055, .36, .41))
    display = m.material("Маркировка поста", (1, 1, 1), .43, texture=dict(baseColorTexture=
        sign("РЕЛЕ" if east else "ЛОТОК", "УПРАВЛЕНИЕ", (230, 173, 111) if east else (117, 215, 214))))
    m.box((0, .10, 0), (1.30, .20, 1.03), stone, .045)
    m.box((0, .728, -.08), (.95, 1.09, .55), iron, .039)
    m.box((0, 1.34, -.08), (1.03, .19, .64), iron, .04)
    sign_face(m, (0, 1.05, .201), .79, .38, display)
    for x in [-.29, -.05, .19]:
        m.box((x, .72, .219), (.14, .10, .063), brass, .018)
        m.box((x, .72, .262), (.089, .074, .04), glass, .013)
    m.cylinder((-.46, .294, -.19), [(0, .077), (1.39, .073), (1.43, .104)], brass)
    m.box((.48, .90, -.17), (.046, .26, .316), brass, .014)
    save(m, "east-relay" if east else "west-gauge")


if __name__ == "__main__":
    roadway()
    profiles()
    street_scenery()
    gauge(False)
    gauge(True)
