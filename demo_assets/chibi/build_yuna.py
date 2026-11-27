"""Build the original Yuna moth courier and her in-engine GLB animation rig."""

import math
import sys
from pathlib import Path

import bpy
from mathutils import Vector
import numpy as np

HERE = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
sys.path.insert(0, str(HERE))
import build_lumi as base
from build_lumi import mesh, round_shape, node, material, tube, soft_lock, line, grid_faces, TAU


HOOD = material('Yuna | Hood', '4d7282', 0.84)
CLOTH = material('Yuna | Coat', '55758f', 0.77)
PANELS = material('Yuna | Panel velvet', '244653', 0.89)
TRIM = material('Yuna | Fleece', 'f7ddad', 0.97)
FACING = material('Yuna | Hood lining', '326467', 0.91)
BRASS = material('Yuna | Brushed brass', 'e7b369', 0.38, 0.47)
CREAM = material('Yuna | Stitching', 'f0c58a', 0.85)
CAPELIGHT = material('Yuna | Cape markings', 'dcac7b', 0.91)
HAIR = material('Yuna | Hair', '806ea7', 0.51)
STRANDS = material('Yuna | Hair glints', 'a993c7', 0.54)
SKIN = material('Yuna | Skin', 'f9c5ad', 0.78)
SCLERA = material('Yuna | Sclera', 'fff4e9', 0.52)
IRIS = material('Yuna | Irises', 'c78646', 0.48)
LASH = material('Yuna | Lashes', '4e3349', 0.80)
BROW = material('Yuna | Brows', '8e5d75', 0.80)
BLUSH = material('Yuna | Blush', 'eb9a86', 0.84)
MOUTH = material('Yuna | Smile', '8c485f', 0.77)
TONGUE = material('Yuna | Lip shine', 'e9999d', 0.7)
TEETH = material('Yuna | Teeth', 'fff3dd', 0.61)
SOLE = material('Yuna | Boot leather', '273d57', 0.69)


def normal_map(mat, name, height, strength):
    dy, dx = np.gradient(height)
    n = np.stack((-dx * strength, -dy * strength, np.ones_like(dx)), axis=2)
    n = n / np.linalg.norm(n, axis=2, keepdims=True) * 0.5 + 0.5
    tex = bpy.data.images.new(name, width=n.shape[1], height=n.shape[0], alpha=False)
    tex.colorspace_settings.name = 'Non-Color'
    pixels = np.concatenate((n, np.ones((*n.shape[:2], 1))), axis=2)
    tex.pixels.foreach_set(pixels.astype(np.float32).ravel())
    tex.pack()
    nodes = mat.node_tree.nodes
    sampler = nodes.new('ShaderNodeTexImage')
    sampler.image = tex
    mapping = nodes.new('ShaderNodeNormalMap')
    mapping.inputs['Strength'].default_value = 0.32
    mat.node_tree.links.new(sampler.outputs['Color'], mapping.inputs['Color'])
    mat.node_tree.links.new(mapping.outputs['Normal'], nodes.get('Principled BSDF').inputs['Normal'])


