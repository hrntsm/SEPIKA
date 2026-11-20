from math import atan2, cos, degrees, radians, sin, sqrt

import numpy as np


def coupled_story(positions_mm):
    e_n_mm2, g_n_mm2 = 200_000, 80_000
    area_mm2, shear_area_mm2 = 2_000, 1_600
    inertia_mm4, torsion_constant_mm4 = 8_000_000, 4_000_000
    ei = e_n_mm2 * inertia_mm4
    stiffness = np.zeros((3, 3))
    for (x, y), axis_mm in zip(
        positions_mm, ((2_400, 3_200, 3_000), (0, 0, 3_000), (0, 0, 3_000))
    ):
        length_mm = np.linalg.norm(axis_mm)
        axis = np.array(axis_mm) / length_mm
        flexible = np.array([
            [length_mm**3 / (3 * ei) + length_mm / (g_n_mm2 * shear_area_mm2),
             length_mm**2 / (2 * ei)],
            [length_mm**2 / (2 * ei), length_mm / ei],
        ])
        shear_bending = np.linalg.inv(flexible)

        translation = np.array([[1, 0, -y], [0, 1, x], [0, 0, 0]])
        vertical_rotation = np.array([0, 0, 1])
        axial = axis @ translation
        transverse = translation - np.outer(axis, axial)
        slope = np.outer(np.cross(vertical_rotation, axis), vertical_rotation)
        twist = axis[2] * vertical_rotation
        stiffness += (
            e_n_mm2 * area_mm2 / length_mm * np.outer(axial, axial)
            + shear_bending[0, 0] * transverse.T @ transverse
            + shear_bending[0, 1] * (transverse.T @ slope + slope.T @ transverse)
            + shear_bending[1, 1] * slope.T @ slope
            + g_n_mm2 * torsion_constant_mm4 / length_mm * np.outer(twist, twist)
        )
    return stiffness


def orthogonal_story(positions_mm):
    stiffness = np.zeros((3, 3))
    for x, y in positions_mm:
        motion = np.array([[1, 0, -y], [0, 1, x]])
        stiffness += motion.T @ np.diag([1_000, 2_000]) @ motion
    return stiffness


def direction(angle_deg):
    angle = radians(angle_deg)
    return np.array([cos(angle), sin(angle)])


def principal_angle(matrix):
    return degrees(atan2(2 * matrix[0, 1], matrix[0, 0] - matrix[1, 1])) / 2


def compare(stiffness, gravity_center_mm, angles_deg):
    horizontal = stiffness[:2, :2]
    coupling = stiffness[:2, 2]
    shift = np.linalg.solve(horizontal, coupling)
    rigidity_center_mm = np.array([shift[1], -shift[0]])
    kr = stiffness[2, 2] - coupling @ shift
    offset = gravity_center_mm - rigidity_center_mm
    arm = np.array([-offset[1], offset[0]])
    flexibility = np.linalg.inv(horizontal)
    mass_center_flexibility = flexibility + np.outer(arm, arm) / kr
    stiffness_axis = principal_angle(horizontal)
    work_axis = principal_angle(-mass_center_flexibility)

    assert np.all(np.linalg.eigvalsh(stiffness) > 0)
    print("層剛性\n", stiffness)
    print(f"剛心 {rigidity_center_mm}, KR = {kr:.6f}")
    print(f"並進主軸 {stiffness_axis:.6f}°, 最小仕事の主軸 {work_axis:.6f}°")
    print("角度 [deg] | 境界条件 | k [N/mm] | e [mm] | re [mm] | Re | Fes | Qun [kN]")
    for angle_deg in (*angles_deg, stiffness_axis, stiffness_axis + 90, work_axis, work_axis + 90):
        axis = direction(angle_deg)
        e_mm = abs(axis @ arm)
        constrained = axis @ horizontal @ axis
        force_free = 1 / (axis @ flexibility @ axis)
        assert force_free <= constrained * (1 + 1e-12)
        for label, stiffness_n_mm in (("変位ゼロ", constrained), ("外力ゼロ", force_free)):
            radius_mm = sqrt(kr / stiffness_n_mm)
            re = e_mm / radius_mm
            fes = 1 + 0.5 * min(1, max(0, (re - 0.15) / (0.30 - 0.15)))
            qun_kn = 0.30 * fes * 1_000
            print(f"{angle_deg:10.6f} | {label} | {stiffness_n_mm:12.6f} | {e_mm:11.6f} | "
                  f"{radius_mm:11.6f} | {re:.6f} | {fes:.6f} | {qun_kn:.6f}")
    return rigidity_center_mm, kr


positions_mm = ((0, 0), (6_000, 0), (0, 6_000))
np.set_printoptions(precision=8)
print("#1596 連成3柱")
rigidity_center_mm, kr = compare(coupled_story(positions_mm), np.array([2_000, 2_000]), (0, 90, 20, 50))
np.testing.assert_allclose(rigidity_center_mm, [2_166.687027709256, 2_862.348055979125], rtol=1e-10)
np.testing.assert_allclose(kr, 31_403_342_934.89614, rtol=1e-10)

print("\n非連成対照")
rigidity_center_mm, kr = compare(orthogonal_story(positions_mm), np.array([1_100, 3_250]), (0, 90))
np.testing.assert_allclose(rigidity_center_mm, [2_000, 2_000], rtol=1e-12)
np.testing.assert_allclose(kr, 72_000_000_000, rtol=1e-12)
