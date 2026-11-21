"""連成剛床の立体静力学と軸降伏機構を照合する調査用計算。単位は kN, m, rad。"""

import argparse
import csv
from itertools import combinations, product
from pathlib import Path

import numpy as np

HEIGHT_M = 3.6
WEIGHTS_KN = np.array([140.0, 120.0, 340.0])
C0_PRIMARY, C0_ULTIMATE = 0.2, 1.0
Z, RT, AI = 1.0, 1.0, 1.0
PRIMARY_KN = C0_PRIMARY * Z * RT * AI * WEIGHTS_KN.sum()
QUD_KN = C0_ULTIMATE * Z * RT * AI * WEIGHTS_KN.sum()
DS_COMPARISON = 0.45
E_KN_M2 = 200e6
TOP_XY_M = np.array([[0.0, 0.0], [6.0, 0.0], [0.0, 6.0]])
AXIAL_LIMITS_KN = np.array([
    [-180., 260.], [-90., 115.],
    [-230., 150.], [-130., 85.],
    [-160., 210.], [-75., 120.],
])
MEMBER_NAMES = ("A1", "A2", "B1", "B2", "C1", "C2")
GRAVITY_CENTER_M = WEIGHTS_KN @ TOP_XY_M / WEIGHTS_KN.sum()
TOL = 1e-8
POSE_CASES = ("free", "transverse_fixed", "rotation_fixed", "both_fixed")


def rotate90(vector_xy):
    return np.array([-vector_xy[1], vector_xy[0]])


def direction_deg(angle_deg):
    radians = np.radians(angle_deg)
    return np.array([np.cos(radians), np.sin(radians)])


def axis_deg(vector_xy):
    return float(np.degrees(np.arctan2(vector_xy[1], vector_xy[0])) % 180)


def free_pose_basis(direction_xy, case):
    if case == "free":
        return np.eye(3)
    d = np.r_[direction_xy, 0.]
    r = np.array([0., 0., 1.])
    if case == "transverse_fixed":
        return np.column_stack((d, r))
    if case == "rotation_fixed":
        return np.eye(3)[:, :2]
    if case == "both_fixed":
        return d[:, None]
    raise ValueError(case)