def paint():
    x, y = base.uv_grid(640)
    rng = np.random.default_rng(415)
    diagonal = np.cos(x * TAU * 111 + y * TAU * 96)
    yarn = np.cos(y * TAU * 157 + .83 * diagonal) * np.sin(x * TAU * 141)
    nap = 0.010 * rng.standard_normal(x.shape) + .008 * diagonal + .011 * yarn
    for mat, color, name in [
        (HOOD, (0.25, 0.46, 0.52), 'moth plush'),
        (CLOTH, (0.251, 0.416, 0.485), 'brushed teal twill'),
        (TRIM, (0.867, 0.779, 0.653), 'fleece trim'),
    ]:
        mottling = 0.019 * np.cos(x * TAU * 3) * np.sin(y * TAU * 4) + 0.101 * y
        mottling -= 0.035 * np.cos(x * TAU * 11 + .23*np.sin(y*TAU))**6 * (1 - y) ** 1.4
        color = np.array(color) + (nap + mottling)[..., None]
        base.painted_map(mat, 'Yuna | ' + name, color)
        normal_map(mat, 'Yuna | ' + name + ' normal', .14*diagonal + .18*yarn + .065*rng.standard_normal(x.shape), .75)

    x, y = base.uv_grid(512)
    chop = .013*np.sin(x*TAU*24 + y*TAU*.76) + .008*np.sin(x*TAU*47 - y*TAU*1.2)
    silk = (.095 * np.exp(-((x-.25)/.14)**2) + .082 * np.exp(-((x-.71)/.20)**2)) * np.sin(math.pi*y)**1.55
    roots = np.array([.420,.335,.515])
    ends = np.array([.728,.626,.824])
    pigment = (1-y[..., None])*roots + y[..., None]*ends + (chop + silk)[..., None]
    base.painted_map(HAIR, 'Yuna | hand-painted strand gradient', pigment)

    x, y = base.uv_grid(640)

    skin = np.broadcast_to([0.977, 0.810, 0.733], (*x.shape, 3)).copy()
    blush = sum(np.exp(-((x - u) / 0.037)**2 - ((y - 0.434) / 0.056)**2) for u in (0.384, 0.616))
    nose = np.exp(-((x - 0.5) / 0.031)**2 - ((y - 0.426) / 0.033)**2)
    skin += blush[..., None] * [0.017, -0.190, -0.161]
    skin += nose[..., None] * [0.018, -0.091, -0.081]
    for side in (-1, 1):
        for _ in range(18):
            u = 0.5 + side * rng.uniform(0.052, 0.108)
            v = rng.uniform(0.408, 0.460)
            freckle = np.exp(-((x - u) / 0.0014)**2 - ((y - v) / 0.0017)**2)
            skin -= freckle[..., None] * [0.072, 0.092, 0.116]
    base.painted_map(SKIN, 'Yuna | rose and freckles', skin)

    x, y = base.uv_grid(384)
    x, y = 2 * x - 1, 2 * y - 1
    radius = np.sqrt(x*x + y*y)
    angle = np.arctan2(y, x)
    fiber = (0.027 * np.cos(83 * angle + radius * 18) + 0.020 * np.sin(61 * angle - radius * 7))
    fire = np.clip(0.45 - 0.57 * y, 0, 1) * np.clip((radius - 0.26) * 3.4, 0, 1)
    amber = np.array([0.39, 0.19, 0.23]) + (fire + fiber)[..., None] * [0.88, 0.80, 0.37]
    rim = np.clip((radius - 0.84) * 10, 0, 1)
    pupil = np.clip((0.50 - np.sqrt((x * 1.11)**2 + ((y - 0.07) * 0.91)**2)) * 20, 0, 1)
    amber = amber * (1 - rim[..., None]) + [0.29, 0.13, 0.19] * rim[..., None]
    amber = amber * (1 - pupil[..., None]) + [0.12, 0.068, 0.13] * pupil[..., None]
    cast = np.clip((y-.34) * 1.76, 0, .54)
    amber *= 1 - cast[..., None]
    for u, v, rx, ry in [(-0.32, 0.40, 0.150, 0.140), (0.37, 0.13, 0.068, 0.062),
                          (0.442, -0.42, 0.029, 0.050), (-0.27, -0.56, 0.054, 0.053)]:
        shine = np.clip((1 - ((x - u) / rx)**2 - ((y - v) / ry)**2) * 8, 0, 1)
        amber = amber * (1 - shine[..., None]) + [1, 0.975, 0.94] * shine[..., None]
    base.painted_map(IRIS, 'Yuna | firefly amber irises', amber)
    u,v = base.uv_grid(256)
    cast = np.clip((v-.51)*1.9,0,1)**1.64
    base.painted_map(SCLERA, 'Yuna | soft shadow of upper lid', np.array([.996,.957,.927])+cast[...,None]*[-.12,-.16,-.12])


