import json,math,pathlib,time
from shapely.geometry import Polygon
from sectionproperties.pre.geometry import Geometry
from sectionproperties.analysis.section import Section
p=pathlib.Path(__file__).parent
rows=json.loads((p/'results.json').read_text());rows+=json.loads((p/'refinement/results.json').read_text())
checks=[]
for shape,n,area in [('ellipse_2_1',128,.0005),('rectangle_10_1',4,.00003125)]:
 r=next(x for x in rows if x['shape']==shape and x['boundary_n']==n and x['normalized_max_area_setting']==area)
 a,b,ell=r['a_semiaxis_or_w'],r['b_semiaxis_or_d'],r['ell']
 if r['kind']=='ellipse':pts=[(a*math.cos(2*math.pi*i/n)/ell,b*math.sin(2*math.pi*i/n)/ell) for i in range(n)]
 else:pts=[(-a/(2*ell),-b/(2*ell)),(a/(2*ell),-b/(2*ell)),(a/(2*ell),b/(2*ell)),(-a/(2*ell),b/(2*ell))]
 geom=Geometry(Polygon(pts));geom.create_mesh(mesh_sizes=area,min_angle=30.,coarse=False)
 sec=Section(geom);sec.calculate_geometric_properties();t=time.perf_counter();sec.calculate_warping_properties(solver_type='direct');j=float(sec.get_j())*ell**4
 checks.append(dict(shape=shape,area=area,j_public_api=j,j_runner=r['j'],relative_difference=j/r['j']-1,full_warping_seconds=time.perf_counter()-t))
 print(checks[-1],flush=True)
(p/'public-api-crosscheck.json').write_text(json.dumps(checks,indent=2))