class Frame:
    def __init__(self, name, rotation_deg):
        self.name = name
        self.upper_xyz = np.column_stack((TOP_XY_M, np.full(3, HEIGHT_M)))
        bearings = np.tile([rotation_deg, rotation_deg + 90], 3)
        plan = np.array([direction_deg(angle) for angle in bearings])
        self.lower_xyz = self.upper_xyz.repeat(2, axis=0) - HEIGHT_M * np.column_stack(
            (plan, np.ones(6))
        )
        members_xyz = self.upper_xyz.repeat(2, axis=0) - self.lower_xyz
        lengths_m = np.linalg.norm(members_xyz, axis=1)
        self.member_axes = members_xyz / lengths_m[:, None]
        self.axial_k = np.tile([36000., 6400.], 3)
        self.area_m2 = self.axial_k * lengths_m / E_KN_M2

        self.floor_to_nodes = np.vstack([
            np.column_stack((np.eye(2), rotate90(point_xy - GRAVITY_CENTER_M)))
            for point_xy in TOP_XY_M
        ])
        self.node_to_bar = np.zeros((6, 6))
        for bar, axis in enumerate(self.member_axes):
            node = bar // 2
            self.node_to_bar[bar, 2 * node:2 * node + 2] = axis[:2]
        self.floor_to_bar = self.node_to_bar @ self.floor_to_nodes

        self.floor_k = self.floor_to_bar.T @ (self.axial_k[:, None] * self.floor_to_bar)
        self.h = self.floor_k[:2, :2]
        self.center_xy = GRAVITY_CENTER_M - rotate90(np.linalg.solve(self.h, self.floor_k[:2, 2]))
        self.kr = self.floor_k[2, 2] - self.floor_k[2, :2] @ np.linalg.solve(
            self.h, self.floor_k[:2, 2]
        )
        assert self.kr > 0 and np.min(np.linalg.eigvalsh(self.h)) > 0
        np.testing.assert_allclose(self.center_xy, TOP_XY_M.mean(axis=0), atol=TOL, rtol=0)
        for direction in np.eye(2):
            torque = rotate90(self.center_xy - GRAVITY_CENTER_M) @ direction
            response = np.linalg.solve(self.floor_k, np.r_[direction, torque])
            np.testing.assert_allclose(response[2], 0, atol=TOL, rtol=0)
        twist = np.linalg.solve(self.floor_k, [0., 0., self.kr])
        np.testing.assert_allclose(
            twist[:2] + rotate90(self.center_xy - GRAVITY_CENTER_M), 0, atol=TOL, rtol=0
        )
        np.testing.assert_allclose(twist[2], 1, atol=TOL, rtol=0)
        if rotation_deg == 0:
            np.testing.assert_allclose(self.h, np.diag([54000., 9600.]), atol=TOL, rtol=0)
            np.testing.assert_allclose(self.kr, 240000. + 268800., atol=TOL, rtol=0)
        self.translation_angles = [
            axis_deg(vector) for vector in np.linalg.eigh(self.h)[1].T
        ]
        self.work_angles = [
            axis_deg(vector) for vector in np.linalg.eigh(np.linalg.inv(self.floor_k)[:2, :2])[1].T
        ]
        self.node_k = self.node_to_bar.T @ (self.axial_k[:, None] * self.node_to_bar)
        self.rigid_constraints = np.linalg.svd(self.floor_to_nodes.T)[2][3:, :]

    def elastic(self, direction_xy, case):
        load = PRIMARY_KN * np.r_[direction_xy, 0.]
        basis = free_pose_basis(direction_xy, case)
        pose = basis @ np.linalg.solve(basis.T @ self.floor_k @ basis, basis.T @ load)
        axial = self.axial_k * (self.floor_to_bar @ pose)
        reaction = self.floor_k @ pose - load

        # 床の一般化座標を用いない節点釣合いでも、節点変位と軸力を照合する。
        locked = np.linalg.svd(basis.T)[2][basis.shape[1]:, :]
        extra_constraints = locked @ np.linalg.pinv(self.floor_to_nodes)
        constraints = np.vstack((self.rigid_constraints, extra_constraints))
        system = np.block([
            [self.node_k, constraints.T],
            [constraints, np.zeros((len(constraints), len(constraints)))],
        ])
        nodal_load = (WEIGHTS_KN[:, None] / WEIGHTS_KN.sum() * load[:2]).ravel()
        rhs = np.r_[nodal_load, np.zeros(len(constraints))]
        nodes_xy = np.linalg.solve(system, rhs)[:6]
        np.testing.assert_allclose(nodes_xy, self.floor_to_nodes @ pose, atol=TOL, rtol=0)
        np.testing.assert_allclose(self.axial_k * (self.node_to_bar @ nodes_xy), axial, atol=TOL, rtol=0)

        force_xyz = axial[:, None] * self.member_axes
        arms_xyz = self.upper_xyz.repeat(2, axis=0) - np.r_[GRAVITY_CENTER_M, HEIGHT_M]
        force = force_xyz.sum(axis=0)
        torque = np.cross(arms_xyz, force_xyz).sum(axis=0)
        balance = np.r_[force[:2], torque[2]] - reaction
        np.testing.assert_allclose(balance, load, atol=TOL, rtol=0)
        base_moment = np.cross(
            self.lower_xyz - np.r_[GRAVITY_CENTER_M, HEIGHT_M], force_xyz
        ).sum(axis=0)
        np.testing.assert_allclose(base_moment, torque, atol=TOL, rtol=0)
        assert np.all(axial > AXIAL_LIMITS_KN[:, 0]) and np.all(axial < AXIAL_LIMITS_KN[:, 1])
        return pose, axial, reaction, force_xyz

    def design_comparison(self, direction_xy, case):
        n = rotate90(direction_xy)
        compare_cases = case in ("rotation_fixed", "both_fixed")
        if not compare_cases:
            return {}
        if case == "rotation_fixed":
            k = 1 / (direction_xy @ np.linalg.solve(self.h, direction_xy))
        else:
            k = direction_xy @ self.h @ direction_xy
        eccentric_m = abs(n @ (GRAVITY_CENTER_M - self.center_xy))
        radius_m = np.sqrt(self.kr / k)
        re = eccentric_m / radius_m
        fe = 1 + 0.5 * np.clip((re - 0.15) / 0.15, 0, 1)
        return dict(k_kn_m=k, e_m=eccentric_m, re_radius_m=radius_m, re=re,
                    fes=fe, qun_comparison_kn=DS_COMPARISON * fe * QUD_KN)