def hood(head):
    points, uv, edge = [], [], []
    rings, sides = 31, 66
    for ri in range(rings):
        v = ri / (rings - 1)
        for si in range(sides + 1):
            u = si / sides
            angle = TAU * u
            opening = 1.123 + 0.10 * math.sin(angle) + 0.031 * math.cos(2 * angle)
            phi = (1 - v) * opening + v * (math.pi - 0.002)
            shape = math.sin(phi) * (1 + 0.025 * math.cos(angle * 3) * math.sin(math.pi * v))
            point = (0.479 * shape * math.cos(angle), 0.038 - 0.438 * math.cos(phi),
                     1.158 + 0.442 * shape * math.sin(angle))
            points.append(point)
            uv.append((u, v))
            if ri == 0:
                edge.append(Vector(point))
    mesh('Coif | brushed plush shell', points, grid_faces(sides + 1, rings, True), HOOD, head, uv)
    inset = []
    for p in edge:
        x, z = 0.900 * p.x, 1.16 + 0.93 * (p.z - 1.16)
        inset.append((x, base.skin_depth(x, z) + 0.021, z))
    mesh('Dark hood facing', [*edge, *inset], grid_faces(sides + 1, 2, True), FACING, head)
    line('Piped fleece hood trim', [p + Vector((0, -.012, 0)) for p in edge], 0.029, TRIM, head)
    line('Fine bronze hood piping', [p + Vector((p.x * .058, -.018, (p.z - 1.16) * .052)) for p in edge],
         0.006, CREAM, head)
    antennae = []
    for i, s in enumerate((-1, 1)):
        tip = 1.745 + i * .061
        lead = node(f'Antenna.{i}', (s * .229, .015, 1.535), head)
        branch = [(s*.216, .016, 1.490), (s*.223, .001, 1.635), (s*.349, -.071, tip+.049),
                  (s*.394, -.118, tip)]
        path = base.cubic(branch, 32)
        tube('Tapered moth antenna', path, [0.016 * (1 - 0.62 * t) for t in np.linspace(0, 1, len(path))],
             FACING, lead, 0.84, 12)
        soft_lock('Amber velvet antenna pennant', [(s*.341, -.079, tip+.031), (s*.356, -.116, tip+.041),
                    (s*.396, -.148, tip+.018), (s*.424, -.145, tip-.030)], .037, CAPELIGHT, lead, .68)
        antennae.append(lead)
    return antennae


def edit_shape(obj, target, change):
    if obj.data.shape_keys is None:
        obj.shape_key_add(name='Basis')
    pivot = Vector(obj.parent['world_pivot'])
    shape = obj.shape_key_add(name=target)
    shape.value = 0.0
    for point in shape.data:
        world = point.co + pivot
        point.co = Vector(change(*world)) - pivot


