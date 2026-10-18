import csv,hashlib,inspect,json,math,pathlib,sys,time,traceback
import numpy as np
from scipy.integrate import quad
from scipy.sparse.linalg import spsolve
from shapely.geometry import Polygon
from sectionproperties.pre.geometry import Geometry
from sectionproperties.pre.library.steel_sections import i_section,rectangular_hollow_section
from sectionproperties.analysis.section import Section
P=pathlib.Path(__file__).parent
ELL=400.
(P/'installed-shape-api-source.json').write_text(json.dumps({f.__name__:inspect.getsource(f) for f in [i_section,rectangular_hollow_section]},indent=2))
rows=[]
def rr(b,h,r,n):
 if r==0:return [(0,0),(b,0),(b,h),(0,h)]
 out=[]
 for cx,cy,a in [(r,r,math.pi),(b-r,r,1.5*math.pi),(b-r,h-r,0),(r,h-r,.5*math.pi)]:
  out.extend([(cx+r*math.cos(a+math.pi*.5*i/n),cy+r*math.sin(a+math.pi*.5*i/n)) for i in range(n+1)])
 return out

def shape(kind,r,n):
 if kind=='H':
  g=i_section(d=400,b=200,t_f=13,t_w=8,r=r,n_r=n+1)
  # Independent sharp cross plus four square-minus-disk root additions.
  sharp=Polygon([(0,0),(200,0),(200,13),(104,13),(104,387),(200,387),(200,400),(0,400),(0,387),(96,387),(96,13),(0,13)])
  exact=sharp
  tangents=[(0,0),(200,0),(200,400),(0,400)]
  if r:
   for x0,y0,cx,cy,a in [(104,13,104+r,13+r,math.pi),(104,387-r,104+r,387-r,math.pi*.5),(96-r,13,96-r,13+r,math.pi*1.5),(96-r,387-r,96-r,387-r,0)]:
    square=Polygon([(x0,y0),(x0+r,y0),(x0+r,y0+r),(x0,y0+r)])
    arc=[(cx+r*math.cos(a+math.pi*.5*i/n),cy+r*math.sin(a+math.pi*.5*i/n)) for i in range(n+1)]
    sector=Polygon([(cx,cy)]+arc)
    exact=exact.union(square.difference(sector))
   tangents += [(104+r,13),(104,13+r),(104,387-r),(104+r,387),(96-r,13),(96,13+r),(96,387-r),(96-r,387)]
  area_exact=2*200*13+374*8+(4-math.pi)*r*r
 else:
  ri=0 if r==0 else r-12
  outer=Polygon(rr(300,400,r,n));inner=Polygon([(x+12,y+12) for x,y in rr(276,376,ri,n)])
  exact=outer.difference(inner)
  g=Geometry(exact) if r==0 else rectangular_hollow_section(d=400,b=300,t=12,r_out=r,r_in=ri,n_r=n+1)
  tangents=rr(300,400,r,1)+[(x+12,y+12) for x,y in rr(276,376,ri,1)]
  area_exact=300*400-(4-math.pi)*r*r-(276*376-(4-math.pi)*ri*ri)
 diff=g.geom.symmetric_difference(exact).area
 verts=list(g.geom.exterior.coords)+[p for ring in g.geom.interiors for p in ring.coords]
 missing=[p for p in tangents if min(math.dist(p,v) for v in verts)>1e-9]
 if diff>1e-8 or missing:raise ValueError(('shape audit',diff,missing))
 return g,dict(independent_polygon_symmetric_difference_mm2=diff,tangent_count=len(tangents),missing_tangents=missing,analytic_smooth_area_mm2=area_exact,polygon_area_mm2=g.geom.area,polygon_area_signed_relative=g.geom.area/area_exact-1,holes=len(g.geom.interiors),bounds_mm=g.geom.bounds,root_r_mm=r if kind=='H' else None,outer_r_mm=r if kind=='Box' else None,inner_r_mm=(0 if r==0 else r-12) if kind=='Box' else None)

def smooth_moments(kind,r):
 def width(y,b,h,rad):
  if y<0 or y>h:return 0.
  z=min(y,h-y)
  return b if z>=rad or rad==0 else b-2*rad+2*math.sqrt(max(0,rad*rad-(rad-z)**2))
 if kind=='H':
  def w(y):
   if y<13 or y>387:return 200.
   z=min(y-13,387-y)
   return 8. if z>=r or r==0 else 8+2*(r-math.sqrt(max(0,r*r-(r-z)**2)))
  ix=2*quad(lambda y:w(y)*(y-200)**2,0,200,points=[13,13+r],epsabs=.0001)[0]
  iy=2*quad(lambda y:w(y)**3/12,0,200,points=[13,13+r],epsabs=.0001)[0]
 else:
  ri=0 if r==0 else r-12
  ix=quad(lambda y:width(y,300,400,r)*(y-200)**2,0,400,points=[r,400-r],epsabs=.0001)[0]-quad(lambda y:width(y,276,376,ri)*(y-188)**2,0,376,points=[ri,376-ri],epsabs=.0001)[0]
  iy=quad(lambda y:width(y,300,400,r)**3/12,0,400,points=[r,400-r],epsabs=.0001)[0]-quad(lambda y:width(y,276,376,ri)**3/12,0,376,points=[ri,376-ri],epsabs=.0001)[0]
 return ix,iy

