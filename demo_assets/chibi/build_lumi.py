"""Author the original Lumi starter. Run with Blender in background mode."""

import math
import sys
from pathlib import Path

import bpy
from mathutils import Vector
import numpy as np


HERE = Path(__file__).resolve().parent
TAU = math.tau
bpy.ops.wm.read_factory_settings(use_empty=True)


def pigment(hex_color):
    def linear(channel):
        return channel / 12.92 if channel <= 0.04045 else ((channel + 0.055) / 1.055) ** 2.4
    return [linear(int(hex_color[i:i + 2], 16) / 255) for i in (0, 2, 4)]


def material(name, color, roughness=0.68, metal=0):
    result = bpy.data.materials.new(name)
    result.diffuse_color = (*pigment(color), 1)
    result.use_nodes = True
    shader = result.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = result.diffuse_color
    shader.inputs["Roughness"].default_value = roughness
    shader.inputs["Metallic"].default_value = metal
    return result


def painted_map(mat, name, rgb):
    height, width = rgb.shape[:2]
    image = bpy.data.images.new(name, width=width, height=height, alpha=False)
    image.colorspace_settings.name = 'sRGB'
    rgba = np.concatenate((np.clip(rgb, 0, 1), np.ones((height, width, 1))), axis=2)
    image.pixels.foreach_set(rgba.astype(np.float32).ravel())
    image.pack()
    nodes = mat.node_tree.nodes
    sampler = nodes.new('ShaderNodeTexImage')
    sampler.image = image
    mat.node_tree.links.new(sampler.outputs['Color'], nodes.get('Principled BSDF').inputs['Base Color'])


def uv_grid(size):
    pixels = (np.arange(size) + 0.5) / size
    return np.meshgrid(pixels, pixels)


HOOD = material('Lumi | Hood', '9783c8', 0.95)
FABRIC = material('Lumi | Fabric', '687bb4', 0.93)
TRIM = material('Lumi | Trim', 'f1dfdb', 0.91)
HAIR = material('Lumi | Hair', 'f9dfb4', 0.65)
SKIN = material('Lumi | Skin', 'f7c7b1', 0.78)
SCLERA = material('Lumi | Sclera', 'fff2ea', 0.48)
IRISES = material('Lumi | Irises', '9752c4', 0.52)
LASH = material('Lumi | Lashes', '55344b', 0.82)
BROW = material('Lumi | Brows', 'a56a59', 0.88)
NOSE = material('Lumi | Blush', 'f3a998', 0.88)
MOUTH = material('Lumi | Smile', 'a04c65', 0.78)
LINING = material('Lumi | Hood lining', 'e0a8ba', 0.94)
SCARF = material('Lumi | Scarf', '96d3ce', 0.94)
STITCH = material('Lumi | Stitching', 'c4b0de', 0.96)
SOLE = material('Lumi | Sole', '696382', 0.89)
BRASS = material('Lumi | Brass', 'eaa75f', 0.39, 0.27)


