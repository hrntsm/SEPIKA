from decimal import Decimal as D, getcontext

getcontext().prec = 60
pi = D("3.14159265358979323846264338327950288419716939937510582097494")
at = 10 * pi * 11**2
cases = [
    (300, 300, ".003999", ".002"),
    (300, 300, ".004", ".002"),
    (300, 300, ".004001", ".002"),
    (300, 300, ".012", ".002"),
    (300, 300, ".02", ".005999"),
    (300, 300, ".02", ".006"),
    (300, 300, ".02", ".006001"),
    (200, 300, ".012", ".002"),
    (200, 300, ".005", ".002"),
    (200, 300, ".02", ".006"),
]
for t, te, horizontal, vertical in cases:
    t, te, horizontal, vertical = map(D, map(str, (t, te, horizontal, vertical)))
    d = D(4150)
    j = D(7) / 8 * d
    effective = min(horizontal * t / te, D(".012"), 2 * vertical)
    converted_vertical = min(horizontal * t / te, D(".012"), 2 * vertical * t / te)
    values = []
    for k, denominator in [(D(".053"), D("1.62")), (D(".068"), D("1.62").sqrt())]:
        concrete = k * (100 * at / (te * d)) ** D(".23") * 42 / denominator
        values.append((concrete + D(".85") * (effective * 295).sqrt()) * te * j)
    print(
        f"t={t} te={te} h={horizontal} v={vertical} effective={effective} "
        f"other={converted_vertical} min={values[0]:.12f} mean={values[1]:.12f}"
    )

# この幾何の静的一次参照はλ<1なので、公開準備の期待値には下限1を使う。
pte = 100 * at / (300 * D(4150))
for k, denominator in [(D(".053"), D("1.12")), (D(".068"), D("1.12").sqrt())]:
    capacity_n = (
        k * pte ** D(".23") * 42 / denominator + D(".85") * (D(".004") * 295).sqrt()
    ) * 300 * D(7) / 8 * 4150
    print("public-preparation", capacity_n)
