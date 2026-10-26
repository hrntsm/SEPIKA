"""Issue821: 側柱なし平面応力弾性体の軸剛性上界を有理数で再現する。"""

from fractions import Fraction as F


def axial_bound(young_n_mm2, poisson, thickness_mm, length_mm, height_mm, hole_mm2):
    if not (young_n_mm2 > 0 and -1 < poisson < F(1, 2)):
        raise ValueError("正のヤング係数と安定な等方弾性係数が必要")
    if not (thickness_mm > 0 and length_mm > 0 and height_mm > 0):
        raise ValueError("正の幾何寸法が必要")
    net_mm2 = length_mm * height_mm - hole_mm2
    if not (0 <= hole_mm2 < length_mm * height_mm):
        raise ValueError("開口面積は外形面積未満が必要")
    return young_n_mm2 * thickness_mm * net_mm2 / ((1 - poisson**2) * height_mm**2)


if __name__ == "__main__":
    # このEは現行比較体の均質材料仮定。実RCの均質化を検証するものではない。
    young = F(304375, 10)
    thickness, length, height = F(200), F(4000), F(3000)
    full_wall_k = young * thickness * length / height
    load_n = F(100000)
    print(f"current_EA_H_N_mm={float(full_wall_k):.12f}")
    print(f"current_uz_mm={float(load_n / full_wall_k):.12f}")
    print("nu,hole_mm2,K_upper_N_mm,K_upper_over_current,uz_lower_mm")
    for poisson in [F(0), F(1, 5)]:
        for hole in [F(0), F(1200000)]:
            upper = axial_bound(young, poisson, thickness, length, height, hole)
            print(f"{float(poisson):.1f},{hole},{float(upper):.12f},"
                  f"{upper / full_wall_k},{float(load_n / upper):.12f}")
