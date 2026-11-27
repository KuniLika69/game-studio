"""Build original, recolorable Storybook Workshop GLB templates."""
import math
from functools import lru_cache
from pathlib import Path
import random
import runpy
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent
BASE = runpy.run_path(str(OUT.parent / 'rain' / 'build.py'))
Mesh = BASE['Mesh']
TAU = math.tau
PI = math.pi


def add(a, b): return tuple(x + y for x, y in zip(a, b))
def sub(a, b): return tuple(x - y for x, y in zip(a, b))
def mul(a, s): return tuple(x * s for x in a)
def cross(a, b): return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])
def unit(a): return mul(a, 1.0 / max(1.0e-9, math.sqrt(sum(x*x for x in a))))


def axis_coords(p, t):
    x, y, z = p
    return (x*math.cos(t) + z*math.sin(t), y, z*math.cos(t) - x*math.sin(t))


def ellipsoid(m, center, extent, material, segments=16, rings=10, warp=0., yaw=0., power=1., salt=0., basis=None):
    def orient(value):
        p = axis_coords(value, yaw)
        return p if basis is None else tuple(sum(basis[j][i]*p[j] for j in range(3)) for i in range(3))
    def point(row, col):
        lat = -PI*.5 + PI*row/rings
        long = TAU*col/segments
        normal = (math.cos(lat)*math.cos(long), math.sin(lat), math.cos(lat)*math.sin(long))
        wave = 1 + warp*(.54*math.cos(long*3+lat*1.7+salt) + .30*math.sin(long*7-lat*4+salt) + .16*math.sin(lat*11+long*2)) * max(0., math.cos(lat))**.5
        p = tuple(math.copysign(abs(v)**power, v)*a*wave for v, a in zip(normal, extent))
        n = unit(tuple(math.copysign(abs(v)**(2-power), v)/a for v, a in zip(normal, extent)))
        return add(center, orient(p)), orient(n), (col/segments*2., row/rings)
    for row in range(rings):
        for col in range(segments):
            pts = [point(row,col), point(row+1,col), point(row+1,col+1), point(row,col+1)]
            if row == 0: pts.pop(3)
            if row == rings - 1: pts.pop(2)
            m.face([p[0] for p in pts], material, [p[2] for p in pts], [p[1] for p in pts])


def branch(m, chain, material, steps=12):
    loops = []
    for i, (p, radius) in enumerate(chain):
        tangent = unit(sub(chain[min(i+1,len(chain)-1)][0], chain[max(0,i-1)][0]))
        ref = (0.,0.,1.) if abs(tangent[0]) + abs(tangent[1]) > .45 else (0.,1.,0.)
        u = unit(cross(tangent,ref)); v = unit(cross(u,tangent))
        loop=[]
        for j in range(steps+1):
            t = j*TAU/steps
            direction = add(mul(u, math.cos(t)), mul(v, math.sin(t)))
            loop.append((add(p,mul(direction,radius*(1+.09*math.sin(t*3+i)))), direction, (j/steps, i/3)))
        loops.append(loop)
    for i in range(len(loops)-1):
        for j in range(steps):
            pts=[loops[i][j],loops[i+1][j],loops[i+1][j+1],loops[i][j+1]]
            m.face([p[0] for p in pts], material, [p[2] for p in pts], [p[1] for p in pts])


def tube_between(m,a,b,radius,material,steps=9):
    branch(m,[(a, radius*.92),(add(a,mul(sub(b,a),.12)),radius),(add(a,mul(sub(b,a),.91)),radius),(b,radius*.78)],material,steps)


