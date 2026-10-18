import csv, json, math, pathlib, time, traceback
import numpy as np
from sectionproperties.pre.library import circular_section
from sectionproperties.analysis.section import Section
p=pathlib.Path(__file__).parent
rows=[]
for shape in ['circle_r1','annulus_ro1_ri0.5']:
    for n,area in [(128,1e-3),(256,2.5e-4),(512,6.25e-5)]:
        row=dict(shape=shape,n_outer=n,n_inner=n if shape.startswith('annulus') else 0,max_triangle_area=area,outer_radius=1.,inner_radius=.5 if shape.startswith('annulus') else 0.,solver='direct',element='Tri6_P2',boundary='warping_pure_Neumann_mean_zero_Lagrange',status='running')
        started=time.perf_counter()
        try:
            geom=circular_section(d=2.,n=n)
            if shape.startswith('annulus'): geom=geom-circular_section(d=1.,n=n)
            row['polygon_area']=geom.geom.area
            row['holes']=len(geom.geom.interiors)
            t=time.perf_counter(); geom.create_mesh(mesh_sizes=area,min_angle=30.,coarse=False); row['mesh_seconds']=time.perf_counter()-t
            sec=Section(geometry=geom)
            row['nodes']=len(sec.mesh_nodes);row['triangles']=len(sec.mesh_elements)
            coords=sec.mesh_nodes[sec.mesh_elements[:,:3]]
            signed=(coords[:,1,0]-coords[:,0,0])*(coords[:,2,1]-coords[:,0,1])-(coords[:,2,0]-coords[:,0,0])*(coords[:,1,1]-coords[:,0,1])
            row['actual_max_triangle_area']=float(np.max(np.abs(signed))/2)
            row['min_abs_triangle_area']=float(np.min(np.abs(signed))/2)
            t=time.perf_counter();sec.calculate_geometric_properties();row['geometric_seconds']=time.perf_counter()-t
            t=time.perf_counter();sec.calculate_warping_properties(solver_type='direct');row['warping_seconds']=time.perf_counter()-t
            row['j']=float(sec.get_j()); row['exact_j']=math.pi*(1-row['inner_radius']**4)/2
            row['signed_relative_error']=row['j']/row['exact_j']-1
            row['absolute_error_percent']=abs(row['signed_relative_error'])*100
            row['within_point1_percent']=abs(row['signed_relative_error'])<=1e-3
            row['status']='ok'
        except Exception as e:
            row['status']='failed';row['reason']=repr(e);traceback.print_exc()
        row['total_seconds']=time.perf_counter()-started
        rows.append(row)
        (p/'calibration-results.json').write_text(json.dumps(rows,indent=2))
        print(json.dumps(row),flush=True)
keys=list(dict.fromkeys(k for r in rows for k in r))
with (p/'calibration-results.csv').open('w',newline='') as f:
    w=csv.DictWriter(f,fieldnames=keys);w.writeheader();w.writerows(rows)

