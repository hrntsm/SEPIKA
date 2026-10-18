import collections, hashlib, json, math, pathlib, statistics, sys
root = pathlib.Path(__file__).parent
workspace = pathlib.Path(r'C:\Users\hiroa\Desktop\ikkan')
summary = {}
violations = []
for directory in sorted(root.iterdir()):
    if not directory.is_dir() or not (directory/'manifest.json').exists():
        continue
    records = [json.loads(line) for line in (directory/'calls.jsonl').read_text(encoding='utf-8').splitlines()]
    manifest = json.loads((directory/'manifest.json').read_text(encoding='utf-8-sig'))
    environment = json.loads((directory/'environment.json').read_text(encoding='utf-8-sig'))
    fresh = [r for r in records if r['kind'] == 'fresh_diagnostic']
    failures = [r for r in fresh if r['status'] != 'success']
    levels = [l for r in fresh for l in r['levels']]
    accepted = [r['levels'][r['accepted_level']] for r in fresh if r['status'] == 'success']
    for r in records:
        if r['kind'] == 'alias_no_solve':
            original = records[r['alias_of']]
            if r['wall_seconds'] is not None or r['levels'] or r['j_mm4'] != original['j_mm4']:
                violations.append([directory.name, r['call_id'], 'alias contract'])
    for r in fresh:
        if r['status'] != 'success':
            continue
        if not (math.isfinite(r['j_mm4']) and r['j_mm4'] > 0):
            violations.append([directory.name, r['call_id'], 'invalid J'])
        if len(r['levels']) < 3 or max(r['internal_changes']) > .001:
            violations.append([directory.name, r['call_id'], 'internal threshold'])
        if (r['inner_r_mm'] > 0) != (r['boundary_changes'] is not None):
            violations.append([directory.name, r['call_id'], 'boundary presence'])
        if r['boundary_changes'] is not None and max(r['boundary_changes']) > .001:
            violations.append([directory.name, r['call_id'], 'boundary threshold'])
        for l in r['levels']:
            if not (0 <= l['relative_residual'] <= 1e-10 and l['vertices'] <= 200000 and l['triangles'] <= 400000 and l['min_signed_double_area'] > 0 and l['free_dofs'] > 0):
                violations.append([directory.name, r['call_id'], 'level quality/cap'])
        a = r['levels'][r['accepted_level']]
        if a['j_hat'] != r['j_hat']:
            violations.append([directory.name, r['call_id'], 'accepted mismatch'])
        boundary_n = a['attempt']['quarter_arc_segments']
        target_area = a['attempt']['max_triangle_area']
        recent_ns = [boundary_n] if r['boundary_changes'] is None else [boundary_n//4, boundary_n//2, boundary_n]
        for n in recent_ns:
            selected = [next((l for l in r['levels'] if l['attempt']['quarter_arc_segments']==n and l['attempt']['max_triangle_area']==area), None) for area in [target_area*16,target_area*4,target_area]]
            if any(l is None for l in selected):
                violations.append([directory.name, r['call_id'], 'missing common internal last3'])
                continue
            changes = [abs(selected[i+1]['j_hat']-selected[i]['j_hat']) / abs(selected[i+1]['j_hat']) for i in [0,1]]
            if max(changes) > .001:
                violations.append([directory.name, r['call_id'], 'common internal last2 threshold'])
    times = [r['wall_seconds'] for r in fresh]
    summary[directory.name] = {
        'manifest': manifest,
        'process': json.loads((directory/'process.json').read_text(encoding='utf-8-sig')),
        'status_counts': dict(collections.Counter(r['status'] for r in fresh)),
        'fresh_wall_seconds': {'min': min(times), 'median':statistics.median(times), 'max':max(times), 'sum':sum(times)},
        'full_level_count':len(levels),
        'max_relative_residual':max(l['relative_residual'] for l in levels),
        'max_vertices':max(l['vertices'] for l in levels),
        'max_triangles':max(l['triangles'] for l in levels),
        'max_free_dofs':max(l['free_dofs'] for l in levels),
        'max_nnz':max(l['nnz'] for l in levels),
        'max_swap_relative_difference':max(r['swap_relative_difference'] or 0 for r in records),
        'accepted_area_denominator_counts':dict(collections.Counter(str(round(1/l['attempt']['max_triangle_area'])) for l in accepted)),
        'accepted_quarter_n_counts':dict(collections.Counter(str(l['attempt']['quarter_arc_segments']) for l in accepted)),
        'failures':[{'label':r['label'],'swapped':r['swapped'],'reason':r['reason'],'levels':len(r['levels']),'failed_attempt':r['failed_attempt']} for r in failures],
        'source_changed_since_run': [item['path'] for item in environment['source_sha256'] if hashlib.sha256((workspace/item['path']).read_bytes()).hexdigest().upper()!=item['sha256']],
        'executable_sha256':environment['executable_sha256'],
        'first_fresh_call':{k:fresh[0].get(k) for k in ['label','swapped','repetition','wall_seconds','j_mm4','status']},
    }
(root/'summary.json').write_text(json.dumps({'runs':summary,'quality_audit_violations':violations},ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({'run_names':list(summary),'quality_audit_violations':violations,'fresh_calls':sum(s['manifest']['actual_fresh_diagnostic_calls'] for s in summary.values())},ensure_ascii=False))