@lru_cache
def relief(kind):
    n = 128
    hp=Image.new('L',(n,n)); pix=hp.load()
    pigment=Image.new('RGB',(n,n)); p=pigment.load()
    rng=random.Random({'chalk':21,'wood':31,'stone':41}[kind]); phases=[rng.random()*TAU for _ in range(7)]
    for y in range(n):
        for x in range(n):
            u,v=x/n*TAU,y/n*TAU
            field=math.sin(u*2+v*3+phases[0])*math.cos(v*3-u+phases[1])
            if kind=='wood':
                drift=math.sin(v*2+math.sin(u)+phases[2])*1.1
                waves=(math.sin(u*8+drift)+.5*math.sin(u*15+drift*.9)+.28*math.sin(u*23+drift*.6))
                value=150+22*waves+12*field; tint=215+17*waves+8*field
            elif kind=='stone':
                value=145+34*field+19*math.sin(u*4-v*3)*math.sin(v*5+u*4)
                tint=220+16*field-7*math.cos(u*7+v*2)
            else:
                value=167+7*field+3*math.sin(u*7+v*6)
                tint=237+6*field+4*math.cos(v*4)
            pix[x,y]=int(min(255,max(0,value)))
            p[x,y]=(int(tint),int(tint),int(tint))
    return BASE['mapped'](n,hp,pigment,.63 if kind=='wood' else .78,620)


def material(m, name, color, surface='chalk', rough=.71, emission=None):
    return m.material(name, color, rough, texture=relief(surface) if surface else None,
                      emissive=emission, tile_size=.78)


def blossom(m, center, radius, petals, gold, twist=0., direction=None):
    basis = None
    if direction is not None:
        up = unit(direction)
        right = unit(cross(up,(0.,0.,1.))) if abs(up[2]) < .9 else unit(cross(up,(0.,1.,0.)))
        fore = cross(right,up)
        basis = (right, up, fore)
    def move(p):
        return add(center,p if basis is None else tuple(sum(basis[j][i]*p[j] for j in range(3)) for i in range(3)))
    for i in range(5):
        t=i*TAU/5+twist
        p=move((math.cos(t)*radius*.53, radius*.018, math.sin(t)*radius*.53))
        ellipsoid(m,p,(radius*.49,radius*.16,radius*.31),petals,8,5,yaw=-t,basis=basis)
    ellipsoid(m,move((0.,radius*.11,0.)),(radius*.17,radius*.14,radius*.17),gold,8,5,basis=basis)


def smile(m, y, z, distance, eyes, shine, cheek, mouth, size=1.):
    for side in [-1,1]:
        ellipsoid(m,(side*distance,y,z),(.048*size,.064*size,.018*size),eyes,16,10)
        ellipsoid(m,(side*distance-.015*size,y+.022*size,z+.014*size),(.015*size,.021*size,.009*size),shine,10,6)
        ellipsoid(m,(side*(distance+.038*size),y-.064*size,z-.002*size),(.052*size,.025*size,.018*size),cheek,12,7)
    branch(m, [((i*size*.017, y+(-.060+.005*i*i)*size, z+.018*size),.011*size) for i in range(-3,4)],mouth,9)


def arch(m, center, radius, y_bottom, height, axis, face_mat, edge_mat):
    x,y,z=center
    shape=[(-radius,y_bottom),(radius,y_bottom)] + [(radius*math.cos(j*PI/16),y+height*math.sin(j*PI/16)) for j in range(17)]
    verts=[(x+a,b,z) if axis=='z' else (z,b,x+a) for a,b in shape]
    if axis=='x': verts.reverse()
    m.face(verts,face_mat)
    branch(m, [(p,.041) for p in verts+[verts[0]]],edge_mat,10)
    return verts


