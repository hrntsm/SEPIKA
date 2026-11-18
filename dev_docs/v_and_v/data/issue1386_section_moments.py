"""Integrate the study profiles from the Issue #1386 V&V report."""

import cmath
import math


def outline(corners, subdivisions):
    points = []
    for index, (x, y, radius) in enumerate(corners):
        vertex = complex(x, y)
        if radius == 0:
            points.append(vertex)
            continue

        previous = complex(*corners[index - 1][:2])
        following = complex(*corners[(index + 1) % len(corners)][:2])
        incoming = (vertex - previous) / abs(vertex - previous)
        outgoing = (following - vertex) / abs(following - vertex)
        turn = cmath.phase(outgoing / incoming)
        distance = radius * math.tan(abs(turn) / 2)
        entry = vertex - distance * incoming
        exit_point = vertex + distance * outgoing
        center = entry + math.copysign(radius, turn) * 1j * incoming
        radial = entry - center
        assert abs(center + radial * cmath.exp(1j * turn) - exit_point) < 1e-10
        points.extend(
            center + radial * cmath.exp(1j * turn * step / subdivisions)
            for step in range(subdivisions + 1)
        )
    return points


def moments(points):
    columns = [[] for _ in range(6)]
    for first, second in zip(points, points[1:] + points[:1]):
        x, y = first.real, first.imag
        u, v = second.real, second.imag
        cross = x * v - u * y
        triangle_factors = (
            1 / 2,
            (x + u) / 6,
            (y + v) / 6,
            (y * y + y * v + v * v) / 12,
            (x * x + x * u + u * u) / 12,
            (2 * x * y + x * v + u * y + 2 * u * v) / 24,
        )
        for column, factor in zip(columns, triangle_factors):
            column.append(cross * factor)

    area, sx, sy, ix, iy, ixy = map(math.fsum, columns)
    cx, cy = sx / area, sy / area
    ix -= area * cy**2
    iy -= area * cx**2
    ixy -= area * cx * cy
    mean = (ix + iy) / 2
    spread = math.hypot((ix - iy) / 2, ixy)
    return area, cx, cy, ix, iy, ixy, mean + spread, mean - spread


def angle(height, width, t1, t2, root_radius, toe_radius, vertical_toe_radius):
    return [
        (0, 0, 0),
        (width, 0, 0),
        (width, t2, toe_radius),
        (t1, t2, root_radius),
        (t1, height, vertical_toe_radius),
        (0, height, 0),
    ]


def channel(height, width, t1, t2, root_radius, toe_radius, slope):
    delta = (width - t1) / 2 * slope
    inner, outer = t2 + delta, t2 - delta
    return [
        (0, 0, 0),
        (width, 0, 0),
        (width, outer, toe_radius),
        (t1, inner, root_radius),
        (t1, height - inner, root_radius),
        (width, height - outer, toe_radius),
        (width, height, 0),
        (0, height, 0),
    ]


def cut_tee(height, width, t1, t2, radius):
    left, right = (width - t1) / 2, (width + t1) / 2
    under_flange = height - t2
    return [
        (left, 0, 0),
        (right, 0, 0),
        (right, under_flange, radius),
        (width, under_flange, 0),
        (width, height, 0),
        (0, height, 0),
        (0, under_flange, 0),
        (left, under_flange, radius),
    ]


def main():
    examples = {
        "L100x75, both toes": angle(100, 75, 7, 7, 10, 5, 5),
        "L200x90, both toes": angle(200, 90, 8, 14, 14, 7, 7),
        "L200x90, horizontal toe": angle(200, 90, 8, 14, 14, 7, 0),
        "C200x80, 95 degrees": channel(
            200, 80, 7.5, 11, 12, 6, math.tan(math.radians(5))
        ),
        "C200x80, parallel": channel(200, 80, 7.5, 11, 12, 6, 0),
        "CT225x200": cut_tee(225, 200, 9, 12, 13),
    }
    print("profile | A mm^2 | cx, cy mm | Ix, Iy, Ixy, Imax, Imin mm^4")
    for name, corners in examples.items():
        coarse = moments(outline(corners, 2048))
        fine = moments(outline(corners, 4096))
        change = max(abs(a - b) / max(1, abs(b)) for a, b in zip(coarse, fine))
        assert change < 2.2e-7, (name, change)
        print(name, "|", ", ".join(f"{value:.6f}" for value in fine))
        print(f"  maximum relative change: {change:.3e}")


if __name__ == "__main__":
    main()
