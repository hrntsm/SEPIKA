"""保存f64分枝の元MP式を100桁で評価する。製品の安定化式は用いない。"""
from decimal import Decimal, localcontext
import struct

# eps_r, sig_r, eps_0, sig_0, R（Rustのround-trip出力を同じf64へ戻す）。
BRANCHES = [
    (1e10, 0., (2.6e-6, .533, -48780.48780487805, -1e10, 20.)),
    (235., 0., (.002, 203.11588874077032, -.0001269568304546456, -232.91026150243204, 2.544697347360362)),
    (235., .03, (.002, 214.3574716032695, -.00026633983881175826, -250.24219535314094, 2.5948117776881645)),
]


def evaluate(strain, branch, b=.01):
    with localcontext() as ctx:
        ctx.prec = 100
        e = Decimal.from_float(strain)
        er, sr, e0, s0, r = map(Decimal.from_float, branch)
        b = Decimal.from_float(b)
        x = (e-er)/(e0-er)
        d = 1 + abs(x)**r
        root = d**(1/r)
        stress = sr + (s0-sr)*(b*x + (1-b)*x/root)
        tangent = (b + (1-b)/(d*root))*(s0-sr)/(e0-er)
        return float(stress), float(tangent)


def legacy_f64_diagnostic(strain, branch):
    er, sr, e0, s0, r = branch
    x = (strain-er)/(e0-er)
    return sr + (s0-sr)*(.01*x + .99*x/(1+abs(x)**r)**(1/r))


if __name__ == "__main__":
    for fy, a3, branch in BRANCHES:
        print(f"fy={fy}, a3={a3}")
        print("state bits:", [hex(struct.unpack('>Q', struct.pack('>d', x))[0]) for x in branch])
        for strain in [0., 1e-14, -.001, -.006]:
            print(strain, evaluate(strain, branch))
            if fy == 1e10:
                print("legacy f64 diagnostic", legacy_f64_diagnostic(strain, branch))
    for b in [0., 1e-14]:
        print("large strain", b, evaluate(1e20, (0., 0., 235./205000., 235., 20.), b))
