import csv,hashlib,inspect,json,math,pathlib,platform,sys
import numpy as np
P=pathlib.Path(__file__).parent
rows=json.loads((P/'results.json').read_text());old=json.loads((P/'original-results.json').read_text())
assert len(rows)==len(old)==31
comparisons=[]
for i,(r,o) in enumerate(zip(rows,old)):
 for key in ['shape','series','boundary_segments_per_quadrant','normalized_max_area_setting']: assert r[key]==o[key]
 mesh=np.load(P/r['mesh_npz_path'])
 h=hashlib.sha256(mesh['nodes'].tobytes()+mesh['triangles'].tobytes()).hexdigest()
 assert h==r['mesh_sha256']==r['mesh_sha256_before']==r['mesh_sha256_after']==r['solver_mesh_sha256_before_centering']==r['solver_mesh_sha256_after']
 assert hashlib.sha256((P/r['mesh_npz_path']).read_bytes()).hexdigest()==r['mesh_npz_file_sha256']
 assert r['status']=='ok' and r['original_relative_residual']<=1e-10 and r['augmented_relative_residual']<=1e-10 and r['j_mm4']>0 and math.isfinite(r['j_mm4'])
 assert r['negative_or_zero_triangles']==0 and r['actual_max_area']<=r['normalized_max_area_setting']*(1+1e-10)
 c=dict(row_index=i,shape=r['shape'],series=r['series'],old_mesh_sha256=o['mesh_sha256'],new_mesh_sha256=h,mesh_bytes_equal_to_old_hash=h==o['mesh_sha256'],nodes_equal=r['nodes']==o['nodes'],triangles_equal=r['triangles']==o['triangles'],min_area_equal=r['actual_min_area']==o['actual_min_area'],max_area_equal=r['actual_max_area']==o['actual_max_area'],old_j_mm4=o['j_mm4'],new_j_mm4=r['j_mm4'],j_exact_equal=r['j_mm4']==o['j_mm4'],j_difference_mm4=r['j_mm4']-o['j_mm4'],j_signed_relative_to_old=r['j_mm4']/o['j_mm4']-1)
 prior=P.parent/'sepika416-h-box-reference'/'meshes'/f'{i:02}_{r["shape"]}.npz'
 prev=np.load(prior)
 c['old_reconstruction_npz_nodes_bytes_equal']=mesh['nodes'].tobytes()==prev['nodes'].tobytes()
 c['old_reconstruction_npz_elements_bytes_equal']=mesh['triangles'].tobytes()==prev['triangles'].tobytes()
 c['old_reconstruction_npz_coordinates_numeric_equal']=bool(np.array_equal(mesh['nodes'],prev['nodes']))
 c['old_reconstruction_npz_max_abs_coordinate_difference']=float(np.max(np.abs(mesh['nodes']-prev['nodes']))) if mesh['nodes'].shape==prev['nodes'].shape else None
 c['old_reconstruction_npz_elements_numeric_equal']=bool(np.array_equal(mesh['triangles'],prev['triangles']))
 comparisons.append(c)
summary={}
for key in dict.fromkeys(r['shape'] for r in rows):
 group=[r for r in rows if r['shape']==key]
 selected=['internal_fixed_polygon']+(['boundary_fixed_area_refined_common_area' if key=='H_r13' else 'boundary_fixed_area'] if key not in ['H_r0','Box_r0'] else [])
 summary[key]={}
 for series in selected:
  rr=[r for r in group if r['series']==series]
  ds=[abs(b['j_mm4']-a['j_mm4'])/abs(b['j_mm4']) for a,b in zip(rr,rr[1:])]
  passed=len(rr)>=3 and all(d<=.001 for d in ds[-2:]) and len(set(r['mesh_sha256'] for r in rr))==len(rr)
  assert passed,(key,series,ds)
  summary[key][series]=dict(row_indices=[r['row_index'] for r in rr],steps=len(rr),relative_changes=ds,last_two_relative_changes=ds[-2:],quality_passed=passed,final_j_mm4=rr[-1]['j_mm4'])
 if len(selected)>1:
  assert summary[key][selected[0]]['row_indices'] and [r for r in group if r['series']==selected[0]][-1]['normalized_max_area_setting']==[r for r in group if r['series']==selected[1]][-1]['normalized_max_area_setting']
assert sum(len(v) for v in summary.values())==8
E=P.parent/'sepika416-reference-environment';manifest=json.loads((E/'artifact-sha256.json').read_text());checks={f:hashlib.sha256((E/f).read_bytes()).hexdigest()==sha for f,sha in manifest.items()};assert len(checks)==26 and all(checks.values())
(P/'environment-after.json').write_text(json.dumps(checks,indent=2))
(P/'summary.json').write_text(json.dumps(summary,indent=2))
(P/'old-new-comparison.json').write_text(json.dumps(comparisons,indent=2))
for name,rr in [('old-new-comparison',comparisons),('results',rows)]:
 with (P/(name+'.csv')).open('w',newline='') as f:
  w=csv.DictWriter(f,fieldnames=list(dict.fromkeys(k for r in rr for k in r)));w.writeheader();w.writerows(rr)
facts=dict(rows=len(rows),series=8,old_mesh_hash_matches=sum(c['mesh_bytes_equal_to_old_hash'] for c in comparisons),old_j_exact_matches=sum(c['j_exact_equal'] for c in comparisons),max_abs_j_relative_difference=max(abs(c['j_signed_relative_to_old']) for c in comparisons),max_original_relative_residual=max(r['original_relative_residual'] for r in rows),max_augmented_relative_residual=max(r['augmented_relative_residual'] for r in rows),all_counts_and_extreme_areas_equal=all(c[k] for c in comparisons for k in ['nodes_equal','triangles_equal','min_area_equal','max_area_equal']),total_compute_seconds=sum(r['total_seconds'] for r in rows),quality_passed=True)
(P/'facts.json').write_text(json.dumps(facts,indent=2));print(json.dumps(facts,indent=2))
(P/'runtime.json').write_text(json.dumps(dict(python=sys.version,executable=sys.executable,platform=platform.platform(),solver='scipy.sparse.linalg.spsolve; sequential single process; Tri6/P2 warping Neumann with mean-zero multiplier'),indent=2))
