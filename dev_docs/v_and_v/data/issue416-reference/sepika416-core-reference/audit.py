import csv,hashlib,json,math,pathlib,platform,sys
import numpy as np
P=pathlib.Path(__file__).parent
rows=json.loads((P/'results.json').read_text())
assert len(rows)==12
for i,r in enumerate(rows):
 assert r['status']=='ok',r
 mesh=np.load(P/r['mesh_npz_path'])
 h=hashlib.sha256(mesh['nodes'].tobytes()+mesh['triangles'].tobytes()).hexdigest()
 assert h==r['mesh_sha256_before']==r['mesh_sha256_after']==r['solver_mesh_sha256_before_centering']==r['solver_mesh_sha256_after']
 assert hashlib.sha256((P/r['mesh_npz_path']).read_bytes()).hexdigest()==r['mesh_npz_file_sha256']
 assert r['original_relative_residual']<=1e-10 and r['augmented_relative_residual']<=1e-10
 assert r['j_mm4']>0 and math.isfinite(r['j_mm4'])
 assert r['negative_or_zero_triangles']==0 and r['holes']==0 and not r['missing_tangents']
 assert r['actual_max_area']<=r['normalized_max_area_setting']*(1+1e-10)
 xyz=mesh['nodes'][mesh['triangles'][:,:3]]
 area=((xyz[:,1,0]-xyz[:,0,0])*(xyz[:,2,1]-xyz[:,0,1])-(xyz[:,2,0]-xyz[:,0,0])*(xyz[:,1,1]-xyz[:,0,1]))/2
 assert abs(float(area.sum())/r['normalized_polygon_area']-1)<=1e-10
 r['sum_triangle_area_mm2']=float(area.sum()*376**2)
 r['triangle_area_vs_polygon_signed_relative']=float(area.sum()/r['normalized_polygon_area']-1)
 r['actual_max_triangle_area_mm2']=r['actual_max_area']*376**2
 tangent_distances=[float(np.linalg.norm(mesh['nodes']-np.array(p)/376,axis=1).min()) for p in r['tangent_points_mm']]
 assert max(tangent_distances)<=1e-12
 r['tangent_max_distance_normalized']=max(tangent_distances)
 r['all_tangents_preserved_in_mesh']=True
summary={}
def series(key,rr):
 d=[abs(b['j_mm4']-a['j_mm4'])/abs(b['j_mm4']) for a,b in zip(rr,rr[1:])]
 good=len(rr)>=3 and all(x<=.001 for x in d[-2:]) and len(set(r['mesh_sha256_before'] for r in rr))==len(rr)
 assert good,(key,d)
 summary[key]=dict(row_indices=[r['row_index'] for r in rr],relative_changes=d,last_two_relative_changes=d[-2:],final_j_mm4=rr[-1]['j_mm4'],passed=good)
series('Core_r0_internal',[r for r in rows if r['shape']=='Core_r0'])
for n in [16,32,64]:
 rr=[r for r in rows if r['shape']=='Core_r18' and r['boundary_segments_per_quadrant']==n]
 assert len(set(json.dumps(r['polygon_vertices_mm']) for r in rr))==1
 assert all(a['actual_max_area']>b['actual_max_area'] for a,b in zip(rr,rr[1:]))
 series(f'Core_r18_n{n}_internal',rr)
series('Core_r18_boundary_common_area',[r for r in rows if r['shape']=='Core_r18' and r['normalized_max_area_setting']==.000125])
w,d=376.,276.
s=0.
for N in range(1,1000000,2):
 s+=math.tanh(N*math.pi*w/(2*d))/N**5
 exact=w*d**3/3*(1-192*d/(math.pi**5*w)*s)
 tail=w*d**3/3*192*d/(math.pi**5*w)/(4*N**4)
 if tail/exact<=1e-10:break
rectangle=dict(width_long_mm=w,depth_short_mm=d,j_series_mm4=exact,last_odd_N=N,absolute_tail_bound_mm4=tail,relative_tail_bound=tail/exact,tail_proof='sum odd n>N tanh(c*n)/n^5 <= sum integers n>N 1/n^5 <= integral_N^infinity x^-5 dx = 1/(4*N^4); multiplied by prefactor; floating point rounding not bounded')
core0=summary['Core_r0_internal']['final_j_mm4'];core18=summary['Core_r18_boundary_common_area']['final_j_mm4']
rectangle['reference_signed_relative_error']=core0/exact-1
assert abs(rectangle['reference_signed_relative_error'])<=.001
old=d**3*w/16*(16/3-3.36*(d/w)*(1-(d/w)**4/12))
comparison=[]
for ro,r,j in [(0,0,core0),(12,0,core0),(30,18,core18)]:
 comparison.append(dict(box_H_mm=400,box_B_mm=300,wall_t_mm=12,outer_radius_mm=ro,core_depth_ref_y_mm=376,core_width_ref_x_mm=276,core_radius_mm=r,reference_j_mm4=j,old_rectangle_j_mm4=old,reference_minus_old_mm4=j-old,reference_minus_old_over_old=(j-old)/old,sign='positive' if j>old else 'negative',source_row_index=2 if r==0 else 11))
(P/'summary.json').write_text(json.dumps(summary,indent=2));(P/'rectangle-series.json').write_text(json.dumps(rectangle,indent=2));(P/'core-old-comparison.json').write_text(json.dumps(comparison,indent=2));(P/'audited-results.json').write_text(json.dumps(rows,indent=2))
for name,data in [('core-old-comparison',comparison),('audited-results',rows)]:
 with (P/(name+'.csv')).open('w',newline='') as f:
  wr=csv.DictWriter(f,fieldnames=list(dict.fromkeys(k for r in data for k in r)));wr.writeheader();wr.writerows(data)
E=P.parent/'sepika416-reference-environment';manifest=json.loads((E/'artifact-sha256.json').read_text());checks={f:hashlib.sha256((E/f).read_bytes()).hexdigest()==sha for f,sha in manifest.items()};assert len(checks)==26 and all(checks.values())
(P/'environment-after.json').write_text(json.dumps(checks,indent=2))
facts=dict(rows=len(rows),all_series_passed=True,max_original_relative_residual=max(r['original_relative_residual'] for r in rows),total_compute_seconds=sum(r['total_seconds'] for r in rows),min_compute_seconds=min(r['total_seconds'] for r in rows),max_compute_seconds=max(r['total_seconds'] for r in rows),max_nodes=max(r['nodes'] for r in rows),max_triangles=max(r['triangles'] for r in rows),max_triangle_area_relative_error=max(abs(r['triangle_area_vs_polygon_signed_relative']) for r in rows))
(P/'facts.json').write_text(json.dumps(facts,indent=2));print(json.dumps(dict(facts=facts,summary=summary,rectangle=rectangle,comparison=comparison),indent=2))
(P/'runtime.json').write_text(json.dumps(dict(python=sys.version,executable=sys.executable,platform=platform.platform(),solver='Tri6/P2 warping Neumann, mean-zero Lagrange, scipy.spsolve',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1',PYTHONDONTWRITEBYTECODE='1'),indent=2))

corrected=[dict(r,axis_mapping='(x_ref,y_ref)=(SEPIKA element local z,y)',axis_mapping_correction='axis-mapping-correction.json') for r in rows]
(P/'corrected-metadata-results.json').write_text(json.dumps(corrected,indent=2))
with (P/'corrected-metadata-results.csv').open('w',newline='') as f:
 wr=csv.DictWriter(f,fieldnames=list(dict.fromkeys(k for r in corrected for k in r)));wr.writeheader();wr.writerows(corrected)
