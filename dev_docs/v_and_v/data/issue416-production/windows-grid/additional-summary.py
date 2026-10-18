import collections, hashlib, json, pathlib
root=pathlib.Path(__file__).parent
all_records={d.name:[json.loads(x) for x in (d/'calls.jsonl').read_text(encoding='utf-8').splitlines()] for d in root.iterdir() if d.is_dir() and (d/'manifest.json').exists()}
data={}
for name,records in all_records.items():
    fresh=[r for r in records if r['kind']=='fresh_diagnostic']
    levels=[l for r in fresh for l in r['levels']]
    data[name]={
        'phase_sums_seconds':{key:sum(l[key] for l in levels) for key in ['mesh_seconds','assembly_seconds','factorization_seconds','solve_seconds']},
        'max_internal_last2_change':max(v for r in fresh if r['status']=='success' for v in r['internal_changes']),
        'max_boundary_last2_change':max((v for r in fresh if r.get('boundary_changes') is not None for v in r['boundary_changes']),default=None),
        'by_outer_aspect':dict(collections.Counter(str(max(r['height_mm'],r['width_mm'])/min(r['height_mm'],r['width_mm'])) for r in fresh)),
        'max_inner_aspect_ratio':max(r['inner_aspect_ratio'] for r in records),
        'slowest_call':{k:max(fresh,key=lambda r:r['wall_seconds']).get(k) for k in ['label','swapped','repetition','wall_seconds','j_mm4']},
    }
    if name.startswith('repeat-'):
        data[name]['direction_repeat_j_identical']={str(swapped):len(set(r['j_mm4'] for r in fresh if r['swapped']==swapped))==1 for swapped in [False,True]}
        data[name]['first_process_call_and_same_process_repeats_seconds']={str(swapped):[r['wall_seconds'] for r in fresh if r['swapped']==swapped] for swapped in [False,True]}
(root/'additional-summary.json').write_text(json.dumps(data,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(data['grid96'],ensure_ascii=False,indent=2))
files=sorted(p for p in root.rglob('*') if p.is_file() and p.name!='artifact-sha256.json')
(root/'artifact-sha256.json').write_text(json.dumps([{'path':str(p.relative_to(root)),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in files],indent=2),encoding='utf-8')