def eye_panel(tag, s, head):
    x0, z0 = s * .166, 1.139
    width = .136
    eye = node('Eye.' + tag, (x0, base.skin_depth(x0,z0),z0), head)
    blink = 'EyeBlinkLeft' if tag == 'L' else 'EyeBlinkRight'

    def slope(x):
        return z0 + .152 * s*(x-x0)

    def arch(x):
        return max(0., 1-((x-x0)/width)**2)**.69

    def surface(x,z):
        return base.skin_depth(x,z) - .0035 - .014 * arch(x) * max(0,1-((z-slope(x))/.121)**2)

    def shut(x,y,z,far):
        z = slope(x) - .017 - .011 * arch(x) + (z-slope(x))*.005
        return x, base.skin_depth(x,z)+far, z

    def closed_lash(x,y,z):
        lip = slope(x) + .104 * arch(x)
        thickness = (z - lip)*.79
        close = slope(x) - .027 - .016 * arch(x) + thickness
        depth = y - (surface(x,lip)-.009)
        return x, base.skin_depth(x,close)-.010+max(-.008,min(.006,depth*.30)), close

    def startled(x,y,z):
        dz = z - slope(x)
        next_z = slope(x) + dz * (1.23 if dz > 0 else 1.12)
        return x, y + base.skin_depth(x,next_z)-base.skin_depth(x,z), next_z

    cols, rows = 55, 20
    for iris in (False,True):
        points, uv = [], []
        for r in range(rows):
            radius = max(.0001, r/(rows-1))
            for c in range(cols):
                angle = TAU * c/(cols-1)
                dx, dy = math.cos(angle)*radius, math.sin(angle)*radius
                if iris:
                    x = x0 - s*.010 + dx*.074
                    z = slope(x) + .006 + .086*dy
                    z = min(slope(x)+.104*arch(x)-.001, max(slope(x)-.073*arch(x)+.001,z))
                else:
                    x = x0 + width*dx
                    z = slope(x) + math.copysign((.104 if dy > 0 else .073)*arch(x) * min(1,abs(dy)/max(.00001,math.sqrt(max(0,1-dx*dx)))), dy)
                points.append((x, surface(x,z)-(.0045 if iris else 0), z))
                uv.append((.5+dx*.5,.5+dy*.5))
        obj = mesh('Amber iris.'+tag if iris else 'Curved sclera.'+tag, points,
                   grid_faces(cols,rows,True), IRIS if iris else SCLERA, eye, uv)
        # The iris sinks behind skin during a blink. Only the lid edge remains visible.
        edit_shape(obj, blink, lambda x,y,z: shut(x,y,z,.013))
        if not iris:
            edit_shape(obj, 'Surprise', startled)

    cap = []
    for i in range(45):
        t = i/44
        x = x0 + width*math.cos(math.pi*t)
        z = slope(x)+.104*arch(x)
        cap.append((x,surface(x,z)-.009,z))
    lash = tube('Upper lashline.'+tag, cap, [.002+.009*math.sin(math.pi*t)**.71 for t in np.linspace(0,1,len(cap))],
                LASH, eye,.54,12)
    edit_shape(lash, blink, closed_lash)
    edit_shape(lash, 'Surprise', startled)
    # Tapered, swept lashes on the raised outer third.
    for n in range(3):
        x = x0 + s*(.092+.013*n)
        z = slope(x)+.104*arch(x)
        p = (x, surface(x,z)-.012,z)
        strand = soft_lock('Sculpted outer eyelashes.'+tag, [p,(x+s*.017,p[1]-.003,z+.006),
            (x+s*.026,p[1]+.007,z+.014+n*.002),(x+s*(.032+.002*n),p[1]+.019,z+.028+n*.002)], .0051,LASH,eye,.33)
        def swept_closed(x,y,z):
            lip = slope(x) + .104*arch(x)
            close = slope(x)-.026-.016*arch(x)+.34*(z-lip)
            return x,base.skin_depth(x,close)-.011,close
        edit_shape(strand, blink, swept_closed)
        edit_shape(strand, 'Surprise', startled)
    fold = []
    for i in range(33):
        t=i/32
        x=x0+width*.9*math.cos(t*math.pi)
        z=slope(x)+.104*arch(x)+.024*math.sin(t*math.pi)**.8
        fold.append((x,base.skin_depth(x,z)-.003,z))
    shade = tube('Lower edge of mobile lid.'+tag,fold,[.001+.0024*math.sin(math.pi*t) for t in np.linspace(0,1,len(fold))],BLUSH,eye,.49,8)
    edit_shape(shade, blink, lambda x,y,z: (x,base.skin_depth(x,z)+.006,z))
    edit_shape(shade, 'Surprise', startled)
    return eye