def textures():
    rng = np.random.default_rng(6248)
    x, y = uv_grid(512)
    twill = 0.010 * (np.sin(x * TAU * 157 + y * TAU * 91) + np.cos(y * TAU * 161))
    nap = rng.normal(0.0, 0.014, x.shape) + twill * 0.45
    hood = np.array([0.568, 0.469, 0.747]) + (nap + 0.084 * y + 0.016 * np.cos(x * TAU * 3))[..., None]
    painted_map(HOOD, 'Lumi | lavender brushed plush', hood)
    weave = np.array([0.346, 0.417, 0.652]) + (twill + 0.105 * y + nap * 0.36)[..., None]
    painted_map(FABRIC, 'Lumi | indigo twill', weave)

    face = np.broadcast_to([0.970, 0.799, 0.711], (*x.shape, 3)).copy()
    blush = sum(np.exp(-((x - center) / 0.033) ** 2 - ((y - 0.438) / 0.047) ** 2) for center in (0.387, 0.613))
    lips = np.exp(-((x - 0.5) / 0.035) ** 2 - ((y - 0.356) / 0.040) ** 2)
    for center in (0.41, 0.59):
        for _ in range(10):
            uf, vf = center + rng.uniform(-0.019, 0.019), 0.463 + rng.uniform(-0.016, 0.014)
            freckle = np.exp(-((x - uf) / 0.0017) ** 2 - ((y - vf) / 0.0021) ** 2)
            face -= freckle[..., None] * [0.063, 0.114, 0.135]
    face += blush[..., None] * [0.004, -0.249, -0.201]
    face += lips[..., None] * [0.023, -0.047, -0.035]
    painted_map(SKIN, 'Lumi | painted blush and freckles', face)

    x, y = uv_grid(384)
    x, y = (x - 0.5) * 2, (y - 0.5) * 2
    radius = np.sqrt(x*x + y*y)
    angle = np.arctan2(y, x)
    light = 0.52 * np.clip(0.62 - 0.64 * y, 0, 1)
    filament = (0.035 * np.sin(67 * angle + 11 * radius) + 0.024 * np.cos(103 * angle - 5 * radius))
    filament *= np.clip((radius - 0.26) * 7, 0, 1)
    iris = np.array([0.28, 0.125, 0.49]) + (light + filament)[..., None] * [0.91, 0.70, 0.87]
    eye_rim = np.clip((radius - 0.83) * 9, 0, 1)
    pupil_radius = np.sqrt((x * 0.97)**2 + ((y - 0.08) * 0.94)**2)
    pupil = np.clip((0.47 - pupil_radius) * 19, 0, 1)
    iris = iris * (1 - eye_rim[..., None]) + np.array([0.215, 0.103, 0.348]) * eye_rim[..., None]
    iris = iris * (1 - pupil[..., None]) + np.array([0.119, 0.071, 0.184]) * pupil[..., None]
    for u, v, rx, ry, color in [(-0.315, 0.444, 0.198, 0.148, [1, 0.985, 0.95]),
                                  (0.383, 0.211, 0.076, 0.096, [0.97, 0.915, 1]),
                                  (-0.282, -0.641, 0.21, 0.054, [0.91, 0.58, 0.90])]:
        shine = np.clip((1 - ((x - u) / rx) ** 2 - ((y - v) / ry) ** 2) * 9, 0, 1)
        iris = iris * (1 - shine[..., None]) + np.array(color) * shine[..., None]
    painted_map(IRISES, 'Lumi | painted violet iris', iris)


def node(name, pivot, parent=None):
    obj = bpy.data.objects.new(name, None)
    bpy.context.collection.objects.link(obj)
    obj.parent = parent
    obj.location = Vector(pivot) - (Vector(parent['world_pivot']) if parent else Vector((0, 0, 0)))
    obj['world_pivot'] = Vector(pivot)
    obj.rotation_mode = 'XYZ'
    return obj


def mesh(name, points, triangles, mat, parent, uv=None):
    geometry = bpy.data.meshes.new(name)
    geometry.from_pydata(points, [], triangles)
    geometry.update()
    for poly in geometry.polygons:
        poly.use_smooth = True
    if uv is not None:
        coords = geometry.uv_layers.new(name='UVMap')
        for loop in geometry.loops:
            coords.data[loop.index].uv = uv[loop.vertex_index]
    obj = bpy.data.objects.new(name, geometry)
    bpy.context.collection.objects.link(obj)
    obj.parent = parent
    for vertex in geometry.vertices:
        vertex.co -= Vector(parent['world_pivot'])
    geometry.materials.append(mat)
    return obj


def round_shape(name, center, radii, mat, parent, stretch=0):
    cx, cy, cz = center
    rx, ry, rz = radii
    steps, rings = 56, 37
    points, uv = [], []
    for ring in range(rings):
        v = ring / (rings - 1)
        lat = (v - 0.5) * (math.pi - 0.001)
        for col in range(steps + 1):
            u = col / steps
            theta = (u - 0.5) * TAU
            cross = math.cos(lat) * (1.0 + stretch * math.sin(lat))
            points.append((cx + rx * math.sin(theta) * cross, cy - ry * math.cos(theta) * cross, cz + rz * math.sin(lat)))
            uv.append((u, v))
    return mesh(name, points, grid_faces(steps + 1, rings), mat, parent, uv)


def grid_faces(width, height, reverse=False):
    result = []
    for row in range(height - 1):
        for col in range(width - 1):
            a = (row * width) + col
            face = (a, a + 1, a + width + 1, a + width)
            result.append(face[::-1] if reverse else face)
    return result


def cubic(points, count=24):
    a, b, c, d = [Vector(p) for p in points]
    return [(1 - t)**3 * a + 3 * (1 - t)**2 * t * b + 3 * (1 - t) * t**2 * c + t**3 * d
            for t in np.linspace(0, 1, count)]