def cottage():
    m=Mesh(); rng=random.Random(1903)
    stucco=material(m,'Cream limewashed wall',(.92,.69,.45),'chalk')
    roof=[material(m,'Palette Main petal tile %d'%i,c,'chalk',.68) for i,c in enumerate([
        (.57,.21,.31),(.64,.24,.34),(.54,.18,.25),(.70,.28,.36),(.55,.21,.31)])]
    wood=material(m,'Honey-colored joinery',(.52,.29,.13),'wood')
    trim=material(m,'Palette Trim seafoam paint',(.14,.42,.40),'chalk')
    bright=material(m,'Goldsmith details',(.72,.42,.13),None,.41)
    slate=material(m,'Foundation pebbles',(.42,.38,.36),'stone')
    glass=material(m,'Amber glass',(.87,.57,.19),None,.46,(.32,.12,.02))
    glass_shine=material(m,'Glass reflection',(.96,.75,.43),None,.39,(.36,.17,.04))
    vine=material(m,'Palette Flora leaf',(.24,.43,.14),None)
    bloom=material(m,'Palette Main window flower',(.89,.26,.32),None)
    m.box((0,.15,0),(2.67,.31,2.18),slate,.105)
    m.box((0,1.17,0),(2.48,1.97,2.),stucco,.23)
    for z in [-1.044,1.044]:
        for i in range(9):
            x = (i-4)*.272
            ellipsoid(m,(x,.205,z),(.164,.128,.071),slate,10,6,.06,salt=i)
        front=[(-1.28,1.85,z),(1.28,1.85,z),(1.16,2.08,z),(.00,2.69,z),(-1.16,2.08,z)]
        m.face(front if z>0 else front[::-1],stucco)
        tube_between(m,(-1.14,1.98,z),(0,2.64,z),.052,wood)
        tube_between(m,(0,2.64,z),(1.14,1.98,z),.052,wood)
    for side in [-1,1]:
        for i in range(10):
            t=i/10; xp=(t+.044)*1.44
            y=2.80-.75*t-.17*t*t
            for j in range(10):
                z=(j-4.5+(i%2)*.13)*.249
                # Shingles follow the descending X slope, leaving a soft uneven eave.
                a,b=side*(xp-.092),side*(xp+.101); za,zb=z-.123,z+.123
                pts=[(a,y+.048,za),(a,y+.048,zb),(b,y-.074,zb),(b,y-.074,za)]
                m.face(pts if side>0 else pts[::-1],rng.choice(roof))
                edge=[(b,y-.074,za),(b,y-.074,zb),(b,y-.116,zb),(b,y-.116,za)]
                m.face(edge if side>0 else edge[::-1],roof[2])
    for j in range(11):
        ellipsoid(m,(0,2.82,-1.19+j*.237),(.136,.110,.145),roof[j%len(roof)],14,8)
    m.box((.70,2.78,-.50),(.29,.90,.35),slate,.042)
    m.box((.70,3.25,-.50),(.43,.12,.45),slate,.038)
    ellipsoid(m,(.70,3.32,-.50),(.128,.038,.138),roof[2],12,7)
    arch(m,(-.32,1.12,1.043),.407,.30,.46,'z',trim,wood)
    for i in range(6):
        x=-.32+(i-2.5)*.116
        y=1.12+.40*math.sqrt(max(0.,1-((x+.32)/.407)**2))
        tube_between(m,(x,.34,1.055),(x,y,1.055),.014,trim)
    ellipsoid(m,(-.06,.879,1.106),(.040,.037,.046),bright,14,8)
    m.box((-.32,.131,1.25),(.93,.16,.67),slate,.071)
    m.box((-.32,.059,1.55),(1.00,.12,.27),slate,.035)
    for x,y,z in [(.64,1.30,1.045),(0.,2.25,1.054),(-.64,1.30,-1.05)]:
        r=.237 if y<2 else .139
        ellipsoid(m,(x,y,z),(r*1.18,r*1.18,.061),trim,22,13)
        ellipsoid(m,(x,y,z+(.051 if z>0 else -.051)),(r,r,.041),glass,22,13)
        yy=z+(.090 if z>0 else -.090)
        tube_between(m,(x-r,y,yy),(x+r,y,yy),.025,wood)
        tube_between(m,(x,y-r,yy),(x,y+r,yy),.024,wood)
        tube_between(m,(x-r*.60,y+r*.46,yy+.009),(x-r*.22,y+r*.76,yy+.009),.013,glass_shine)
    m.box((.64,.989,1.12),(.73,.17,.28),wood,.046)
    for j in range(10):
        p=(.36+rng.random()*.55,1.10+rng.random()*.069,1.21-rng.random()*.135)
        ellipsoid(m,p,(.092,.046,.082),vine,10,7,warp=.09)
        if j%2==0: blossom(m,add(p,(0,.052,.011)),.059,bloom,bright,j)
    m.save('cottage',OUT)