def lips(head):
    center_z = .951
    mouth = node('Mouth', (0, -.391, center_z), head)
    parameters = {
        'Basis': (.044,.0075,.010,.009,.001),
        'Smile': (.058,.012,.0125,.022,.003),
        'Joy': (.071,.022,.030,.026,.007),
        'Surprise': (.036,.032,.039,.001,-.004),
        'aa': (.053,.030,.035,.006,-.002),
        'ih': (.072,.014,.015,.010,.001),
        'ou': (.031,.021,.028,.004,-.001),
        'oh': (.040,.026,.033,.001,-.003),
    }
    samples = [(max(.0001,r/12),i*TAU/48) for r in range(13) for i in range(49)]

    def geometry(role, shape):
        width, up, down, corner, dz = parameters[shape]
        points = []
        for radius,angle in samples:
            x,z = radius*math.cos(angle), radius*math.sin(angle)
            actual_x = x*width
            arc = (abs(actual_x)/width)**2.5 * corner
            actual_z = center_z+dz+z*(up if z>0 else down)+arc
            push = -0.007
            if role != 'hollow':
                actual_x *= 0.61 if role == 'teeth' else 0.57
                actual_z = center_z+dz+up*.61+(up*.25)*z if role == 'teeth' else center_z+dz-down*.71+(down*.21)*z
                push = -0.011
                if shape == 'Basis' or role == 'teeth' and shape in ('ou','oh','Surprise'):
                    push = .018
            points.append((actual_x,base.skin_depth(actual_x,actual_z)+push,actual_z))
        return points

    for role, mat in [('hollow',MOUTH),('teeth',TEETH),('tongue',TONGUE)]:
        obj = mesh('Mouth | '+role, geometry(role,'Basis'), grid_faces(49,13,True),mat,mouth)
        pivot = Vector(mouth['world_pivot'])
        obj.shape_key_add(name='Basis')
        for name in list(parameters)[1:]:
            shape = obj.shape_key_add(name=name, from_mix=False)
            shape.value = 0.0
            for point,target in zip(shape.data, geometry(role,name)):
                point.co = Vector(target)-pivot


