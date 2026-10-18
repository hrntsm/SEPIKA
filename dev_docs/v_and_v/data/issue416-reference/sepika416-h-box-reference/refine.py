import json,pathlib
import runner
P=pathlib.Path(__file__).parent
runner.rows=json.loads((P/'results.json').read_text())
for a in [.0000078125,.000001953125]:runner.run('H',0,64,a,'internal_fixed_polygon')
runner.run('H',13,64,.0000078125,'internal_fixed_polygon')
for n in [16,32,64]:runner.run('H',13,n,.0000078125,'boundary_fixed_area_refined_common_area')
runner.run('Box',0,64,.0000078125,'internal_fixed_polygon')