def run(kind,r,n,area,series):
 row=dict(shape=f'{kind}_r{r}',series=series,boundary_segments_per_quadrant=n,normalized_max_area_setting=area,ell_mm=ELL,units='input:mm,J:mm4; xhat=x/ell; J=ell^4*Jhat',status='running')
 t0=time.perf_counter()
 try:
  g,audit=shape(kind,r,n);row.update(audit)
  # Explicit physical-to-normalized affine map; no library radius inference.
  p=g.geom
  from shapely.affinity import scale
  geom=Geometry(scale(p,xfact=1/ELL,yfact=1/ELL,origin=(0,0)))
  row['normalized_polygon_area']=geom.geom.area
  t=time.perf_counter();geom.create_mesh(mesh_sizes=area,min_angle=30.,coarse=False);row['mesh_seconds']=time.perf_counter()-t
  s=Section(geom);row['nodes']=s.num_nodes;row['triangles']=len(s.mesh_elements)
  row['row_index']=len(rows)
  meshfile=f'meshes/{len(rows):02}_{kind}_r{r}.npz'
  (P/'meshes').mkdir(exist_ok=True)
  np.savez_compressed(P/meshfile,nodes=s.mesh_nodes,triangles=s.mesh_elements)
  row['mesh_npz_path']=meshfile
  row['mesh_npz_file_sha256']=hashlib.sha256((P/meshfile).read_bytes()).hexdigest()
  row['mesh_sha256_before']=hashlib.sha256(s.mesh_nodes.tobytes()+s.mesh_elements.tobytes()).hexdigest()
  saved=np.load(P/meshfile)
  assert saved['nodes'].tobytes()==s.mesh_nodes.tobytes() and saved['triangles'].tobytes()==s.mesh_elements.tobytes()
  row['saved_mesh_matches_before']=True
  xyz=s.mesh_nodes[s.mesh_elements[:,:3]];signed=((xyz[:,1,0]-xyz[:,0,0])*(xyz[:,2,1]-xyz[:,0,1])-(xyz[:,2,0]-xyz[:,0,0])*(xyz[:,1,1]-xyz[:,0,1]))/2
  row.update(actual_min_area=float(signed.min()),actual_max_area=float(signed.max()),negative_or_zero_triangles=int(np.count_nonzero(signed<=0)),mesh_sha256=hashlib.sha256(s.mesh_nodes.tobytes()+s.mesh_elements.tobytes()).hexdigest())
  if row['negative_or_zero_triangles']:raise ValueError('nonpositive triangle')
  t=time.perf_counter();s.calculate_geometric_properties();row['geometric_seconds']=time.perf_counter()-t
  ix,iy,ixy=s.get_ic();smoothix,smoothiy=smooth_moments(kind,r)
  row.update(ix_mm4=ix*ELL**4,iy_mm4=iy*ELL**4,ixy_mm4=ixy*ELL**4,analytic_smooth_ix_mm4=smoothix,analytic_smooth_iy_mm4=smoothiy,ix_signed_relative=ix*ELL**4/smoothix-1,iy_signed_relative=iy*ELL**4/smoothiy-1,mesh_area_signed_relative=s.get_area()/geom.geom.area-1)
  cx,cy=s.get_c();ws=Section(geom)
  row['solver_mesh_sha256_before_centering']=hashlib.sha256(ws.mesh_nodes.tobytes()+ws.mesh_elements.tobytes()).hexdigest()
  assert row['solver_mesh_sha256_before_centering']==row['mesh_sha256_before']
  for el in ws.elements:el.coords[0]-=cx;el.coords[1]-=cy
  t=time.perf_counter();A,f=ws.assemble_torsion();row['assembly_seconds']=time.perf_counter()-t
  t=time.perf_counter();rhs=np.append(f,0.);u=spsolve(A,rhs);row['solve_seconds']=time.perf_counter()-t
  K=A[:-1,:-1];c=A[:-1,-1].toarray().ravel();w=u[:-1]
  row.update(nnz_augmented=A.nnz,f_norm=float(np.linalg.norm(f)),original_relative_residual=float(np.linalg.norm(K@w-f)/np.linalg.norm(f)),augmented_relative_residual=float(np.linalg.norm(A@u-rhs)/np.linalg.norm(rhs)),constraint_absolute=float(abs(c@w)),lagrange_multiplier=float(u[-1]))
  row['j_hat']=float(ix+iy-w@f);row['j_mm4']=row['j_hat']*ELL**4;row['status']='ok'
  row['mesh_sha256_after']=hashlib.sha256(s.mesh_nodes.tobytes()+s.mesh_elements.tobytes()).hexdigest()
  row['solver_mesh_sha256_after']=hashlib.sha256(ws.mesh_nodes.tobytes()+ws.mesh_elements.tobytes()).hexdigest()
  assert row['mesh_sha256_before']==row['mesh_sha256_after']==row['solver_mesh_sha256_after']
  row['mesh_unchanged_after_solve']=True
 except Exception as e:row.update(status='failed',reason=repr(e));traceback.print_exc()
 row['total_seconds']=time.perf_counter()-t0;rows.append(row)
 (P/'results.json').write_text(json.dumps(rows,indent=2));print(json.dumps(row),flush=True)

def main():
 settings=json.loads((P/'settings.json').read_text())
 for setting in settings:
  kind,r=setting['shape'].split('_r')
  run(kind,int(r),setting['boundary_segments_per_quadrant'],setting['normalized_max_area_setting'],setting['series'])
 keys=list(dict.fromkeys(k for row in rows for k in row))
 with (P/'results.csv').open('w',newline='') as f:
  w=csv.DictWriter(f,fieldnames=keys);w.writeheader();w.writerows(rows)
if __name__=='__main__':main()
