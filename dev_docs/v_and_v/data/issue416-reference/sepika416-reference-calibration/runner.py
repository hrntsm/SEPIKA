import csv, hashlib, inspect, json, math, pathlib, platform, sys, time, traceback
import numpy as np
from scipy.sparse.linalg import spsolve
from shapely.geometry import Polygon
from sectionproperties.pre.geometry import Geometry
from sectionproperties.analysis.section import Section
from sectionproperties.analysis import solver
P=pathlib.Path(__file__).parent
SOURCES={'assemble_torsion':inspect.getsource(Section.assemble_torsion),'direct_solver':inspect.getsource(solver.solve_direct_lagrange),'calculate_warping_properties':inspect.getsource(Section.calculate_warping_properties)}
(P/'api-source-confirmed.json').write_text(json.dumps(SOURCES,indent=2))
rows=[]
def exact_rectangle(w,d):
    coef=w*d**3/3; k=192*d/(math.pi**5*w); total=0.; n=1
    while True:
        total+=math.tanh(n*math.pi*w/(2*d))/n**5
        result=coef*(1-k*total)
        # All omitted odd terms are a subset of all integer terms > n.
        # Integral test bounds that larger tail by 1/(4*n**4).
        tail=coef*k/(4*n**4)
        if tail/result<=1e-10:
            return result,n,tail,tail/result
        n+=2

def run(kind,a,b,n,area,series):
    ell=2*max(a,b) if kind=='ellipse' else max(a,b)
    if kind=='ellipse':
        points=[(a*math.cos(2*math.pi*i/n)/ell,b*math.sin(2*math.pi*i/n)/ell) for i in range(n)]
        exact=math.pi*a**3*b**3/(a*a+b*b); terms=tail=tailrel=None
    else:
        points=[(-a/(2*ell),-b/(2*ell)),(a/(2*ell),-b/(2*ell)),(a/(2*ell),b/(2*ell)),(-a/(2*ell),b/(2*ell))]
        exact,terms,tail,tailrel=exact_rectangle(a,b)
    row=dict(shape=f'{kind}_{a:g}_{b:g}',kind=kind,a_semiaxis_or_w=a,b_semiaxis_or_d=b,ell=ell,coordinate_normalization='x_hat=x/ell; J=ell^4*J_hat',series=series,boundary_n=n,normalized_max_area_setting=area,exact_j=exact,series_last_odd_n=terms,series_absolute_tail_bound=tail,series_relative_tail_bound=tailrel,solver='scipy_spsolve augmented mean-zero Neumann',element='Tri6_P2',status='running')
    start=time.perf_counter()
    try:
        geom=Geometry(Polygon(points)); row['normalized_polygon_area']=geom.geom.area
        t=time.perf_counter();geom.create_mesh(mesh_sizes=area,min_angle=30.,coarse=False);row['mesh_seconds']=time.perf_counter()-t
        sec=Section(geom);row['nodes']=sec.num_nodes;row['triangles']=len(sec.mesh_elements)
        coords=sec.mesh_nodes[sec.mesh_elements[:,:3]]
        signed=(coords[:,1,0]-coords[:,0,0])*(coords[:,2,1]-coords[:,0,1])-(coords[:,2,0]-coords[:,0,0])*(coords[:,1,1]-coords[:,0,1])
        row['actual_max_area']=float(np.max(abs(signed))/2);row['actual_min_area']=float(np.min(abs(signed))/2);row['negative_triangle_count']=int(np.count_nonzero(signed<0))
        row['mesh_sha256']=hashlib.sha256(sec.mesh_nodes.tobytes()+sec.mesh_elements.tobytes()).hexdigest()
        t=time.perf_counter();sec.calculate_geometric_properties();row['geometric_seconds']=time.perf_counter()-t
        cx,cy=sec.get_c();ws=Section(geom)
        for el in ws.elements:
            el.coords[0,:]-=cx;el.coords[1,:]-=cy
        t=time.perf_counter();A,f=ws.assemble_torsion();row['assembly_seconds']=time.perf_counter()-t
        t=time.perf_counter();rhs=np.append(f,0.);u=spsolve(A,rhs);row['solve_seconds']=time.perf_counter()-t
        omega=u[:-1];K=A[:-1,:-1];c=A[:-1,-1].toarray().ravel()
        row['nnz_augmented']=A.nnz;row['lagrange_multiplier']=float(u[-1]);row['f_norm']=float(np.linalg.norm(f))
        row['original_relative_residual']=float(np.linalg.norm(K@omega-f)/np.linalg.norm(f))
        row['augmented_relative_residual']=float(np.linalg.norm(A@u-rhs)/np.linalg.norm(rhs))
        row['mean_zero_absolute_constraint']=float(abs(c@omega))
        row['mean_zero_scaled_constraint']=float(abs(c@omega)/(np.linalg.norm(c)*np.linalg.norm(omega)))
        row['multiplier_relative']=float(abs(u[-1])/np.max(abs(u)))
        row['public_multiplier_check_pass']=not(row['multiplier_relative']>1e-7 and abs(u[-1])>10*np.finfo(float).eps)
        # Same published torsion functional, without unrelated shear analyses.
        ixx,iyy,_=sec.get_ic();jhat=float(ixx+iyy-omega@f)
        row['j_hat']=jhat;row['j']=jhat*ell**4;row['signed_relative_error']=row['j']/exact-1
        row['error_within_point1_percent']=abs(row['signed_relative_error'])<=1e-3
        row['residual_within_1e10']=row['original_relative_residual']<=1e-10
        row['status']='ok'
    except Exception as ex:
        row['status']='failed';row['reason']=repr(ex);traceback.print_exc()
    row['total_seconds']=time.perf_counter()-start;rows.append(row)
    (P/'results.json').write_text(json.dumps(rows,indent=2));print(json.dumps(row),flush=True)

for a,b in [(2.,1.),(4.,1.)]:
    for n in [128,256,512]:run('ellipse',a,b,n,.0005,'boundary_fixed_area')
    for area in [.002,.0005,.000125]:run('ellipse',a,b,512,area,'internal_fixed_polygon')
for w,d in [(1.,1.),(2.,1.),(4.,1.),(10.,1.)]:
    for area in [.002,.0005,.000125]:run('rectangle',w,d,4,area,'internal_fixed_polygon')
keys=list(dict.fromkeys(k for r in rows for k in r))
with (P/'results.csv').open('w',newline='') as file:
    writer=csv.DictWriter(file,fieldnames=keys);writer.writeheader();writer.writerows(rows)
(P/'runtime.json').write_text(json.dumps(dict(python=sys.version,executable=sys.executable,platform=platform.platform()),indent=2))
