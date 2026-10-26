from decimal import Decimal as D, getcontext
getcontext().prec = 60
pi = D("3.14159265358979323846264338327950288419716939937510582097494")
b, depth, effective_depth = D(400), D(600), D("537.5")
at = D(5) * pi * D(25)**2 / D(4)
pw = D(2) * pi * D(10)**2 / D(4) / (b * D(100))
pt = D(100) * at / (b * effective_depth)
j = D(7) * effective_depth / D(8)
base = (D(".068") * pt**D(".23") * D(42) / D("1.12") + D(".85") * (pw*D(295)).sqrt()) * b*j
print("units: N, mm, N/mm2; lambda=1, Fc=24, sigmawy=295")
print("at=", at, "de=", effective_depth, "pw=", pw, "pt%=", pt)
print("Qsu_zero_N=", base)
for n in [-240000, 0, 240000]:
    delta = D(".1")*D(n)/(b*depth)*b*j
    print("N=",n,"signed_comparison_only_N=",base+delta,"delta_N=",delta)
print("seismic_final_N=800000; increment_N=8000")
print("first_grid_at_zero=84, Q=672000; previous_Q=664000")
print("first_grid_at_compression=86, Q=688000; previous_Q=680000")

span_ratio = D(600) / effective_depth
fixed_base = (D(".068")*pt**D(".23")*D(42)/(span_ratio+D(".12")) + D(".85")*(pw*D(295)).sqrt())*b*j
print("fixed_sway1200: actual M/Q=600mm; a/D=1; lambda=",span_ratio)
for n in [-240000,0,240000]:
    current = fixed_base + D(".1")*max(D(0),D(n))/(b*depth)*b*j
    print("N=",n,"fixed_sway_Qsu=",current,"first800k=", int(current/D(8000))+1,"first660k=", int(current/D(6600))+1)
