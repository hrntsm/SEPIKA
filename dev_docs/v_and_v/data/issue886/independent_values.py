"""原画像の限定算術と現行仕様の固定期待値。製品実装を呼ばない。単位 N, mm。"""
from math import pi, sqrt, sin

fc_prime = 24 * (0.85 - 2.5 * 0.02)
print("appendix_concrete,Fc_prime,cNuc,cN0,cM0", fc_prime,
      600 * 600 * fc_prime, 0.5 * 600 * 600 * fc_prime,
      0.125 * 600 * 600**2 * fc_prime)
print("appendix_shear,Fs_at_24_and_50", min(0.15 * 24, 2.25 + 4.5 * 24 / 100),
      min(0.15 * 50, 2.25 + 4.5 * 50 / 100))
print("appendix_shear,rQsu1,rQsu2,rQu", 200000 * (3.33 + 0.5 * 0.006 * 345),
      200000 * (3.33 * 0.5 + 0.006 * 345), 500000)
print("appendix_shear,sQsu_solid,lacing45", 8 * 374 * 235 / sqrt(3), 1000 * 235 * sin(pi / 4))
panel = 450 * 400 * 500 * (2.664 * 3 + 0.001 * 295) + 1.2 * 12 * 400 * 350 * 235 / sqrt(3)
print("appendix_ultimate_panel,jMu", panel)
print("current_allowable_panel,Ma", 120000000 * 3 * 1.11 * 1.21)
bar_total = 8 * pi * 25**2 / 4
rank_n0 = (360000 - 8192 - bar_total) * 24 + 8192 * 235 + bar_total * 345
print("current_rank,N0,N2MN_over_N0,sM0_over_M0", rank_n0, 2000000 / rank_n0,
      302198720 / (302198720 + 0.9 * (bar_total / 2) * 345 * 537.5))