def tube_geometry(path, radii, depth=1.0, rings=12):
    points, uv = [], []
    for i, (p, radius) in enumerate(zip(path, radii)):
        p = Vector(p)
        t = (Vector(path[min(i + 1, len(path) - 1)]) - Vector(path[max(i - 1, 0)])).normalized()
        side = t.cross(Vector((0, 1, 0))).normalized()
        face = side.cross(t).normalized()
        for k in range(rings + 1):
            angle = TAU * k / rings
            points.append(p + side * (radius * math.cos(angle)) + face * (radius * depth * math.sin(angle)))
            uv.append((k / rings, i / (len(path) - 1)))
    return points, grid_faces(rings + 1, len(path), True), uv


def tube(name, path, radii, mat, parent, depth=1.0, rings=12):
    points, faces, uv = tube_geometry(path, radii, depth, rings)
    return mesh(name, points, faces, mat, parent, uv)


def line(name, path, radius, mat, parent):
    return tube(name, path, [radius] * len(path), mat, parent, rings=8)


def soft_lock(name, controls, width, mat, parent, depth=0.38):
    path = cubic(controls)
    radii = [0.0023 + width * math.sin(math.pi * t) ** 0.64 * (1.19 - 0.65 * t) for t in np.linspace(0, 1, len(path))]
    return tube(name, path, radii, mat, parent, depth, 18)


def flex(obj, name, factor, weighting=lambda x: 1.0):
    obj.shape_key_add(name='Basis') if obj.data.shape_keys is None else None
    key = obj.shape_key_add(name=name)
    key.value = 0.0
    for point in key.data:
        point.co.z = point.co.z * (1 - weighting(point.co.x) * (1 - factor))


def hood_surface(head):
    points, uv = [], []
    front = []
    rings, sectors = 35, 81
    for i in range(rings):
        v = i / (rings - 1)
        for j in range(sectors):
            t = TAU * j / (sectors - 1)
            opening = 1.108 + 0.090 * math.sin(t) - 0.025 * math.cos(t * 2)
            phi = opening * (1 - v) + (math.pi - 0.003) * v
            span = (1 + 0.015 * math.sin(t * 5) * math.sin(math.pi * v)) * math.sin(phi)
            p = (0.478 * span * math.cos(t), 0.015 - 0.425 * math.cos(phi), 1.135 + 0.458 * span * math.sin(t))
            points.append(p)
            uv.append((j / (sectors - 1), math.sin(phi) * 0.34 + (math.sin(t) + 1) * 0.21))
            if i == 0:
                front.append(Vector(p))
    mesh('Hood plush shell', points, grid_faces(sectors, rings, True), HOOD, head, uv)
    rim = [Vector((p.x * 1.003, p.y - 0.014, p.z)) for p in front]
    inner = []
    for p in rim:
        x, z = p.x * 0.859, 1.135 + (p.z - 1.135) * 0.87
        inner.append(Vector((x, skin_depth(x, z) + 0.019, z)))
    mesh('Warm inset lining', rim + inner, grid_faces(sectors, 2, True), LINING, head)
    line('Hood rolled edging', rim, 0.037, TRIM, head)
    line('Hood seam under edging', [p + Vector((0, -0.016, 0)) for p in rim], 0.013, STITCH, head)
    stitch_points, stitch_faces = [], []
    for j, p in enumerate(rim[:-1]):
        if j % 2 or p.y > -0.115:
            continue
        q = rim[j + 1]
        stitch = [p.lerp(q, i / 7) + Vector((0, -0.034, 0)) for i in range(3)]
        points, faces, _ = tube_geometry(stitch, [0.0027] * len(stitch), rings=8)
        offset = len(stitch_points)
        stitch_points.extend(points)
        stitch_faces.extend(tuple(index + offset for index in face) for face in faces)
    mesh('Hood hand-stitched edge', stitch_points, stitch_faces, TRIM, head)


def skin_depth(x, z):
    return -0.056 - 0.365 * math.sqrt(max(0.012, 1 - (x / 0.414)**2 - ((z - 1.136) / 0.376)**2))


