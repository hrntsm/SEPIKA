"""Finite SRC column comparison using section equilibrium and published formulas."""

import math

D = B = 600.0
FC, SY, F = 24.0, 345.0, 235.0
AG = 8 * math.pi * 25**2 / 4
A_STEEL = 2 * 200 * 13 + 8 * 374
ZP = (2 * 200 * 13 * (200 - 13 / 2) + 8 * 374**2 / 4,
      2 * 13 * 200**2 / 4 + 374 * 8**2 / 4)
LEVER = 475.0
RANK_N = 3.3e6
GROSS_CONCRETE = B * D * FC
C_U = GROSS_CONCRETE * (0.85 - 2.5 * (200 * 13 / (B * D)))
R_T = AG * SY
R_U = GROSS_CONCRETE + R_T
N_0 = (B * D - A_STEEL - AG) * FC + A_STEEL * F + R_T
N_U = C_U + A_STEEL * F + R_T


def single_row(n, at):
    """2025 Commentary Appendix 1-3-10 through -12, with current endpoint rule."""
    n = min(max(n, -R_T), R_U)
    base = 0.8 * at * SY * D
    if n < 0:
        return max(0, base + 0.4 * n * D)
    if n > 0.4 * GROSS_CONCRETE:
        return (base + 0.12 * GROSS_CONCRETE * D) * (R_U - n) / (R_U - 0.4 * GROSS_CONCRETE)
    return base + n * D / 2 * (1 - n / GROSS_CONCRETE)


def multiple_rows(n, g):
    """2025 Commentary Appendix 1-3-13 through -15, within its axial range."""
    nb = 0.22 * (1 + g) * GROSS_CONCRETE
    base = R_T * g * D / 2
    if n < 0:
        return base + n * g * D / 2
    if n > nb:
        at_nb = base + 0.024 * (1 + g) * (3.6 - g) * GROSS_CONCRETE * D
        return at_nb * (R_U - n) / (R_U - nb)
    return base + n * D / 2 * (1 - n / GROSS_CONCRETE)


def aij_strong_simple(n):
    # AIJ 32.1–32.9, Appendix B1–B3. Strong-axis example with symmetric
    # steel and rebar layers, both represented by the simplified component curves.
    steel_nu = A_STEEL * F
    r_nc = C_U + R_T
    r_m0 = R_T * LEVER / 2
    s_m0 = ZP[0] * F

    def rc_moment(rc_n):
        if rc_n < 0:
            return r_m0 * (1 + rc_n / R_T)
        if rc_n <= C_U:
            return r_m0 + rc_n * D / 2 * (1 - rc_n / C_U)
        return r_m0 * (r_nc - rc_n) / R_T

    def steel_moment(steel_n):
        excess = max(0, abs(steel_n) - (8 * 374 * F) / 2)
        # The line with exact sharp-corner Zp is negative near pure axial loading.
        # Fig. 32.8 ends at zero moment there.
        return max(0, s_m0 - excess * (400 - 13) / 2)

    if not -R_T - steel_nu <= n <= r_nc + steel_nu:
        return None
    rc_n = min(max(n, -R_T), r_nc)
    steel_n = min(max(n, -steel_nu), steel_nu)
    return (
        rc_moment(rc_n) + steel_moment(n - rc_n),
        steel_moment(steel_n) + rc_moment(n - steel_n),
    )


