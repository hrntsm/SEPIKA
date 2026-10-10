"""直交U/C床の独立積分 [m²]。格子・凸分割・本番Rustを使用しない。
距離二乗の区分二次式の全交点で水平切片を解析分割し、yをQUADPACK積分する。
実行: python3 polygon_integral.py（検証専用依存: scipy）
"""
import math
from scipy.integrate import quad

U = [(0,0),(12,0),(12,12),(9,12),(9,4),(3,4),(3,12),(0,12)]
C = [(0,0),(12,0),(12,2),(2,2),(2,10),(12,10),(12,12),(0,12)]


def sections(poly, y):
    xs = sorted({p[0] for p in poly})
    edges = list(zip(poly, poly[1:] + poly[:1]))
    result = [0.] * len(edges)
    crossings = sorted(a[0] for a,b in edges if a[0] == b[0] and min(a[1],b[1]) <= y < max(a[1],b[1]))
    for lo,hi in zip(xs,xs[1:]):
        x = (lo+hi)/2
        if not any(a <= x <= b for a,b in zip(crossings[::2],crossings[1::2])):
            continue
        polys = []
        for a,b in edges:
            if a[0] == b[0]:
                offset = max(min(a[1],b[1])-y,0,y-max(a[1],b[1]))
                polys.append((1.,-2.*a[0],a[0]**2+offset**2))
            else:
                left,right = sorted((a[0],b[0]))
                if x < left or x > right:
                    endpoint = left if x < left else right
                    polys.append((1.,-2.*endpoint,endpoint**2+(y-a[1])**2))
                else:
                    polys.append((0.,0.,(y-a[1])**2))
        cuts = [lo,hi]
        for i,p in enumerate(polys):
            for q in polys[i+1:]:
                a,b,c = (p[k]-q[k] for k in range(3))
                if a == 0:
                    roots = [-c/b] if b != 0 else []
                else:
                    disc = b*b-4*a*c
                    roots = [(-b-math.sqrt(disc))/(2*a),(-b+math.sqrt(disc))/(2*a)] if disc >= 0 else []
                cuts.extend(r for r in roots if lo < r < hi)
        cuts = sorted(set(cuts))
        for left,right in zip(cuts,cuts[1:]):
            mid = (left+right)/2
            vals = [a*mid*mid+b*mid+c for a,b,c in polys]
            winner = min(range(len(vals)),key=vals.__getitem__)
            # 正面積で同率になるのは距離二乗式が恒等的に等しい辺群のみ。
            group = [i for i,p in enumerate(polys) if p == polys[winner]]
            for i in group:
                result[i] += (right-left)/len(group)
    return result


if __name__ == '__main__':
    for name, poly in [('U',U),('C',C)]:
        ys = sorted({p[1] for p in poly})
        values, errors = [], []
        for edge in range(len(poly)):
            pairs = [quad(lambda y: sections(poly,y)[edge],a,b,epsabs=1e-9,epsrel=1e-10,limit=300) for a,b in zip(ys,ys[1:])]
            values.append(sum(v for v,e in pairs))
            errors.append(sum(e for v,e in pairs))
        print(name,'areas_m2=',values,'quad_estimates_m2=',errors,'total=',sum(values))
