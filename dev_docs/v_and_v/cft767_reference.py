"""Issue #767: 公開入口 fixture の独立期待値。production の helper を使わない。"""
import math

F = 235.0
N = 8_000_000.0

for shape, area_mm2, core_mm2 in [
    ("box", 400**2 - 380**2, 380**2),
    ("pipe", math.pi * (400**2 - 380**2) / 4, math.pi * 380**2 / 4),
]:
    for term, concrete_fc, factor in [("Long", 8, 1), ("Short", 16, 1.5)]:
        for e in [100000, 205000, 300000]:
            x = 100**2 * 0.6 * F / (math.pi**2 * e)
            s_fc = F * (1 - 0.4 * x) / (1.5 + 2 * x / 3) if x <= 1 else 0.277 * F / x
            s_nc = area_mm2 * s_fc * factor
            capacity = core_mm2 * concrete_fc + s_nc
            print(shape, term, e, f"sfc={s_fc * factor:.12f}", f"ratio={N / capacity:.12f}")

for e in [100000, 205000, 300000]:
    limit = math.sqrt(math.pi**2 * e / (0.6 * F))
    for factor in [1 - 1e-6, 1.0, 1 + 1e-6]:
        slenderness = limit * factor
        x = (slenderness / limit) ** 2
        s_fc = F * (1 - 0.4 * x) / (1.5 + (2 / 3) * x) if slenderness <= limit else 0.277 * F / x
        ratios = [
            N / (core * concrete_fc + area * s_fc * term_factor)
            for area, core in [(15600, 144400), (3900 * math.pi, 36100 * math.pi)]
            for concrete_fc, term_factor in [(8, 1), (16, 1.5)]
        ]
        print("boundary", e, repr(slenderness), ratios)
