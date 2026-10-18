import json, pathlib, platform, sys, subprocess, hashlib, urllib.request, inspect
p=pathlib.Path(__file__).parent
sources={
'pypi-3.10.2.json':'https://pypi.org/pypi/sectionproperties/3.10.2/json',
'pyproject-v3.10.2.toml':'https://raw.githubusercontent.com/robbievanleeuwen/section-properties/v3.10.2/pyproject.toml',
'release-v3.10.2.json':'https://api.github.com/repos/robbievanleeuwen/section-properties/releases/tags/v3.10.2',
'primary-theory.html':'https://sectionproperties.readthedocs.io/en/stable/user_guide/theory.html',
'primary-circle.html':'https://sectionproperties.readthedocs.io/en/stable/gen/sectionproperties.pre.library.primitive_sections.circular_section.html'}
source_records=[]
for name,url in sources.items():
    try:
        data=urllib.request.urlopen(url).read(); (p/name).write_bytes(data)
        source_records.append(dict(file=name,url=url,sha256=hashlib.sha256(data).hexdigest(),status='ok'))
    except Exception as e: source_records.append(dict(file=name,url=url,status='failed',reason=str(e)))
(p/'primary-sources.json').write_text(json.dumps(source_records,indent=2))
report=json.loads((p/'install-report.json').read_text(encoding='utf-8-sig'))
locks=[]
for row in report['install']:
    name=row['metadata']['name']; ver=row['metadata']['version']; hashes=row['download_info']['archive_info']['hashes']
    locks.append(f"{name}=={ver} --hash=sha256:{hashes['sha256']}")
(p/'requirements-hashed.lock').write_text('\n'.join(sorted(locks,key=str.lower))+'\n')
freeze=subprocess.check_output([sys.executable,'-m','pip','freeze','--all'],text=True)
(p/'freeze.txt').write_text(freeze)
(p/'environment.json').write_text(json.dumps(dict(executable=sys.executable,version=sys.version,platform=platform.platform(),machine=platform.machine(),processor=platform.processor(),dependencies=freeze.splitlines()),indent=2))
from sectionproperties.pre.library import circular_section
from sectionproperties.analysis.section import Section
from sectionproperties.analysis import solver
(p/'circle-api-source.txt').write_text(inspect.getsource(circular_section))
(p/'direct-solver-source.txt').write_text(inspect.getsource(solver.solve_direct_lagrange))
(p/'warping-api-source.txt').write_text(inspect.getsource(Section.calculate_warping_properties))
print((p/'environment.json').read_text())
print(json.dumps(source_records,indent=2))
