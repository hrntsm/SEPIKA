import csv,hashlib,json,pathlib
P=pathlib.Path(__file__).parent
E=P.parent/'sepika416-reference-environment'
rows=json.loads((P/'results.json').read_text())+json.loads((P/'refinement/results.json').read_text())
groups={}
for r in rows:groups.setdefault((r['shape'],r['series']),[]).append(r)
summaries=[]
for (shape,series),rr in groups.items():
 changes=[abs(b['j']-a['j'])/abs(b['j']) for a,b in zip(rr,rr[1:])]
 changed=all(a['mesh_sha256']!=b['mesh_sha256'] for a,b in zip(rr,rr[1:]))
 ok=len(rr)>=3 and max(changes[-2:])<=1e-3 and abs(rr[-1]['signed_relative_error'])<=1e-3 and changed
 if series=='internal_fixed_polygon':ok=ok and all(b['actual_max_area']<a['actual_max_area'] for a,b in zip(rr,rr[1:]))
 summaries.append(dict(shape=shape,series=series,levels=len(rr),all_relative_changes=changes,last_two_changes=changes[-2:],j_final=rr[-1]['j'],exact_j=rr[-1]['exact_j'],final_signed_relative_error=rr[-1]['signed_relative_error'],distinct_meshes=changed,passed=ok))
assert len(rows)==25 and all(r['status']=='ok' for r in rows)
assert all(s['passed'] for s in summaries)
assert all(r['original_relative_residual']<=1e-10 for r in rows)
assert all(r['actual_min_area']>0 and r['negative_triangle_count']==0 for r in rows)
assert all(r['actual_max_area']<=r['normalized_max_area_setting']*(1+1e-12) for r in rows)
assert all(r['series_relative_tail_bound']<=1e-8 for r in rows if r['kind']=='rectangle')
for r in rows:r['residual_within_1e_minus_10']=r.pop('residual_within_1e10')
(P/'all-results.json').write_text(json.dumps(rows,indent=2))
with (P/'all-results.csv').open('w',newline='') as f:
 w=csv.DictWriter(f,fieldnames=list(dict.fromkeys(k for r in rows for k in r)));w.writeheader();w.writerows(rows)
(P/'summary.json').write_text(json.dumps(summaries,indent=2))
old=json.loads((E/'artifact-sha256.json').read_text());checks={name:hashlib.sha256((E/name).read_bytes()).hexdigest()==sha for name,sha in old.items()}
assert all(checks.values())
(P/'environment-evidence-unchanged.json').write_text(json.dumps(checks,indent=2))
report=['Status: done','', '# 独立二次要素そり参照：非円形校正・残差観測','',
'原Issue416の全6Doneは未完了。本phaseは楕円2形状・矩形4形状の参照校正のみ。repo/source/Cargo/Git/GH/依存導入を行っていない。H/Box/coregrid、本番P1、回転・移動・尺度変換は次phaseに残す。','',
'## 固定環境と独立性','',
'Python 3.12.10 / Windows 11 / 固定公開MIT参照3.10.2。実行exeは `'+str(E/'venv/Scripts/python.exe')+'`。freeze/hashlockの変更なし。前phaseの26個のSHA対象をすべて再照合して一致した（environment-evidence-unchanged.json）。新成果は本ディレクトリだけ、旧証拠は上書きしていない。','',
'参照は直辺Tri6/P2そり純Neumann＋平均零Lagrangeで、本番Prandtl/P1 Dirichletとは式・次数・実装が異なる。材料は無指定の均質幾何J。断面平面は(x_ref,y_ref)、材軸とは独立の校正座標。','',
'公式実版のSection.calculate_warping_properties、assemble_torsion、solve_direct_lagrangeをローカルsourceで全文確認しapi-source-confirmed.jsonへ保存。重心移動した要素で [K,c;cᵀ,0][ω,λ]=[f,0] を組立し、scipy.sparse.linalg.spsolveで解いた。J=Ixx+Iyy−ωᵀfは公式sourceと同じ機能的表式（連立式成立時にωᵀKωと等価）。無関係なせん断解析は省略した。公開calculate_warping_properties(direct)との楕円(2,1)粗境界、矩形(10,1)最細内部2件の交差照合はJ差0。公開APIとの差分0は公開solverの残差保証と解釈しない。','',
'一次理論: https://sectionproperties.readthedocs.io/en/latest/user_guide/theory.html#torsion-constant 、固定版source: https://github.com/robbievanleeuwen/section-properties/tree/v3.10.2/src/sectionproperties/analysis 。','',
'## 形状・正規化・期待値','',
'楕円(a,b)=(2,1),(4,1)は半軸であり全幅は(2a,2b)。ell=2max(a,b)=4,8。矩形(w,d)=(1,1),(2,1),(4,1),(10,1)は全幅でell=max(w,d)=1,2,4,10。座標をellで除しJ=ell⁴Jhatで復元。ここでは任意長さ単位u（寸法をmmと読めばJはmm⁴）。尺度s倍試験を実行したことは意味しない。','',
'楕円厳密J=πa³b³/(a²+b²)。矩形期待値は独立級数 J=wd³/3[1−192d/(π⁵w)Σodd tanh(nπw/(2d))/n⁵]。本番コードを使用・複写しない。最後の奇数Nの後の省略項は全整数の正の尾より小さく、Σ_{k>N}k⁻⁵<=∫N∞x⁻⁵dx=1/(4N⁴)。これにwd³/3×192d/(π⁵w)を掛けた絶対tailを保存し、相対<=1e−10で停止（要求1e−8より厳しい）。このboundは浮動小数演算の厳密保証ではなく級数打切り誤差の上限。','',
'楕円境界系列はn=128/256/512全周内接点、正規化最大面積.0005固定。楕円内部系列はn=512固定、面積.002/.0005/.000125。矩形は同じ面積3段、比10だけ.00003125を追加した。min_angle30°、coarse=False。各系列の元境界ポリゴンを固定し、内部メッシュは実際に変化している。設定・実最大面積・mesh hash・nodes/triangles・所要時間はCSV/JSONに記録。','',
'## 収束観測','',
'相対変化はabs(Jk−Jk−1)/abs(Jk)。最終連続2区間<=.1%、最終の厳密/級数差<=.1%を全8系列で観測。','',
'|形状|系列|段数|最終J|期待J|最終符号付き差%|最後2区間%|','|---|---|---:|---:|---:|---:|---|']
for s in summaries:
 report.append(f"|{s['shape']}|{s['series']}|{s['levels']}|{s['j_final']:.15g}|{s['exact_j']:.15g}|{s['final_signed_relative_error']*100:.9g}|"+', '.join(f'{x*100:.9g}' for x in s['last_two_changes'])+'|')