def eye_panel(name, side, parent):
    x0, z0 = 0.169 * side, 1.128
    pivot = (x0, skin_depth(x0, z0), z0)
    eye = node('Eye.' + name, pivot, parent)
    blink = 'EyeBlinkLeft' if name == 'L' else 'EyeBlinkRight'
    steps, rings = 48, 18
    for iris in (False, True):
        points, uv = [], []
        for i in range(rings):
            radius = max(0.0001, i / (rings - 1))
            for j in range(steps + 1):
                t = TAU * j / steps
                dx, dz = math.cos(t) * radius, math.sin(t) * radius
                x = x0 + (0.092 * dx - 0.008 * side if iris else 0.131 * dx)
                z = z0 + (0.112 * dz + 0.003 if iris else (0.124 if dz > 0 else 0.104) * dz + 0.019 * side * dx)
                curve = 0.010 * math.sqrt(max(0, 1 - (x - x0)**2 / 0.139**2 - (z - z0)**2 / 0.140**2))
                y = skin_depth(x, z) - curve - (0.019 if iris else 0.009)
                points.append((x, y, z))
                uv.append((0.5 + dx * 0.5, 0.5 + dz * 0.5))
        obj = mesh('Iris.' + name if iris else 'Sclera.' + name, points, grid_faces(steps + 1, rings, True), IRISES if iris else SCLERA, eye, uv)
        flex(obj, blink, 0.018)
    upper = []
    for t in np.linspace(0, math.pi, 39):
        dx, dz = math.cos(t), math.sin(t)
        x, z = x0 + 0.131 * dx, z0 + 0.124 * dz + 0.019 * side * dx
        upper.append(Vector((x, skin_depth(x, z) - 0.021, z)))
    lengths = [0.0035 + 0.0104 * math.sin(math.pi * t) ** 0.73 for t in np.linspace(0, 1, len(upper))]
    eyelash = tube('Upper soft eyeliner.' + name, upper, lengths, LASH, eye, 0.65, 10)
    flex(eyelash, blink, 0.10)
    tail_x = x0 + side * 0.131
    lash = soft_lock('Outer lash.' + name, [(tail_x - side * 0.04, skin_depth(tail_x, z0) - 0.031, z0 + 0.026),
                    (tail_x + side * 0.010, skin_depth(tail_x, z0) - 0.022, z0 + 0.023),
                    (tail_x + side * 0.020, skin_depth(tail_x, z0) - 0.010, z0 + 0.051),
                    (tail_x + side * 0.025, skin_depth(tail_x, z0) - 0.006, z0 + 0.066)], 0.009, LASH, eye)
    flex(lash, blink, 0.14)
    return eye


def head_details(head):
    round_shape('Face | skin', (0, -0.056, 1.136), (0.414, 0.365, 0.376), SKIN, head)
    hood_surface(head)
    ears = []
    for tag, s in (('L', -1), ('R', 1)):
        ear = node('Hood.' + tag, (s * 0.29, -0.005, 1.46), head)
        ears.append(ear)
        w = 0.116 if s < 0 else 0.105
        run = [(s * 0.251, 0.009, 1.447), (s * 0.28, -0.020, 1.575), (s * 0.393, -0.006, 1.693), (s * 0.422, 0.035, 1.723)]
        soft_lock('Owl hood tuft.' + tag, run, w, HOOD, ear, 0.85)
        run = [(s * 0.285, -0.071, 1.488), (s * 0.322, -0.096, 1.588), (s * 0.389, -0.041, 1.669), (s * 0.410, -0.010, 1.688)]
        soft_lock('Owl tuft lining.' + tag, run, w * 0.59, LINING, ear, 0.50)
    for i, (a, b, c, d, width) in enumerate([
        ((-0.243, -0.195, 1.436), (-0.212, -0.369, 1.469), (0.023, -0.344, 1.462), (0.166, -0.334, 1.393), 0.088),
        ((-0.145, -0.300, 1.446), (-0.047, -0.370, 1.452), (0.166, -0.355, 1.400), (0.193, -0.331, 1.329), 0.080),
        ((0.014, -0.302, 1.463), (0.145, -0.340, 1.454), (0.259, -0.307, 1.404), (0.292, -0.268, 1.288), 0.070),
        ((-0.215, -0.284, 1.442), (-0.296, -0.338, 1.415), (-0.329, -0.306, 1.343), (-0.337, -0.245, 1.256), 0.075),
        ((-0.355, -0.193, 1.296), (-0.394, -0.209, 1.203), (-0.392, -0.234, 1.089), (-0.335, -0.267, 1.073), 0.039),
        ((0.34, -0.193, 1.291), (0.395, -0.218, 1.218), (0.383, -0.248, 1.099), (0.325, -0.266, 1.067), 0.043),
    ]):
        soft_lock(f'Cream hair lock {i + 1}', [a, b, c, d], width, HAIR, head, 0.58)
    eyes = [eye_panel('L', -1, head), eye_panel('R', 1, head)]
    for s in (-1, 1):
        brow = []
        for t in np.linspace(0, 1, 19):
            x = s * (0.089 + t * 0.169)
            z = 1.268 + 0.033 * math.sin(t * math.pi) + 0.012 * t
            brow.append((x, skin_depth(x, z) - 0.012, z))
        tube(f'Faint eyebrow {s}', brow, [0.003 + 0.006 * math.sin(math.pi * t) for t in np.linspace(0, 1, len(brow))], BROW, head, 0.55, 9)
    round_shape('Nose button', (0, -0.415, 1.027), (0.023, 0.017, 0.015), NOSE, head)
    mouth = node('Mouth', (0, -0.391, 0.942), head)
    smile = []
    for x in np.linspace(-0.054, 0.054, 29):
        z = 0.941 + 0.019 * (x / 0.054) ** 2
        smile.append((x, skin_depth(x, z) - 0.006, z))
    obj = tube('Tiny smiling mouth', smile, [0.002 + 0.007 * math.sin(math.pi * t) for t in np.linspace(0, 1, len(smile))], MOUTH, mouth, 0.46)
    obj.shape_key_add(name='Basis')
    expression = obj.shape_key_add(name='Smile')
    expression.value = 0.0
    for coord in expression.data:
        coord.co.x *= 1.19
        coord.co.z += 0.014 * (abs(coord.co.x) / 0.064) ** 1.4
    return eyes, ears


