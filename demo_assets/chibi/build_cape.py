"""Author the optional travel cape in the Lumi torso's local coordinates."""
from math import cos, pi, sin, tau
from pathlib import Path
from runpy import run_path

folder = Path(__file__).resolve().parent
Mesh = run_path(str(folder.parent / "rain" / "build.py"))["Mesh"]
mesh = Mesh()
upper = mesh.material("Seafoam cotton", (.21, .57, .53), .77, double_sided=True)
lining = mesh.material("Cape edging", (.79, .92, .82), .76, double_sided=True)
metal = mesh.material("Crescent clasp", (.91, .58, .21), .40, .38)


def ring(angle, factor):
    wave = cos(7 * angle) * .011 * factor
    x = (0.151 + .168 * factor + wave) * sin(angle)
    y = .175 - .225 * factor + .027 * cos(angle) * factor
    z = (0.148 + .094 * factor + wave) * cos(angle)
    return x, y, z


steps = 56
angles = [.24 + (tau - .48) * j / steps for j in range(steps + 1)]
for a, b in zip(angles, angles[1:]):
    mesh.face([ring(a, 0), ring(b, 0), ring(b, .46), ring(a, .46)], upper)
    mesh.face([ring(a, .46), ring(b, .46), ring(b, .92), ring(a, .92)], upper)
    mesh.face([ring(a, .92), ring(b, .92), ring(b, 1), ring(a, 1)], lining)
for angle in [angles[0], angles[-1]]:
    for k in range(7):
        p = k / 7
        q = (k + 1) / 7
        side = .035 if angle < 1 else -.035
        mesh.face([ring(angle, p), ring(angle + side, p), ring(angle + side, q),
                   ring(angle, q)], lining)
# Small warm accent at the clasp, kept clear of the chin.
center = (.070, .125, .178)
for i in range(11):
    def point(t, inner):
        angle = -.68 * pi + 1.52 * pi * t
        radius = .018 if inner else .029
        return (center[0] + radius * cos(angle), center[1] + radius * sin(angle), center[2])
    mesh.face([point(i / 11, False), point((i + 1) / 11, False),
               point((i + 1) / 11, True), point(i / 11, True)], metal)
mesh.save("cape", folder)