def tree():
    m=Mesh(); rng=random.Random(4617)
    bark=material(m,'Twisted apricot wood',(.49,.27,.124),'wood')
    line=material(m,'Bark grooves',(.31,.16,.078),'wood')
    can=[material(m,'Palette Main canopy %s'%i,c,None,.84) for i,c in enumerate([
        (.79,.35,.43),(.92,.47,.51),(.93,.53,.53),(.64,.24,.37),(.96,.59,.56)])]
    green=[material(m,'Palette Flora leaf %s'%i,c,None) for i,c in enumerate([(.27,.49,.15),(.40,.61,.23),(.20,.36,.087)])]
    petals=material(m,'Palette Trim flower petals',(.99,.72,.67),None,.68)
    flower_centers=material(m,'Flower stamens',(.99,.63,.18),None)
    branch(m,[((0,.03,0),.235),((.15,.48,-.02),.20),((.13,.92,.07),.178),((-.06,1.34,.09),.153),((-.05,1.70,.04),.123),((.17,2.07,-.10),.079),((.29,2.43,-.21),.021)],bark,18)
    for i in range(6):
        a=TAU*i/6+.4
        tip=(math.cos(a)*rng.uniform(.54,.78),.073,math.sin(a)*rng.uniform(.53,.81))
        branch(m,[((.015,.38,.012),.12),((tip[0]*.44,.194,tip[2]*.47),.084),((tip[0]*.78,.105,tip[2]*.76),.051),(tip,.007)],bark,10)
    tips=[(-.88,1.99,.27),(-.67,2.38,-.48),(-.14,2.61,.41),(.73,2.39,.22),(.64,2.60,-.45),(.24,2.96,-.14),(1.03,2.06,-.31)]
    for i,p in enumerate(tips):
        root=(.07,1.34+i*.090,.034)
        branch(m,[(root,.119),((p[0]*.47,p[1]-.37,p[2]*.63),.083),((p[0]*.77,p[1]-.12,p[2]*.86),.055),(p,.013)],bark,11)
        for j in range(6):
            angle=TAU*j/6+(.39*i)
            tup=add(p,(math.cos(angle)*.31,rng.uniform(-.07,.21),math.sin(angle)*.26))
            extent=(rng.uniform(.40,.57),rng.uniform(.29,.44),rng.uniform(.35,.54))
            ellipsoid(m,tup,extent,can[(i+j*2)%5],18,12,warp=.14,yaw=angle,salt=i+j)
            if math.sin(angle)>.24:
                out=unit((.23*math.cos(angle),rng.uniform(.24,.51),.87))
                front=add(tup,tuple(v*r*1.04 for v,r in zip(out,extent)))
                blossom(m,front,rng.uniform(.095,.142),petals,flower_centers,i+j,out)
        for j in range(8):
            angle=rng.random()*TAU; radius=rng.uniform(.41,.69)
            q=add(p,(math.cos(angle)*radius,rng.uniform(.19,.48),math.sin(angle)*radius))
            if j%3==0:
                ellipsoid(m,q,(.145,.021,.067),rng.choice(green),12,7,.05,yaw=-angle)
            else:
                blossom(m,q,rng.uniform(.069,.115),petals,flower_centers,angle)
    for i in range(5):
        angle=i*TAU/5
        branch(m,[((.21*math.cos(angle),.145,.22*math.sin(angle)),.016),((.15+.19*math.cos(angle+.3),.46,.18*math.sin(angle+.3)),.011),((.10+.176*math.cos(angle+.73),.89,.07+.16*math.sin(angle+.73)),.007),((-.06+.15*math.cos(angle+1.1),1.38,.08+.134*math.sin(angle+1.1)),.004)],line,6)
    m.save('blossom',OUT)