def kinematic_limit(rows, load):
    dimension = len(load)
    if dimension == 1:
        rays = [np.ones(1)]
    elif dimension == 2:
        rays = [rotate90(row) for row in rows]
    else:
        rays = [np.cross(a, b) for a, b in combinations(rows, 2)]
    certificates = []
    for ray in rays:
        if abs(load @ ray) < TOL:
            continue
        mechanism = ray / (load @ ray)
        rates = rows @ mechanism
        work = np.maximum(AXIAL_LIMITS_KN[:, 0] * rates, AXIAL_LIMITS_KN[:, 1] * rates).sum()
        certificates.append((work, mechanism))
    return min(certificates, key=lambda item: item[0])


def static_limit(rows, load):
    members = len(rows)
    best_capacity = 0.
    best_axial = None
    for free in combinations(range(members), len(load) - 1):
        free = np.array(free, dtype=int)
        fixed = np.array([bar for bar in range(members) if bar not in free])
        lhs = np.column_stack((rows[free].T, -load))
        if np.linalg.matrix_rank(lhs) != len(load):
            continue
        extremes = np.array(list(product(*AXIAL_LIMITS_KN[fixed])))
        vertices = np.linalg.solve(lhs, -(rows[fixed].T @ extremes.T)).T
        for values, ends in zip(vertices, extremes, strict=True):
            axial = np.zeros(members)
            axial[fixed] = ends
            axial[free] = values[:-1]
            if (
                values[-1] > best_capacity
                and np.all(axial >= AXIAL_LIMITS_KN[:, 0] - TOL)
                and np.all(axial <= AXIAL_LIMITS_KN[:, 1] + TOL)
            ):
                best_capacity, best_axial = values[-1], axial
    assert best_axial is not None
    return best_capacity, best_axial


def collapse(frame, direction_xy, case):
    basis = free_pose_basis(direction_xy, case)
    unit_load = np.r_[direction_xy, 0.]
    rows = frame.floor_to_bar @ basis
    load = basis.T @ unit_load
    upper_kn, mechanism = kinematic_limit(rows, load)
    lower_kn, axial = static_limit(rows, load)
    np.testing.assert_allclose(lower_kn, upper_kn, atol=TOL, rtol=0)
    np.testing.assert_allclose(rows.T @ axial, lower_kn * load, atol=TOL, rtol=0)
    rates = rows @ mechanism
    active = []
    for bar, (name, rate) in enumerate(zip(MEMBER_NAMES, rates, strict=True)):
        if abs(rate) <= TOL:
            continue
        end = 1 if rate > 0 else 0
        np.testing.assert_allclose(axial[bar], AXIAL_LIMITS_KN[bar, end], atol=TOL, rtol=0)
        active.append(name + ("+" if rate > 0 else "-"))
    return lower_kn, axial, basis @ mechanism, rates, " ".join(active)


