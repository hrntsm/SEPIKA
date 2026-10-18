import csv,hashlib,json,pathlib
P=pathlib.Path(__import__('os').environ['TEMP'])/'sepika416-core-reference'
old=json.loads((P/'artifact-sha256.json').read_text());assert all(hashlib.sha256((P/f).read_bytes()).hexdigest()==sha for f,sha in old.items())
protected=['results.json','results.csv','run.log','audited-results.json','audited-results.csv','summary.json','facts.json','rectangle-series.json','core-old-comparison.json','core-old-comparison.csv','savedmesh-reload-checks.json','reload.log','environment-before.json','environment-after.json','environment-final.json','previous-reproduction-evidence-unchanged.json']+[str(p.relative_to(P)) for p in (P/'meshes').glob('*.npz')]
protected_before={f:hashlib.sha256((P/f).read_bytes()).hexdigest() for f in protected}
changed=['runner.py','prepare.py','audit.py','finalize.py','report.md','artifact-sha256.json']
for name in changed:
 src=P/name;dst=P/(src.stem+'-before-axis-correction'+src.suffix)
 assert not dst.exists();dst.write_bytes(src.read_bytes())
outer=P.parent/'sepika416-core-reference-report.md';outerbackup=P/'external-report-before-axis-correction.md';assert not outerbackup.exists();outerbackup.write_bytes(outer.read_bytes())
oldaxis='(x_ref,y_ref)=(SEPIKA section y,z)';newaxis='(x_ref,y_ref)=(SEPIKA element local z,y)'
source='crates/sepika-core/src/section_shape/face_geometry.rs'
repo=pathlib.Path('C:/Users/hiroa/Desktop/ikkan');sourcebytes=(repo/source).read_bytes()
code=(repo/source).read_text(encoding='utf-8')
assert 'y: [-h / 2.0, h / 2.0]' in code and 'z: [-w / 2.0, w / 2.0]' in code
for name in ['runner.py','prepare.py']:
 f=P/name;s=f.read_text();assert oldaxis in s;s=s.replace(oldaxis,newaxis);f.write_text(s)
explanation='''軸表記訂正：参照xは幅276mmで要素局所z、参照yはせい376mmで要素局所yに対応する。下端左端原点の参照座標から重心原点の局所座標への関係は z_local=x_ref−138mm、y_local=y_ref−188mm。したがってIref_xは局所z軸まわり、Iref_yは局所y軸まわりである。根拠は crates/sepika-core/src/section_shape/face_geometry.rs のSteelH分岐およびrounded_rectangleで、局所y範囲にh、局所z範囲にwを使用している。旧raw内のaxis_mappingだけは逆表記であり、axis-mapping-correction.jsonとcorrected-metadata-results.json/CSVを併用する。実座標・J・A/I・残差・時間・NPZに変更はなく、再計算していない。旧版report/runner/manifest等はbefore-axis-correction名で保全した。環境hash照合は実行当時の履歴として保持した。'''
for name in ['report.md','finalize.py']:
 f=P/name;s=f.read_text(encoding='utf-8');assert '(SEPIKA断面y,z)' in s;s=s.replace('(SEPIKA断面y,z)','(SEPIKA要素局所z,y)')
 anchor='ref x幅276・ref yせい376を取り違えない。';assert anchor in s;s=s.replace(anchor,anchor+'\n\n'+explanation+'\n\n')
 f.write_text(s,encoding='utf-8')
outer.write_bytes((P/'report.md').read_bytes())
# rawの監査結果は履歴として保持し、訂正metadataは別の派生成果へ保存する。
derivedcode='''
corrected=[dict(r,axis_mapping='(x_ref,y_ref)=(SEPIKA element local z,y)',axis_mapping_correction='axis-mapping-correction.json') for r in rows]
(P/'corrected-metadata-results.json').write_text(json.dumps(corrected,indent=2))
with (P/'corrected-metadata-results.csv').open('w',newline='') as f:
 wr=csv.DictWriter(f,fieldnames=list(dict.fromkeys(k for r in corrected for k in r)));wr.writeheader();wr.writerows(corrected)
'''
f=P/'audit.py';f.write_text(f.read_text()+derivedcode)
rows=json.loads((P/'audited-results.json').read_text());exec(derivedcode)
for a,b in zip(rows,corrected):
 assert a['axis_mapping']==oldaxis and b['axis_mapping']==newaxis
 assert {k:v for k,v in a.items() if k!='axis_mapping'}=={k:v for k,v in b.items() if k not in ['axis_mapping','axis_mapping_correction']}
assert all(hashlib.sha256((P/f).read_bytes()).hexdigest()==sha for f,sha in protected_before.items())
correction=dict(reason='要素局所y=せい、局所z=幅との対応表記が逆だった',old_metadata=oldaxis,correct_metadata=newaxis,reference_dimensions_mm=dict(ref_x_width=276,ref_y_depth=376),coordinate_relation_mm=dict(local_z='x_ref - 138',local_y='y_ref - 188'),inertia_relation=dict(I_ref_x='I_local_z',I_ref_y='I_local_y'),source=dict(path=source,sha256=hashlib.sha256(sourcebytes).hexdigest(),evidence='SteelH rect(height y, width z); rounded_rectangle Primitive::Rectangle y=[-h/2,h/2], z=[-w/2,w/2]'),no_new_numerical_evaluation=True,raw_results_unchanged=True,npz_unchanged=True,protected_unchanged_sha256=protected_before,modified_files=changed[:-1]+['sepika416-core-reference-report.md'],derived_files=['corrected-metadata-results.json','corrected-metadata-results.csv'],previous_manifest='artifact-sha256-before-axis-correction.json',environment_hash_checks='historical results preserved unchanged; no fresh environment check asserted')
(P/'axis-mapping-correction.json').write_text(json.dumps(correction,indent=2,ensure_ascii=False),encoding='utf-8')
manifest={str(p.relative_to(P)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(P.rglob('*')) if p.is_file() and p.name!='artifact-sha256.json'}
(P/'artifact-sha256.json').write_text(json.dumps(manifest,indent=2))
assert all(hashlib.sha256((P/f).read_bytes()).hexdigest()==sha for f,sha in manifest.items())
print(json.dumps(dict(manifest_entries=len(manifest),protected_unchanged=len(protected_before),backups=len(changed)+1,corrected_metadata_rows=len(corrected),new_manifest=str(P/'artifact-sha256.json')),indent=2))
