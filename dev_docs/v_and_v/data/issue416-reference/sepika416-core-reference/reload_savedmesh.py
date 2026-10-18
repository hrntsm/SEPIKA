import hashlib,inspect,json,pathlib,time
import numpy as np
from scipy.sparse.linalg import spsolve
from shapely.affinity import scale
from sectionproperties.pre.geometry import Geometry
from sectionproperties.analysis.section import Section
import runner
P=pathlib.Path(__file__).parent
rows=json.loads((P/'results.json').read_text());checks=[]
(P/'installed-section-init-source.txt').write_text(inspect.getsource(Section.__init__))
for idx in [11]:
 r=rows[idx];start=time.perf_counter();mesh=np.load(P/r['mesh_npz_path'])
 nodes=mesh['nodes'];elements=mesh['triangles'];raw=elements.copy()
 raw[:,[3,4,5]]=elements[:,[4,5,3]]
 kind,rad=r['shape'].split('_r');g,_=runner.shape(kind,int(rad),r['boundary_segments_per_quadrant'])
 geom=Geometry(scale(g.geom,xfact=1/376,yfact=1/376,origin=(0,0)))
 geom.mesh=dict(vertices=nodes,triangles=raw,triangle_attributes=np.zeros((len(raw),1),dtype=int))
 s=Section(geom)
 h=hashlib.sha256(s.mesh_nodes.tobytes()+s.mesh_elements.tobytes()).hexdigest();assert h==r['mesh_sha256_before']
 s.calculate_geometric_properties();ix,iy,_=s.get_ic();cx,cy=s.get_c()
 ws=Section(geom)
 for el in ws.elements:el.coords[0]-=cx;el.coords[1]-=cy
 A,f=ws.assemble_torsion();rhs=np.append(f,0.);u=spsolve(A,rhs);w=u[:-1]
 j=float(ix+iy-w@f)*376**4
 residual=float(np.linalg.norm(A[:-1,:-1]@w-f)/np.linalg.norm(f))
 c=dict(row_index=idx,shape=r['shape'],mesh_sha256=h,mesh_generated=False,source_j_mm4=r['j_mm4'],reloaded_j_mm4=j,exact_j_equal=j==r['j_mm4'],signed_relative_j_difference=j/r['j_mm4']-1,original_relative_residual=residual,total_seconds=time.perf_counter()-start)
 assert abs(c['signed_relative_j_difference'])<=1e-12 and residual<=1e-10
 checks.append(c);print(json.dumps(c),flush=True)
(P/'savedmesh-reload-checks.json').write_text(json.dumps(checks,indent=2))