def pushover(frame, direction_xy, case):
    basis = free_pose_basis(direction_xy, case)
    rows = frame.floor_to_bar @ basis
    load = basis.T @ np.r_[direction_xy, 0.]
    lower, upper = AXIAL_LIMITS_KN.T
    axial = np.zeros(len(rows))
    pose = np.zeros(len(load))
    shear = 0.
    events = []
    for _ in range(24):
        bound_sign = np.where(
            np.abs(axial - upper) < TOL, 1, -1 * (np.abs(axial - lower) < TOL)
        )
        at_bound = np.flatnonzero(bound_sign)
        for locked in product((True, False), repeat=len(at_bound)):
            tangent_k = frame.axial_k.copy()
            tangent_k[at_bound[np.array(locked, dtype=bool)]] = 0
            tangent = rows.T @ (tangent_k[:, None] * rows)
            if np.linalg.matrix_rank(tangent, tol=TOL) != len(load):
                continue
            pose_rate = np.linalg.solve(tangent, load)
            strains = rows @ pose_rate
            trials = frame.axial_k[at_bound] * strains[at_bound] * bound_sign[at_bound]
            admissible = np.where(locked, trials >= -TOL, trials <= TOL)
            if np.all(admissible):
                break
        else:
            return shear, basis @ pose, axial, events

        axial_rate = tangent_k * strains
        change = np.full(len(rows), np.inf)
        toward_upper = axial_rate > TOL
        toward_lower = axial_rate < -TOL
        change[toward_upper] = (upper[toward_upper] - axial[toward_upper]) / axial_rate[toward_upper]
        change[toward_lower] = (lower[toward_lower] - axial[toward_lower]) / axial_rate[toward_lower]
        step = np.min(change)
        assert np.isfinite(step) and step > TOL, (frame.name, direction_xy, case, shear, step)
        shear += step
        axial += step * axial_rate
        pose += step * pose_rate
        np.testing.assert_allclose(rows.T @ axial, shear * load, atol=TOL, rtol=0)
        assert np.all(axial >= lower - TOL) and np.all(axial <= upper + TOL)
        reached = np.where(np.abs(change - step) <= TOL)[0]
        events.append(" ".join(MEMBER_NAMES[i] + ("+" if axial_rate[i] > 0 else "-") for i in reached))
    raise AssertionError("降伏イベントの上限回数を超過")


def rows_for_direction(frame, label, angle_deg, cases):
    for sign in (1, -1):
        d = sign * direction_deg(angle_deg)
        n = rotate90(d)
        for case in cases:
            pose, axial, reaction, forces = frame.elastic(d, case)
            qu, limit_axial, mechanism, rates, active = collapse(frame, d, case)
            displacements = (frame.floor_to_nodes @ pose).reshape(3, 2)
            mechanism_nodes = (frame.floor_to_nodes @ mechanism).reshape(3, 2)
            row = dict(
                model=frame.name, direction=label, axis_deg=angle_deg, sign=sign, case=case,
                primary_kn=PRIMARY_KN, q_ud_kn=QUD_KN, rs=1.0, fs=1.0, ds_comparison=DS_COMPARISON,
                drift_parallel_mm=1000 * (d @ pose[:2]),
                drift_transverse_mm=1000 * (n @ pose[:2]),
                drift_angle=(d @ pose[:2]) / HEIGHT_M,
                theta_mrad=1000 * pose[2],
                transverse_reaction_kn=n @ reaction[:2],
                rotation_reaction_kn_m=reaction[2],
                qu_kn=qu, yielding_mechanism=active,
                mechanism_transverse_m_m=n @ mechanism[:2],
                mechanism_theta_rad_m=mechanism[2],
            )
            for index, name in enumerate(("A", "B", "C")):
                row[f"{name}_ux_mm"], row[f"{name}_uy_mm"] = 1000 * displacements[index]
                row[f"{name}_mechanism_ux_m_m"], row[f"{name}_mechanism_uy_m_m"] = mechanism_nodes[index]
                row[f"{name}_guide_seismic_z_kn"] = forces[index * 2:index * 2 + 2, 2].sum()
            for index, name in enumerate(MEMBER_NAMES):
                row[f"{name}_axial_kn"] = axial[index]
                row[f"{name}_axial_stress_mpa"] = axial[index] / frame.area_m2[index] / 1000
                row[f"{name}_fx_kn"], row[f"{name}_fy_kn"], row[f"{name}_fz_kn"] = forces[index]
                row[f"{name}_collapse_axial_kn"] = limit_axial[index]
                row[f"{name}_mechanism_rate_m_m"] = rates[index]
            row.update(frame.design_comparison(d, case))
            if "k_kn_m" in row:
                np.testing.assert_allclose(
                    1000 * PRIMARY_KN / row["drift_parallel_mm"], row["k_kn_m"], atol=TOL, rtol=0
                )
            if label != "sweep":
                peak_kn, peak_pose, peak_axial, events = pushover(frame, d, case)
                np.testing.assert_allclose(peak_kn, qu, atol=TOL, rtol=0)
                for rate, bar_force, limits in zip(rates, peak_axial, AXIAL_LIMITS_KN, strict=True):
                    if abs(rate) > TOL:
                        np.testing.assert_allclose(bar_force, limits[int(rate > 0)], atol=TOL, rtol=0)
                row.update(
                    peak_parallel_mm=1000 * (d @ peak_pose[:2]),
                    peak_transverse_mm=1000 * (n @ peak_pose[:2]),
                    peak_theta_mrad=1000 * peak_pose[2],
                    yield_sequence=" -> ".join(events),
                )
                for index, name in enumerate(MEMBER_NAMES):
                    row[f"{name}_peak_axial_kn"] = peak_axial[index]
            yield row