def costume(torso):
    round_shape('A-line short coat', (0, 0.018, 0.528), (0.251, 0.174, 0.246), FABRIC, torso, -0.45)
    round_shape('Warm collar roll', (0, -0.003, 0.732), (0.189, 0.152, 0.064), SCARF, torso)
    round_shape('Folded scarf triangle', (0.006, -0.145, 0.675), (0.154, 0.059, 0.083), SCARF, torso, 0.33)
    soft_lock('Knotted scarf tail', [(0.122, -0.119, 0.701), (0.226, -0.137, 0.680),
              (0.218, -0.181, 0.604), (0.196, -0.200, 0.561)], 0.057, SCARF, torso)
    for s in (-1, 1):
        seam = []
        for z in np.linspace(0.318, 0.597, 34):
            x = s * (0.029 + 0.006 * math.sin((z - 0.318) / 0.279 * math.pi))
            height = (z - 0.528) / 0.246
            cross = (1 - 0.45 * height) * math.sqrt(1 - height**2)
            y = 0.018 - (0.174 / 0.251) * math.sqrt(max(0, (0.251 * cross) ** 2 - x**2))
            seam.append((x, y - 0.004, z))
        line(f'Double coat topstitch {s}', seam, 0.0044, STITCH, torso)
        button = (s * 0.073, -0.152, .516)
        round_shape(f'Velvet coat button {s}', button, (0.023, 0.013, 0.023), TRIM, torso)
    # An open crescent rather than a geometric pendant shared with any reference.
    arc = [Vector((0.037 + 0.034 * math.cos(t), -0.215, 0.655 + 0.034 * math.sin(t))) for t in np.linspace(0.32, 4.14, 42)]
    tube('Brass crescent clasp', arc, [0.0022 + 0.010 * math.sin(math.pi * t) ** 0.85 for t in np.linspace(0, 1, len(arc))], BRASS, torso, 0.45)


