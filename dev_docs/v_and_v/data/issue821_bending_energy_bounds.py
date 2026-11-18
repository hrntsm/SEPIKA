"""Issue821の側柱なし平面応力板の曲げエネルギー上下界を再現する。"""

from fractions import Fraction as F


def second_moment_mm4(left_mm, right_mm, center_mm, thickness_mm):
    return thickness_mm * ((right_mm - center_mm)**3 - (left_mm - center_mm)**3) / 3


if __name__ == "__main__":
    young = F(304375, 10)
    poisson = F(1, 5)
    thickness, length, height = F(200), F(4000), F(3000)
    rotation = F(1, 10_000)
    cases = [
        ("solid", 0, 0, 0),
        ("center", 1500, 1000, 1200),
        ("left", 300, 1000, 1200),
        ("low", 1500, 1000, 1200),
        ("wide", 1000, 2000, 600),
        ("tall", 1700, 600, 2000),
    ]
    full_i = second_moment_mm4(0, length, length / 2, thickness)
    print("case,2U_lower_N_mm,2U_upper_N_mm")
    for name, x, width, hole_height in cases:
        hole_i = second_moment_mm4(x, x + width, length / 2, thickness)
        lower = young * rotation**2 / height * (full_i - hole_i)
        upper = young * rotation**2 / height * (
            full_i - F(hole_height, height) * hole_i
        ) / (1 - poisson**2)
        print(f"{name},{float(lower):.6f},{float(upper):.6f}")