def stones():
    m=Mesh(); rng=random.Random(234)
    colors=[material(m,'Palette Trim river stone %d'%i,c,'stone') for i,c in enumerate([(.44,.50,.59),(.32,.40,.46),(.65,.61,.59)])]
    moss=[material(m,'Palette Flora creeping moss %d'%i,c,None) for i,c in enumerate([(.24,.46,.15),(.43,.57,.19),(.29,.55,.19)])]
    gold=material(m,'Soft pollen',(.94,.64,.13),None)
    flower=material(m,'Palette Main violet primrose',(.77,.39,.66),None)
    for i,(x,y,z,sx,sy,sz) in enumerate([(-.42,.29,0.,.53,.33,.39),(.27,.34,-.23,.51,.39,.42),(.61,.15,.28,.37,.18,.31),(-.07,.104,.37,.28,.124,.24)]):
        ellipsoid(m,(x,y,z),(sx,sy,sz),colors[i%3],22,14,.18,yaw=i*.71,power=.83,salt=i)
        for j in range(19):
            t=TAU*rng.random(); w=rng.uniform(.10,.61); d=.11+rng.random()*.06
            p=(x+sx*w*math.cos(t),y+sy*math.sqrt(1-w*w)*.99,z+sz*w*math.sin(t))
            ellipsoid(m,p,(d,.027,d*.86),moss[j%3],10,6,.15,yaw=t)
            if j%6==0: blossom(m,add(p,(0.,.037,0.)),.045,flower,gold,t)
    m.save('stones',OUT)


def cap_ring(m, profile, skin, underside, steps=64, edge=None):
    vert=[]
    for index,(y,radius) in enumerate(profile):
        row=[]
        before=profile[max(0,index-1)]; after=profile[min(len(profile)-1,index+1)]
        dy=after[0]-before[0]; dr=after[1]-before[1]
        for j in range(steps+1):
            t=TAU*j/steps; w=radius*(1+.024*math.sin(t*5)+.020*math.cos(t*3+.8))
            row.append(((math.cos(t)*w,y+math.sin(t*5)*w*.030,math.sin(t)*w),unit((dy*math.cos(t),-dr,dy*math.sin(t))),(j/steps*3.,index/(len(profile)-1))))
        vert.append(row)
    for i in range(len(vert)-1):
        for j in range(steps):
            p=[vert[i][j],vert[i+1][j],vert[i+1][j+1],vert[i][j+1]]
            if profile[i][1]<.001: p.pop(3)
            if profile[i+1][1]<.001: p.pop(2)
            mat=underside if i<3 else (edge if i==3 and edge is not None else skin)
            m.face([item[0] for item in p],mat,[item[2] for item in p],[item[1] for item in p])


def mushroom():
    m=Mesh(); rng=random.Random(512)
    cap=material(m,'Palette Main berry mushroom',(.72,.24,.32),'chalk',.78)
    gill=material(m,'Palette Trim pleated gills',(.78,.48,.49),None)
    lip=material(m,'Warm soft border',(.93,.63,.47),None)
    foot=material(m,'Cream mushroom stem',(.94,.79,.58),'chalk')
    ornament=material(m,'Gold pollen freckles',(.95,.71,.27),None)
    dark=material(m,'Dark glaze',(.075,.027,.040),None,.52)
    shine=material(m,'Pale reflection',(.97,.91,.87),None)
    cheek=material(m,'Rosy cheek',(.91,.29,.33),None)
    foliage=material(m,'Palette Flora moss',(.36,.56,.18),None)
    profile=[(.96,0.),(.87,.24),(.845,.49),(.87,.68),(.93,.70),(1.01,.67),(1.13,.61),(1.23,.51),(1.34,.38),(1.40,.25),(1.445,.12),(1.47,0.)]
    cap_ring(m,profile,cap,gill,64,lip)
    ellipsoid(m,(0,.44,0),(.211,.45,.201),foot,28,16,.055,power=.82)
    for i in range(52):
        t=i*TAU/52
        tube_between(m,(.13*math.cos(t),.910,.13*math.sin(t)),(.64*math.cos(t),.871,.64*math.sin(t)),.012,gill,6)
    for _ in range(34):
        t=rng.uniform(0,TAU); rr=rng.uniform(.12,.616); sz=rng.uniform(.012,.042)
        y=max(y0+(y1-y0)*(rr-r0)/(r1-r0) for (y0,r0),(y1,r1) in zip(profile[4:],profile[5:]) if r1<=rr<=r0)
        ellipsoid(m,(math.cos(t)*rr,y+.015,math.sin(t)*rr),(sz,sz*.31,sz*.91),ornament,12,6,warp=.22)
    smile(m,.517,.196,.081,dark,shine,cheek,dark,.69)
    for i in range(14):
        t=TAU*i/14
        ellipsoid(m,(.22*math.cos(t),.055,.21*math.sin(t)),(.064,.047,.065),foliage,12,7,.17)
    m.save('mushroom',OUT)