report+=['','比10矩形の最粗段は符号付き差+0.562177883%で品質未達。次段との差0.539818%も未達であり除外せずraw resultsに保持した。4段目追加後の最後2区間で合格。これはsolver実行失敗ではない。全25評価は正常終了し、例外・失敗実行は無い。run.log/refine.log/public-crosscheck.logをrawのまま保持。','',
'楕円内部J変化は丸め規模（最終2区間が約1e−12%以下）。ただし(2,1)のnodes5625→6761→12961、(4,1)5405→5709→8245で、実最大面積も低下しmesh SHAも全段で異なる。同一meshのδJ=0による偽収束ではない。境界誤差が支配する観測で、一般形状の内部離散化誤差の証明にはしない。','',
'## 独立残差','',
'元のK=A[:-1,:-1]とfを保持しrho=||Kω−f||₂/||f||₂を実測。拡大系残差||Au−rhs||₂/||rhs||₂と拘束|cᵀω|、scaled constraint、λも別保存した。公開solverの乗数1e−7/絶対丸め検査は別の列であり、1e−10達成へ言い換えていない。非円形のf normは全て正で、零荷重割算ではない。',
 f"25評価の元残差最大 {max(r['original_relative_residual'] for r in rows):.15g}、拡大系残差最大 {max(r['augmented_relative_residual'] for r in rows):.15g}、拘束絶対値最大 {max(r['mean_zero_absolute_constraint'] for r in rows):.15g}、scaled constraint最大 {max(r['mean_zero_scaled_constraint'] for r in rows):.15g}。全元残差<=1e−10。これらは参照solverの実測で、本番P1の達成証拠ではない。",
'', '## 実行・成果物・限界','',
 f"25評価の合計算定時間 {sum(r['total_seconds'] for r in rows):.9g} 秒、各評価 {min(r['total_seconds'] for r in rows):.9g}〜{max(r['total_seconds'] for r in rows):.9g} 秒。公開API交差照合時間は別。各1回の観測でcold/warm統計・代表域性能保証ではない。負/ゼロ面積要素なし、設定最大面積を全評価で満足。最大nodes {max(r['nodes'] for r in rows)} / triangles {max(r['triangles'] for r in rows)}。",
'', '再現: commands.ps1。runner.py→run.log/results.json/csv（初回24件）、refine.py→refine.log/refinement/results.json（追加1件）、all-results.json/csv（全25件・rawの残差bool名誤記を明確化）、public_crosscheck.py→public-api-crosscheck.json、audit.py→summary.json/environment-evidence-unchanged.json/report.md、artifact-sha256.json。数値補正・外挿・閾値緩和なし。',
'', '円/円環は前phaseの同時細分化校正のみ。本phaseと合わせても一般H/Box/core、非同心穴、回転・移動・尺度変換、独立参照対本番P1の直接差は未実施。二つの.1%から総誤差.1%を保証しない。全6Done完了を主張しない。',
'', '半分checkpointと2/3 freezeをrootへ報告後、必須比10細分化と監査・成果整理に限定。環境と出力の所有権をrootへ解放する。このagentは以後変更しない。']
(P/'report.md').write_text('\n'.join(report),encoding='utf-8')
(P.parent/'sepika416-reference-calibration-report.md').write_text('\n'.join(report),encoding='utf-8')
manifest={str(f.relative_to(P)):hashlib.sha256(f.read_bytes()).hexdigest() for f in P.rglob('*') if f.is_file() and f.name!='artifact-sha256.json'}
(P/'artifact-sha256.json').write_text(json.dumps(manifest,indent=2))
print(json.dumps(summaries,indent=2));print('Old environment SHA verified:',len(checks))
