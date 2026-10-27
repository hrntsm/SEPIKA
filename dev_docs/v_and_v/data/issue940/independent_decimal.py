"""Mcの項別感度と固定した単材骨格の独立算術。製品関数・solverは呼ばない。"""

from decimal import Decimal, getcontext

getcontext().prec = 60
D = Decimal
fc, depth, ze_gross = D(25), D(600), D(18000000)
my, k0, alpha_y = D(300000000), D(30000000000), D("0.3")
ry = my / (alpha_y * k0)
rotation, moment = D("0.005"), D(80000000)

print("Independent arithmetic only; not a production run or validation")
print("Fc=25 N/mm2, b=300 mm, D=600 mm, Zgross=18000000 mm3")
print("My=300000000 Nmm, K0=30000000000 Nmm/rad, alpha_y=0.3")
print("case, supplied_Ze_mm3, N_N, Mc_Nmm, Rc_rad, K2_Nmm/rad, M_at_R005_Nmm, R_at_M80M_rad")
for label, ze, axial in [
    ("gross_zero", ze_gross, D(0)),
    ("supplied_Ze_sensitivity", D(21600000), D(0)),
    ("compression_same_Ze", ze_gross, D(600000)),
    ("compression_supplied_Ze", D(21600000), D(600000)),
]:
    mc = D("0.56") * fc.sqrt() * ze + axial * depth / 6
    rc = mc / k0
    k2 = (my - mc) / (ry - rc)
    force = k0 * rotation if rotation <= rc else mc + k2 * (rotation - rc)
    angle = moment / k0 if moment <= mc else rc + (moment - mc) / k2
    assert 0 < mc < my and 0 < rc < ry and k2 > 0
    print(", ".join([label, str(ze), str(axial)] + [f"{v:.12f}" for v in (mc, rc, k2, force, angle)]))

assert D("0.56") * fc.sqrt() * ze_gross == D(50400000)
assert D(600000) * depth / 6 == D(60000000)
raw_mc = D(50400000)
for supplied_my, expected in [(D(40000000), D(36000000)), (D(600000000), D(60000000))]:
    limited = min(max(raw_mc, supplied_my * D("0.1")), supplied_my * D("0.9"))
    assert limited == expected
    print(f"generic spring arithmetic: My={supplied_my}, limited_Mc={limited} Nmm")
assert min(raw_mc, D(40000000)) == D(40000000)
print(f"event arithmetic: My=40000000, limited_Mc={min(raw_mc, D(40000000))} Nmm")
print("assertions: PASS")