def figure():
    root = node('Root', (0, 0, 0))
    pelvis = node('Pelvis', (0, 0, 0.348), root)
    hips = node('PelvisMotion', (0, 0, 0.348), pelvis)
    torso = node('Torso', (0, 0, 0.516), hips)
    head = node('Head', (0, -0.004, 0.782), torso)
    eyes, ears = head_details(head)
    costume(torso)
    limbs = []
    for tag, s in (('L', -1), ('R', 1)):
        arm = node('Arm.' + tag, (s * 0.184, 0.009, 0.650), torso)
        hand = node('Hand.' + tag, (s * 0.300, -0.009, 0.476), arm)
        sleeve = round_shape('Puffy sleeve.' + tag, (s * 0.245, 0.001, 0.563), (0.094, 0.091, 0.134), FABRIC, arm)
        sleeve.rotation_euler.y = s * 0.35
        round_shape('Cuff.' + tag, (s * 0.302, -0.003, 0.477), (0.086, 0.084, 0.059), TRIM, arm)
        round_shape('Mitten.' + tag, (s * 0.316, -0.004, 0.424), (0.071, 0.065, 0.081), HOOD, hand)
        round_shape('Thumb.' + tag, (s * 0.271, -0.051, 0.435), (0.037, 0.038, 0.047), HOOD, hand)
        leg = node('Leg.' + tag, (s * 0.115, 0.030, 0.365), hips)
        boot = node('Boot.' + tag, (s * 0.120, 0.040, 0.136), leg)
        round_shape('Twill leg.' + tag, (s * 0.118, 0.027, 0.254), (0.087, 0.095, 0.133), FABRIC, leg)
        round_shape('Plush boot collar.' + tag, (s * 0.120, 0.041, 0.174), (0.104, 0.105, 0.056), TRIM, boot)
        round_shape('Rounded boot.' + tag, (s * 0.122, -0.008, 0.093), (0.112, 0.159, 0.093), HOOD, boot)
        round_shape('Boot sole.' + tag, (s * 0.122, -0.020, 0.028), (0.113, 0.158, 0.028), SOLE, boot)
        limbs.append((arm, hand, leg, boot))
    return root, hips, torso, head, eyes, ears, limbs


def key(track, obj, index, rotation=(0, 0, 0), location=None, scale=None):
    obj.rotation_euler = rotation
    obj.keyframe_insert(data_path='rotation_euler', frame=index)
    if location is not None:
        obj.location = location
        obj.keyframe_insert(data_path='location', frame=index)
    if scale is not None:
        obj.scale = scale
        obj.keyframe_insert(data_path='scale', frame=index)
    track.add(obj)


def animate(figure_nodes, secondary=None, node_blink=True):
    _, hips, torso, head, eyes, ears, limbs = figure_nodes
    hips_start = hips.location.copy()
    rest = {obj: (obj.location.copy(), obj.rotation_euler.copy(), obj.scale.copy())
            for obj in bpy.context.scene.objects if obj.type == 'EMPTY'}
    clips = [('Idle', 81, 0), ('Walk', 29, 1), ('Run', 20, 2), ('Start', 12, 1),
             ('Stop', 13, 1), ('Turn_Left', 15, 1), ('Turn_Right', 15, 1)]
    for clip_index, (name, length, gait) in enumerate(clips, start=1):
        animated = set()
        for frame in range(1, length + 1):
            t = (frame - 1) / (length - 1)
            cycle, envelope, direction = t * TAU, 1.0, 0.0
            eased = t*t*(3 - 2*t)
            if name == 'Start':
                cycle, envelope = math.pi * (t - 1), eased
            elif name == 'Stop':
                cycle, envelope = math.pi * t, 1 - eased
            elif name.startswith('Turn_'):
                envelope = 0.56 * math.sin(math.pi * t)
                direction = 1.0 if name.endswith('Left') else -1.0
            turn = direction * math.sin(math.pi * t)
            bounce = (1 - math.cos(cycle * 2)) * (0.004, 0.009, 0.025)[gait] * envelope
            key(animated, hips, frame, (0.0, (0.023 * math.sin(cycle) + turn * 0.054) * envelope, 0.0), hips_start + Vector((0, 0, bounce)))
            key(animated, torso, frame, (0.047 * gait * envelope, 0.037 * math.sin(cycle) * envelope,
                                        0.05 * gait * math.cos(cycle) * envelope + 0.22 * turn))
            key(animated, head, frame, (0.021 * math.sin(cycle * 2) * envelope, 0.058 * math.sin(cycle) * envelope,
                                       0.022 * math.cos(cycle) * envelope + 0.18 * turn))
            blink = max(0.065, 1 - 0.94 * math.exp(-((t - 0.68) / 0.032) ** 2)) if node_blink and name == 'Idle' else 1
            for side, (eye, ear, (arm, hand, leg, boot)) in enumerate(zip(eyes, ears, limbs)):
                p = cycle + math.pi * side
                sigma = 2 * side - 1
                swing = math.cos(p)
                key(animated, eye, frame, scale=(1, 1, blink))
                key(animated, ear, frame, (0.027 * math.sin(p * 2) * envelope, 0.027 * math.cos(p) * envelope, 0.055 * math.sin(p) * envelope))
                key(animated, arm, frame, (-(0.028, 0.31, 0.60)[gait] * swing * envelope, 0.025 * sigma, 0.10 * turn))
                key(animated, hand, frame, (gait * 0.08 * math.sin(p) * envelope, 0, 0))
                key(animated, leg, frame, ((0.0, 0.37, 0.68)[gait] * swing * envelope, 0.015 * sigma, 0.19 * turn))
                key(animated, boot, frame, ((-gait * 0.16 * math.sin(p) - gait * 0.09 * swing) * envelope, 0, 0))
            if secondary is not None:
                secondary(animated, frame, cycle, envelope, gait, turn)
        for obj in sorted(animated, key=lambda item: item.name):
            data = obj.animation_data
            action = data.action
            action.name = f'{name} · {obj.name}'
            track = data.nla_tracks.new()
            track.name = name
            track.strips.new(name, 100 * clip_index + 1, action).extrapolation = 'NOTHING'
            data.action = None
    bpy.context.scene.frame_set(1)
    for obj, (location, rotation, scale) in rest.items():
        obj.location, obj.rotation_euler, obj.scale = location, rotation, scale
    bpy.context.scene.render.fps = 30