def ribbon_lock(name, path, width, parent, depth=.29, mat=HAIR):
    curve = [Vector(p) for p in base.cubic(path,38)]
    radius = [max(.00075, width*(.78 + .35*math.sin(math.pi*t*1.16)) * (1-t)**.67)
              for t in np.linspace(0,1,len(curve))]
    points, faces, uv = base.tube_geometry(curve,radius,depth,20)
    cols = 21
    for i,p in enumerate(points):
        v = (i//cols)/(len(curve)-1)
        a = TAU * (i%cols)/(cols-1)
        ridge = .19 * math.cos(a*6+.7*v)*math.sin(a)**2*(1-v)**.48
        points[i] = p + (p - curve[i//cols])*ridge
    return mesh(name,points,faces,mat,parent,uv)


def face(head):
    round_shape('Face | cheek and jaw', (0, -0.056, 1.136), (.414, .365, .376), SKIN, head)
    ears = hood(head)
    eyes = [eye_panel('L', -1, head), eye_panel('R', 1, head)]
    for s in (-1, 1):
        path = []
        for t in np.linspace(0, 1, 20):
            x, z = s * (.089 + .164*t), 1.273 + .019*math.sin(math.pi*t) + t*.005
            path.append((x, base.skin_depth(x,z) - .013, z))
        brow = tube(f'Brow {s}', path, [.002 + .007*math.sin(math.pi*t) for t in np.linspace(0,1,len(path))],
                    BROW, head, .61, 10)
        edit_shape(brow,'Surprise',lambda x,y,z: (x,base.skin_depth(x,z+.027)-.014,z+.027))
        edit_shape(brow,'Joy',lambda x,y,z: (x,base.skin_depth(x,z+.007)-.014,z+.007))
    round_shape('Rose button nose', (0, -.405, 1.029), (.020, .017, .012), BLUSH, head)
    lips(head)

    hairline, coords = [], []
    rows, columns = 13, 33
    for ri in range(rows):
        v = ri / (rows - 1)
        for ci in range(columns):
            u = ci / (columns - 1)
            x = (u * 2 - 1) * (.354 - .258 * v * v)
            z = (1.411 - .052 * abs(u * 2 - 1)**2) * (1 - v) + 1.518 * v
            y = base.skin_depth(x, z) - .010 - .030 * math.sin(math.pi * v)
            hairline.append((x,y,z))
            coords.append((u,v))
    mesh('Root layer beneath swept fringe', hairline, grid_faces(columns, rows), HAIR, head, coords)

    hair = []
    specs = [(-.26,-.247,1.455,-.343,-.275,1.270,.047,-.281),
             (-.179,-.291,1.496,-.286,-.331,1.241,.050,-.374),
             (-.105,-.319,1.503,-.220,-.378,1.269,.062,-.414),
             (-.050,-.339,1.507,-.161,-.394,1.289,.042,-.430),
             (.038,-.315,1.510,-.081,-.418,1.258,.071,-.428),
             (.112,-.289,1.511,.015,-.419,1.268,.055,-.414),
             (.165,-.284,1.503,.131,-.407,1.278,.047,-.398),
             (.226,-.253,1.484,.276,-.333,1.245,.058,-.361),
             (.280,-.239,1.449,.345,-.267,1.230,.042,-.327)]
    for i,(x,y,z,tx,ty,tz,w,arc) in enumerate(specs):
        sweep = -.052 if i < 6 else .041
        path = [(x,y,z), (x+sweep,arc,z+.014), (tx+sweep*1.19,ty-.016,(z+tz)*.5-.051), (tx,ty,tz)]
        ribbon_lock(f'Overlapping anisotropic crown lock {i}', path,w,head)
        if i in (0,2,4,6,8):
            split = [(x+.020,y+.019,z-.008),(x+sweep-.019,arc-.006,z-.027),
                     (tx+sweep*1.24-.041,ty-.021,(z+tz)*.49-.050),(tx-.040,ty+.006,tz+.012)]
            ribbon_lock(f'Secondary tapered fringe {i}',split,w*.36,head,.24,STRANDS)
    for i,s in enumerate((-1,1)):
        side = node(f'Hair.{i}', (s*.337, -.205, 1.293), head)
        for order,(offset,length,width) in enumerate([(.054,.17,.049),(.035,.29,.060),(.012,.35,.056),(-.013,.41,.053),(-.041,.453,.033)]):
            path = [(s*(.305+offset),-.213,1.360-.009*order), (s*(.445+offset),-.232,1.25),
                    (s*(.442+offset),-.309,1.21-length*.57),(s*(.334+offset),-.337,1.32-length)]
            ribbon_lock(f'Layered soft side curl {order}',path,width,side,.46)
        ribbon_lock('Long face framing flyaway', [(s*.358,-.288,1.284),(s*.388,-.345,1.158),
            (s*.318,-.371,1.027),(s*.357,-.331,.962)],.0125,side,.46,STRANDS)
        hair.append(side)
    # The rolled cloth pin anchors the asymmetric fringe.
    for angle in (.38,1.21):
        clasp = [( .243 + .043*t*math.cos(angle),-.357,1.310+.043*t*math.sin(angle)) for t in np.linspace(-1,1,17)]
        line('Fine brushed bronze bobby pin',clasp,.0038,BRASS,head)
    round_shape('Opal accent at bobby pins',(.242,-.360,1.309),(.012,.009,.010),TRIM,head)
    return eyes, ears, hair


def vest(torso):
    # A single pleated coat surface keeps the costume continuous around the hips.
    rings = [(.304,.257,.181),(.327,.270,.187),(.361,.263,.181),(.397,.255,.175),(.438,.245,.166),(.464,.235,.161),
             (.525,.208,.147),(.587,.191,.144),(.657,.200,.154),(.704,.184,.137),(.721,.147,.112)]
    points, uv = [], []
    slices = 72
    for ri, (z, rx, ry) in enumerate(rings):
        fullness = (1 - ri / (len(rings)-1))**1.4
        for si in range(slices + 1):
            a = TAU*si/slices
            pleat = 1 + .044 * fullness * math.cos(a*10 + ri*.29)
            points.append((rx * math.sin(a)*pleat, .027 - ry * math.cos(a)*pleat,
                           z + .009 * fullness * math.sin(a*5)))
            uv.append((si/slices, ri/(len(rings)-1)))
    mesh('Continuous pleated courier coat', points, grid_faces(slices+1, len(rings)), CLOTH, torso, uv)
    line('A-line curved piping', points[:slices+1], .014, CREAM, torso)
    stitches, faces = [], []
    for si in range(0, slices, 2):
        a, b = Vector(points[si]), Vector(points[si+1])
        outset = Vector((a.x, a.y - .027, 0)).normalized() * .014
        thread = [a.lerp(b,t) + outset + Vector((0, 0, .026)) for t in (0,.25,.5)]
        pts, fs, _ = base.tube_geometry(thread, [.0022]*len(thread), rings=6)
        offset = len(stitches)
        stitches.extend(pts)
        faces.extend(tuple(vertex + offset for vertex in face) for face in fs)
    mesh('Blanket stitching along coat hem', stitches, faces, CREAM, torso)
    for s in (-1,1):
        branch = base.cubic([(s*.087,-.155,.376),(s*.114,-.152,.391),(s*.138,-.132,.411),(s*.163,-.104,.438)],24)
        line('Golden fern embroidery', branch, .0022, BRASS, torso)
        for i in range(3):
            p = Vector(branch[3+i*7])
            for dy in (-1.,1.):
                path = [p,p+Vector((s*.004,-.004,.006+dy*.006)),p+Vector((s*.008,-.004,.009+dy*.010)),
                        p+Vector((s*.020,-.003,.004+dy*.012))]
                soft_lock('Sprig silk stitch',path,.0035,CREAM,torso,.41)
    round_shape('Rolled turtle collar', (0,-.008,.724), (.175,.142,.069), TRIM, torso)
    round_shape('Neck', (0,-.011,.782), (.093,.075,.079), SKIN, torso)
    path = [(0, -.128 - .040*t, .697-.348*t) for t in np.linspace(0,1,29)]
    line('Raised fabric placket', path, .011, PANELS, torso)
    round_shape('Courier brooch backing', (0,-.165,.672), (.043,.014,.038), BRASS, torso)
    round_shape('Courier brooch ivory face', (0,-.178,.672), (.028,.011,.025), TRIM, torso)
    for delta in [-.016, .016]:
        soft_lock('Inlaid tiny golden wing', [(.0,-.190,.667), (delta,-.193,.680),
            (1.25*delta,-.193,.684), (1.45*delta,-.190,.689)], .009, BRASS, torso, .53)
    for s in (-1, 1):
        path = base.cubic([(s*.062,-.137,.522), (s*.125,-.172,.511),
                           (s*.154,-.134,.468), (s*.172,-.100,.459)])
        tube(f'V-shaped pocket {s}', path, [.008]*len(path), CREAM, torso, .62, 10)
        round_shape('Embossed coat button', (s*.062,-.155,.570), (.017,.013,.017), BRASS, torso)


def wings(torso):
    pieces = []
    for i,s in enumerate((-1,1)):
        joint = node(f'Wing.{i}', (s*.164,.121,.688), torso)
        # Folded twin sails, scalloped hem and stitched eyespots are unique to this jacket.
        rows, columns = 28, 31
        points, uv = [], []
        for ri in range(rows):
            t = ri/(rows-1)
            for ci in range(columns):
                u = ci/(columns-1)
                fullness = math.sin(math.pi * t)**.7
                x = s * (.135 + .048*t + u * (.017 + .223 * fullness))
                y = .132 + .15*t + .073*math.sin(u*math.pi) * fullness + .037*math.sin(u * 3.1 * math.pi)*fullness
                z = .698 - .349*t - .051*u*fullness - .013*math.sin(u*8*math.pi)*t
                points.append((x,y,z)); uv.append((u,t))
        cloth = mesh('Moth caplet | sculpted fold', points, grid_faces(columns, rows, i == 0), PANELS, joint, uv)
        cloth.data.materials[0].use_backface_culling = False
        line('Gold caplet hem', points[-columns:], .0085, CREAM, joint)
        edge = [points[index*columns+columns-1] for index in range(rows)]
        line('Gold caplet outer edge', edge, .007, CREAM, joint)
        round_shape('Embroidered eyespot', (s*.305,.295,.516), (.039,.012,.036), CAPELIGHT, joint)
        round_shape('Raised detail on eyespot', (s*.31,.305,.516), (.014,.009,.015), TRIM, joint)
        pieces.append(joint)
    return pieces


def character():
    root = node('Root', (0,0,0))
    pelvis = node('Pelvis', (0,0,.348), root)
    hips = node('PelvisMotion', (0,0,.348), pelvis)
    torso = node('Torso', (0,0,.516), hips)
    head = node('Head', (0,-.004,.782), torso)
    eyes, ears, hair = face(head)
    vest(torso)
    wings_panels = wings(torso)
    limbs = []
    for tag,s in [('L',-1),('R',1)]:
        arm = node(f'Arm.{tag}', (s*.179,.015,.665), torso)
        hand = node(f'Hand.{tag}', (s*.299,.002,.469), arm)
        sweep = base.cubic([(s*.168,.033,.710),(s*.231,.026,.672),(s*.256,-.006,.567),(s*.295,.001,.473)],35)
        sleeve_profile = [.046 + .057*math.sin(math.pi*t)**.58 + .007*t
                          + .0025*math.sin(5*math.pi*t) for t in np.linspace(0,1,len(sweep))]
        tube(f'Tailored folded sleeve {tag}',sweep,sleeve_profile,HOOD,arm,.79,24)
        cuff_path = base.cubic([(s*.287,-.001,.509),(s*.292,-.002,.489),
                                  (s*.294,-.001,.477),(s*.302,-.004,.456)],18)
        cuff = [.052 + .011*math.sin(math.pi*t)**.40 for t in np.linspace(0,1,len(cuff_path))]
        tube(f'Ribbed turned sleeve cuff {tag}',cuff_path,cuff,TRIM,arm,1.06,24)
        for t in (4,12):
            p = Vector(cuff_path[t])
            trace = [(p.x+.054*math.cos(angle),p.y+.057*math.sin(angle),p.z+.025*s*math.cos(angle))
                     for angle in np.linspace(0,TAU,34)]
            line('Cuff embroidery ring',trace,.0021,CREAM,arm)
        round_shape(f'Mitten {tag}', (s*.317,.009,.415), (.062,.066,.075), FACING, hand)
        round_shape(f'Thumb {tag}', (s*.274,-.032,.426), (.031,.037,.045), FACING, hand)
        for n,x in enumerate((.306,.328)):
            light = [(s*x,-.052,.434),(s*(x+.005),-.057,.426),(s*(x+.007),-.057,.421),(s*(x+.008),-.049,.411)]
            ribbon_lock(f'Sewn motif on mitten {n}',light,.0049,hand,.36,CREAM)
        leg = node(f'Leg.{tag}', (s*.114,.027,.370), hips)
        boot = node(f'Boot.{tag}', (s*.118,.036,.131), leg)
        round_shape(f'Tapered trousers {tag}', (s*.114,.021,.252), (.084,.091,.137), PANELS, leg, -.22)
        round_shape(f'Rolled boot cuff {tag}', (s*.117,.022,.174), (.094,.107,.051), TRIM, boot)
        round_shape(f'Rounded soft boot {tag}', (s*.118,-.027,.088), (.101,.147,.087), HOOD, boot)
        round_shape(f'Sturdy sole {tag}', (s*.118,-.035,.024), (.104,.147,.024), SOLE, boot)
        round_shape(f'Dull-gold buckle {tag}', (s*.116,-.142,.108), (.034,.023,.022), BRASS, boot)
        limbs.append((arm, hand, leg, boot))

    def secondary(animated, frame, cycle, envelope, gait, turn):
        for i,s in enumerate((-1,1)):
            phase = cycle + .80*i
            base.key(animated, hair[i], frame, (.018*math.cos(phase)*envelope,
                (.024 + .028*gait)*math.sin(phase-.8)*envelope, turn*.075))
            base.key(animated, wings_panels[i], frame, (.014*math.sin(phase)*envelope,
                s*.025*math.cos(phase)*envelope, s*(.035 + .055*gait)*math.sin(phase-.6)*envelope))
    return (root, hips, torso, head, eyes, ears, limbs), secondary


if __name__ == '__main__':
    paint()
    rig, secondary = character()
    if '--render' in sys.argv:
        base.render(rig[0], HERE / 'yuna-preview.png')
        base.render(rig[0], HERE / 'yuna-portrait.png', portrait=True)
    base.animate(rig, secondary, node_blink=False)
    bpy.context.scene.frame_set(1)
    bpy.ops.export_scene.gltf(filepath=str(HERE / 'yuna.glb'), export_format='GLB',
                             export_animation_mode='NLA_TRACKS', export_apply=False,
                             export_extras=False, export_morph=True, export_morph_normal=False,
                             export_anim_slide_to_zero=True)
