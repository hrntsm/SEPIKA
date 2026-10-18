import csv,hashlib,inspect,json,math,pathlib,platform,sys
import numpy as np
from shapely.affinity import scale
from sectionproperties.pre.geometry import Geometry
from sectionproperties.analysis.section import Section
from sectionproperties.pre.library import utils
import runner
P=pathlib.Path(__file__).parent
rows=json.loads((P/'results.json').read_text());summary={};comparisons=[]
(P/'draw-radius-installed-source.txt').write_text(inspect.getsource(utils.draw_radius))
for key in dict.fromkeys(r['shape'] for r in rows):
 group=[r for r in rows if r['shape']==key];internal=[r for r in group if r['series']=='internal_fixed_polygon']
 ds=[abs(b['j_mm4']-a['j_mm4'])/abs(b['j_mm4']) for a,b in zip(internal,internal[1:])]
 assert len(internal)>=3 and max(ds[-2:])<=.001,(key,ds)
 assert len(set(r['mesh_sha256'] for r in internal))==len(internal)
 boundary=[r for r in group if r['series']==('boundary_fixed_area_refined_common_area' if key=='H_r13' else 'boundary_fixed_area')]
 bd=[abs(b['j_mm4']-a['j_mm4'])/abs(b['j_mm4']) for a,b in zip(boundary,boundary[1:])]
 if boundary:
  assert len(boundary)>=3 and max(bd[-2:])<=.001
  assert len(set(r['normalized_max_area_setting'] for r in boundary))==1
  assert boundary[-1]['normalized_max_area_setting']==internal[-1]['normalized_max_area_setting']
 final=internal[-1];summary[key]=dict(internal_steps=len(internal),internal_relative_changes=ds,boundary_steps=len(boundary),boundary_relative_changes=bd,j_reference_mm4=final['j_mm4'],final_nodes=final['nodes'],final_triangles=final['triangles'],final_seconds=final['total_seconds'])
 r=float(key.split('_r')[1]);kind=key.split('_')[0]
 if kind=='H':model=(2*200*13**3+(400-2*13)*8**3)/3;extra=None;pm=am=rm=None
 else:
  rm=0 if r==0 else r-6
  am=(300-12)*(400-12)-(4-math.pi)*rm**2
  pm=2*(300+400-24)+(2*math.pi-8)*rm
  model=4*12*am**2/pm;extra=model+12**3*pm/3
 comparison=dict(shape=key,reference_j_mm4=final['j_mm4'],model_j_mm4=model,extra_term_model_j_mm4=extra,model_minus_reference_relative=model/final['j_mm4']-1,reference_minus_model_relative=final['j_mm4']/model-1,model_twist_minus_reference_relative=final['j_mm4']/model-1,rm_mm=rm,pm_mm=pm,am_mm2=am)
 if extra is not None:comparison.update(extra_minus_reference_relative=extra/final['j_mm4']-1,addition_relative_to_centerline=extra/model-1)
 comparisons.append(comparison)
assert all(r['status']=='ok' and r['original_relative_residual']<=1e-10 and r['j_mm4']>0 and math.isfinite(r['j_mm4']) and r['actual_max_area']<=r['normalized_max_area_setting']*(1+1e-10) for r in rows)
arithmetic=dict(h_calculated=comparisons[0]['model_j_mm4'],h_fixed=356762.6666666667,box30_pm_fixed=1310.79644737231,box30_am_fixed=111249.55736846772,box30_j_fixed=453213215.4428274,box30_extra_fixed=453968234.19651383)
for label,calc,expected in [('H',comparisons[0]['model_j_mm4'],356762.6666666667),('Box30_pm',comparisons[-1]['pm_mm'],1310.79644737231),('Box30_Am',comparisons[-1]['am_mm2'],111249.55736846772),('Box30_J',comparisons[-1]['model_j_mm4'],453213215.4428274),('Box30_extra',comparisons[-1]['extra_term_model_j_mm4'],453968234.19651383)]:
 arithmetic[label+'_signed_relative_arithmetic_error']=calc/expected-1
 assert abs(calc/expected-1)<=1e-10
(P/'summary.json').write_text(json.dumps(summary,indent=2));(P/'comparisons.json').write_text(json.dumps(comparisons,indent=2));(P/'arithmetic.json').write_text(json.dumps(arithmetic,indent=2))
keys=list(dict.fromkeys(k for r in rows for k in r))
with (P/'results.csv').open('w',newline='') as f:
 w=csv.DictWriter(f,fieldnames=keys);w.writeheader();w.writerows(rows)
with (P/'comparisons.csv').open('w',newline='') as f:
 keys=list(dict.fromkeys(k for r in comparisons for k in r));w=csv.DictWriter(f,fieldnames=keys);w.writeheader();w.writerows(comparisons)
# 数値解採用meshのSHAはrawに保持。再生成meshは非決定性の監査用に別保存する。
M=P/'meshes';M.mkdir(exist_ok=True);mesh_audit=[]
for i,row in enumerate(rows):
 kind,r=row['shape'].split('_r');g,_=runner.shape(kind,int(r),row['boundary_segments_per_quadrant'])
 geom=Geometry(scale(g.geom,xfact=1/400,yfact=1/400,origin=(0,0)));geom.create_mesh(mesh_sizes=row['normalized_max_area_setting'],min_angle=30.,coarse=False);s=Section(geom)
 digest=hashlib.sha256(s.mesh_nodes.tobytes()+s.mesh_elements.tobytes()).hexdigest()
 name=f'{i:02}_{row["shape"]}.npz';np.savez_compressed(M/name,nodes=s.mesh_nodes,triangles=s.mesh_elements)
 mesh_audit.append(dict(file='meshes/'+name,sha256=digest,matching_original=(digest==row['mesh_sha256']),original_sha256=row['mesh_sha256']))
(P/'mesh-reconstruction-audit.json').write_text(json.dumps(mesh_audit,indent=2))
E=P.parent/'sepika416-reference-environment';old=json.loads((E/'artifact-sha256.json').read_text());checks={f:hashlib.sha256((E/f).read_bytes()).hexdigest()==sha for f,sha in old.items()};assert all(checks.values());(P/'environment-evidence-unchanged.json').write_text(json.dumps(checks,indent=2))
(P/'runtime.json').write_text(json.dumps(dict(python=sys.version,executable=sys.executable,platform=platform.platform(),solver='scipy.sparse.linalg.spsolve; sequential single process; Tri6/P2 warping Neumann with mean-zero multiplier',reference_package='sectionproperties 3.10.2'),indent=2))
print(json.dumps(dict(summary=summary,comparisons=comparisons,max_residual=max(r['original_relative_residual'] for r in rows),rows=len(rows),total_seconds=sum(r['total_seconds'] for r in rows),max_nodes=max(r['nodes'] for r in rows),max_triangles=max(r['triangles'] for r in rows)),indent=2))