def light(name, position, energy, color, size):
    obj = bpy.data.objects.new(name, bpy.data.lights.new(name, 'AREA'))
    bpy.context.collection.objects.link(obj)
    obj.location = position
    obj.rotation_euler = (Vector((0, 0, 0.9)) - obj.location).to_track_quat('-Z', 'Y').to_euler()
    obj.data.energy, obj.data.color, obj.data.shape, obj.data.size = energy, color, 'DISK', size


def render(root, output, *, portrait=False):
    scene = bpy.context.scene
    ground = material('Proof backdrop', 'a69ab6', 0.87)
    bpy.ops.mesh.primitive_plane_add(size=200)
    bpy.context.object.data.materials.append(ground)
    bpy.context.object.location.z = -0.001
    light('Soft box', (-3.2, -4.0, 5.1), 350, (1.0, 0.82, 0.73), 4.0)
    light('Violet fill', (3.0, -2.3, 2.6), 207, (0.78, 0.84, 1.0), 3.4)
    light('Back edge', (1.1, 1.6, 3.2), 306, (1.0, 0.79, 0.87), 2.5)
    scene.world = bpy.data.worlds.new('Warm grey studio')
    scene.world.use_nodes = True
    scene.world.node_tree.nodes['Background'].inputs[0].default_value = (0.27, 0.24, 0.30, 1)
    scene.world.node_tree.nodes['Background'].inputs[1].default_value = 0.44
    bpy.ops.object.camera_add(location=(.68, -5.8, 1.76) if portrait else (2.04, -5.8, 2.21))
    camera = bpy.context.object
    camera.rotation_euler = (Vector((0.0, 0, 1.195 if portrait else 0.884)) - camera.location).to_track_quat('-Z', 'Y').to_euler()
    camera.data.type = 'ORTHO'
    camera.data.ortho_scale = 1.02 if portrait else 2.08
    scene.camera = camera
    scene.render.engine = 'CYCLES'
    scene.cycles.samples = 16
    scene.cycles.use_denoising = True
    scene.render.resolution_x = 916 if portrait else 690
    scene.render.resolution_y = 916 if portrait else 840
    scene.render.resolution_percentage = 100
    scene.view_settings.view_transform = 'AgX'
    scene.render.filepath = str(output)
    bpy.ops.render.render(write_still=True)
    for obj in [obj for obj in scene.objects if obj.parent is None and obj is not root]:
        bpy.data.objects.remove(obj, do_unlink=True)


if __name__ == '__main__':
    textures()
    actor = figure()
    if '--render' in sys.argv:
        render(actor[0], HERE / 'lumi-preview.png')
    animate(actor)
    bpy.context.scene.frame_set(1)
    bpy.ops.export_scene.gltf(filepath=str(HERE / 'lumi.glb'), export_format='GLB',
                             export_animation_mode='NLA_TRACKS', export_apply=False,
                             export_extras=False, export_morph=True, export_morph_normal=False,
                             export_anim_slide_to_zero=True)