def write_rows(path, rows):
    if not rows:
        return
    fields = list(dict.fromkeys(key for row in rows for key in row))
    with path.open("w", encoding="utf-8", newline="") as file:
        writer = csv.DictWriter(file, fields)
        writer.writeheader()
        writer.writerows(rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    detailed, sweep, input_rows = [], [], []
    for frame in (Frame("coupled", 35.), Frame("orthogonal", 0.)):
        print(
            f"{frame.name}: H={frame.h.tolist()}, C={frame.center_xy.tolist()}, KR={frame.kr:.9f}"
        )
        print(f"  並進（弱・強）={frame.translation_angles}°, 仕事（小・大）={frame.work_angles}°")
        for index, (name, low, high, area, k, limits) in enumerate(zip(
            MEMBER_NAMES, frame.lower_xyz, frame.upper_xyz.repeat(2, axis=0),
            frame.area_m2, frame.axial_k, AXIAL_LIMITS_KN, strict=True,
        )):
            input_rows.append(dict(model=frame.name, member=name,
                                   base_x_m=low[0], base_y_m=low[1], base_z_m=low[2],
                                   top_x_m=high[0], top_y_m=high[1], top_z_m=high[2],
                                   top_weight_kn=WEIGHTS_KN[index // 2],
                                   area_mm2=area*1e6, axial_k_kn_m=k,
                                   compression_kn=limits[0], tension_kn=limits[1]))

        directions = (("X", 0.), ("Y", 90.), ("diagonal_45", 45.), ("diagonal_135", 135.),
                      *zip(("H_weak", "H_strong"), frame.translation_angles, strict=True),
                      *zip(("W_min", "W_max"), frame.work_angles, strict=True))
        for label, angle in directions:
            detailed.extend(rows_for_direction(frame, label, angle, POSE_CASES))
        for angle in range(180):
            for row in rows_for_direction(frame, "sweep", angle, ("free",)):
                sweep.append({key: row[key] for key in (
                    "model", "axis_deg", "sign", "drift_parallel_mm", "drift_transverse_mm",
                    "theta_mrad", "qu_kn", "yielding_mechanism"
                )})
    write_rows(args.output / "input.csv", input_rows)
    write_rows(args.output / "directions.csv", detailed)
    write_rows(args.output / "sweep.csv", sweep)
    print(f"節点釣合い・立体力の釣合い・塑性上下界の照合 {len(detailed) + len(sweep)} ケース完了")
    print(f"単調増分解析の終局耐力の照合 {len(detailed)} ケース完了")


if __name__ == "__main__":
    main()