def print_comparison():
    print(f"steel_area={A_STEEL} rebar_area={AG:.6f}")
    print(f"rN0_App13={R_U / 1000:.6f} rNt_App13={-R_T / 1000:.6f}")
    print(f"N0_current={N_0 / 1000:.6f} Nuc_AIJ32={N_U / 1000:.6f}")
    print(f"rank_N_N0_current={RANK_N/N_0:.9f} rank_N_Nuc_example={RANK_N/N_U:.9f}")
    for use_f in (100.0, F):
        n0 = N_0 + A_STEEL * (use_f - F)
        sm0 = ZP[0] * use_f
        rm0 = 0.9 * (AG / 2) * SY * (D + LEVER) / 2
        print(f"steel_F={use_f:.0f} at_N_3400 N_N0={3.4e6/n0:.9f} sM0_M0={sm0/(sm0+rm0):.9f}")

    for name, zp, at, g in zip(("strong", "weak"), ZP, (AG / 2, AG / 4),
                               (LEVER / D, (2 * LEVER / 3) / D)):
        sm = zp * F
        rank_current = sm / (sm + 0.9 * at * SY * (D + LEVER) / 2)
        # AIJ Fig. 32.9 gives the weak-axis web axial resistance only.
        sm_aij = sm if name == "strong" else (2 * 13 * 200**2 / 4) * F
        # AIJ付表B2・解説図32.5では中間主筋を断面中央に集約し、軸力抵抗のみに算入する。
        m_bar_aij = at * SY * LEVER
        print(f"{name} sM0_exact={sm / 1e6:.6f} sM0_AIJ={sm_aij / 1e6:.6f}"
              f" rebar_M0_B2={m_bar_aij / 1e6:.6f} M0_32_1={(sm_aij + m_bar_aij) / 1e6:.6f}"
              f" ratio_32_1={sm_aij / (sm_aij + m_bar_aij):.9f}"
              f" ratio_current_axis_formula={rank_current:.9f}")
        print("N[kN] rMu_current[kNm] rMu_multi[kNm] Qr_current[kN] Qr_multi[kN]")
        for n in (-R_T, -0.9 * R_T, 0, 3.3e6, R_U, R_U + 100_000):
            current = single_row(n, at)
            multi = multiple_rows(min(n, R_U), g)
            print(*(f"{value:.6f}" for value in (n / 1000, current / 1e6,
                                                multi / 1e6, current / 2e6, multi / 2e6)))
        print(f"current first zero in tension: {-2*at*SY/1000:.6f} kN")

    # Compare the two permitted simple accumulation prescriptions for strong-axis bending.
    n_tension = -R_T - A_STEEL * F
    print("AIJ strong simple: N[kN] rc_first[kNm] steel_first[kNm]")
    for n in (n_tension-1000, n_tension, -2e6, 0, 3.3e6, 9e6, N_U, N_U+1000):
        m = aij_strong_simple(n)
        values = ("out of domain",) if m is None else tuple(f"{v / 1e6:.6f}" for v in m)
        print(f"{n/1000:.6f}", *values)

    # A finite endpoint case in the strong axis, both ends with 3.3 MN compression.
    # Appendix 1-4-1, other than top storey: two beams, each sMa+rMu=450 kN m.
    steel_z = (200 * 400**3 - (200 - 8) * 374**3) / (12 * (400 / 2))
    sm_short = steel_z * F
    rm = single_row(RANK_N, AG / 2)
    rm1 = min((2 * 450e6) / 2 - sm_short, rm)
    print(f"column_head_current_Qr={2*rm/4e6:.6f}"
          f" column_head_App14_Qr={(rm1+rm)/4e6:.6f}")
    n_bottom = 1e6
    print(f"distinct_ends_App14_Qr={(rm+single_row(n_bottom,AG/2))/4e6:.6f}"
          f" at_N_1000_current_Qr={2*single_row(n_bottom,AG/2)/4e6:.6f}")

    def current_ratio(n, q):
        ratios = []
        for depth, width, at, aw, perpendicular in (
            (400, 200, AG / 2, 8 * 374, 200),
            (200, 400, AG / 4, 4 * 200 * 13 / 3, 400),
        ):
            if depth == 400:
                z = steel_z
            else:
                z = (2 * 13 * 200**3 + 374 * 8**3) / (12 * (200 / 2))
            s_q = 2 * F * z / 4000
            r_m = single_row(n, at)
            r_q = 2 * r_m / 4000 if r_m > 0 else max(0, 2 * (q - s_q))
            j = 7 * (D + LEVER) / 16
            pw = 2 * math.pi * 10**2 / 4 / (B * 100)
            fs = 1.095
            r_qa = j * min(B * (fs + 0.5 * pw * 295),
                           2 * (B - perpendicular) * fs + B * pw * 295)
            ratios.append(max(s_q / (aw * F / math.sqrt(3)), r_q / r_qa))
        return max(ratios)

    for n in (-0.9 * R_T, 0, RANK_N, R_U, R_U + 100_000):
        print(f"input_N={n:.9f} Qd1_ratio={current_ratio(n, 800e3):.12f}")


if __name__ == "__main__":
    print_comparison()