def barrel():
    m=Mesh()
    staves=[material(m,'Palette Main honey oak %d'%i,c,'wood') for i,c in enumerate([(.64,.37,.146),(.74,.43,.177),(.70,.36,.11),(.81,.43,.19)])]
    band=material(m,'Dark bronze strips',(.28,.154,.081),'stone',.66)
    syrup=material(m,'Honey glaze',(.995,.53,.071),None,.31,(.29,.075,.009))
    nail=material(m,'Brass rim rivets',(.88,.53,.13),None,.48)
    seal=material(m,'Palette Trim lavender seal',(.54,.34,.59),None,.64)
    colors=[(.041,.021,.030),(.93,.77,.57),(.97,.29,.27)]
    colors=[material(m,'Face glaze %d'%i,c,None,.53) for i,c in enumerate(colors)]
    profile=[(.035,.389),(.113,.422),(.32,.496),(.56,.515),(.79,.472),(.91,.438),(.96,.425)]
    for i in range(21):
        t=i*TAU/21; dt=TAU/21*.48
        for (y,radius),(yy,rr) in zip(profile,profile[1:]):
            ring=[(math.cos(a)*r,Y,math.sin(a)*r) for a,r,Y in [(t-dt,radius,y),(t-dt,rr,yy),(t+dt,rr,yy),(t+dt,radius,y)]]
            m.face(ring,staves[(i*i+3*i)%4],[(0,y),(0,yy),(.29,yy),(.29,y)])
    for y,a in [(.185,.472),(.748,.484)]:
        loop=[((a*math.cos(t*TAU/42),y,a*math.sin(t*TAU/42)),.049) for t in range(43)]
        branch(m,loop,band,10)
        for i in range(9):
            t=i*TAU/9
            ellipsoid(m,((a+.038)*math.cos(t),y,(a+.038)*math.sin(t)),(.028,.030,.028),nail,11,7)
    ellipsoid(m,(0,.933,0),(.415,.041,.415),syrup,44,12,.026)
    for i,(t,n) in enumerate([(1.15,.181),(1.46,.265),(1.93,.151),(.59,.136)]):
        radii=[(.937,.397,.048),(.927,.458,.045),(.896-n*.21,.478,.033),(.914-n,.514,.027)]
        branch(m,[((r*math.cos(t),y,r*math.sin(t)),w) for y,r,w in radii],syrup)
        xyz=(.514*math.cos(t),.920-n,.514*math.sin(t))
        ellipsoid(m,xyz,(.038,.053,.037),syrup,14,9,.08,yaw=-t,salt=i)
    tube_between(m,(.79,.203,.22),(.64,.960,.122),.047,staves[0])
    ellipsoid(m,(.64,.971,.122),(.084,.092,.081),staves[0],16,10)
    for i in range(6):
        r=min(.110,.074+.026*math.sin(i*PI/5)+.015)
        ellipsoid(m,(.80,.136+i*.039,.226),(r,.034,r),syrup if i in [1,3] else staves[2],20,10)
    ellipsoid(m,(.012,.56,.507),(.23,.194,.028),seal,22,13,.018)
    smile(m,.591,.531,.080,*colors,colors[0],.62)
    m.save('honey',OUT)


def brick():
    m=Mesh()
    matte=material(m,'Palette Main peach fired clay',(.88,.34,.34),'chalk',.66)
    socket=material(m,'Palette Trim recessed blue glaze',(.31,.30,.56),None,.58)
    dark=material(m,'Glaze ink',(.061,.024,.047),None,.52)
    shine=material(m,'Pearl highlight',(.97,.95,.87),None,.41)
    cheek=material(m,'Rosy blush',(.99,.43,.47),None)
    # A two-socket tile with a visible dark bottom in each recess.
    m.box((0,.15,0),(.84,.30,.40),matte,.059)
    for x in [-.35,0,.35]:
        m.box((x,.325,0),(.143,.20,.398),matte,.047)
    for z in [-.149,.149]:
        m.box((0,.329,z),(.72,.190,.117),matte,.045)
    for x in [-.177,.177]:
        m.box((x,.287,0),(.188,.017,.180),socket,.022)
    smile(m,.191,.199,.132,dark,shine,cheek,dark,.73)
    m.save('block',OUT)


def path():
    m=Mesh(); rng=random.Random(8267)
    stones=[material(m,'Palette Trim stepping stone %d'%i,c,'stone') for i,c in enumerate([(.67,.63,.52),(.76,.60,.54),(.62,.66,.58),(.68,.61,.61)])]
    greenery=material(m,'Palette Flora low moss',(.28,.46,.13),None)
    blush=material(m,'Palette Main path flower',(.96,.63,.44),None)
    gold=material(m,'Pollen glint',(.94,.65,.13),None)
    for row in range(7):
        for side in [-1,1]:
            x=side*(.26+rng.random()*.04)+math.sin(row*.43)*.14
            z = (row-3)*.426+(side*.058)
            ellipsoid(m,(x,.024+rng.random()*.009,z),(.24,.055+rng.random()*.028,.20),rng.choice(stones),15,9,.20,rng.random(),.80,row+side)
            for j in range(4):
                p=(x+side*.31+rng.uniform(-.071,.080),.020,z+rng.uniform(-.18,.17))
                ellipsoid(m,p,(.067,.024,.066),greenery,10,6,.19,salt=j)
                if not j and row%2==0: blossom(m,add(p,(0,.046,0.)),.039,blush,gold,side)
    m.save('path',OUT)


def meadow():
    m=Mesh(); rng=random.Random(6830)
    greens=[material(m,'Palette Flora meadow layer %d'%i,c,'chalk',.93) for i,c in enumerate([
        (.26,.41,.132),(.32,.47,.159),(.34,.49,.167),(.31,.47,.154),(.30,.441,.152)])]
    stalk=material(m,'Palette Flora sedge',(.29,.48,.16),None)
    bloom=material(m,'Palette Main tiny blooms',(.76,.56,.43),None)
    gold=material(m,'Tiny honey pigments',(.88,.66,.18),None)
    def spot(rad,angle):
        ring=rad*(1+.045*math.sin(angle*7)+.08*math.cos(angle*3-.7))
        x,z=ring*7.32*math.cos(angle), ring*5.25*math.sin(angle)
        return (x,.004+.019*(1-rad**5)+.005*math.sin(x*.56)*math.cos(z*1.2),z)
    for i in range(8):
        for j in range(48):
            p=[spot(k/8,a*TAU/48) for k,a in [(i,j),(i,j+1),(i+1,j+1),(i+1,j)]]
            if i==0: p.pop(1)
            # Small shifts of the same green make the edge readable from above.
            m.face(p,greens[min(4,int(2.0+1.7*math.sin(i*.50+j*.23)))])
    for index in range(141):
        t=rng.random()*TAU
        rad=rng.uniform(.39,.94)
        p=spot(rad,t)
        if abs(p[0]-.55)<.96: continue
        for j in range(3):
            h=rng.uniform(.075,.184)
            a=rng.random()*TAU
            d=(math.cos(a),0,math.sin(a))
            w=(-d[2],0,d[0])
            tip=add(p,add((0,h,0),mul(d,h*.44)))
            mid=add(p,add((0,h*.58,0),mul(d,h*.16)))
            bottom=[add(p,mul(w,x)) for x in [-.013,.013]]
            mg=[add(mid,mul(w,x)) for x in [-.009,.009]]
            m.face([bottom[0],bottom[1],mg[1],mg[0]],stalk)
            m.face([mg[0],mg[1],tip],stalk)
            m.face([mg[0],mg[1],bottom[1],bottom[0]],stalk)
            m.face([tip,mg[1],mg[0]],stalk)
        if not index%9:
            blossom(m,add(p,(0,.060,0)),rng.uniform(.027,.046),bloom,gold,t)
    m.save('meadow',OUT)


if __name__=='__main__':
    for build in [cottage,tree,stones,mushroom,barrel,brick,path,meadow]:
        build()
