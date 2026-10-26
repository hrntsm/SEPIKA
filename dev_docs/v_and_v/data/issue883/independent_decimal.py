from decimal import Decimal as D, getcontext
getcontext().prec = 60
pi = D('3.14159265358979323846264338327950288419716939937510582097494')
def wall(fc, te, t, depth, dc, at, fy, horizontal, axial, span, mean=False):
    d = depth - dc / 2
    pt_percent = 100 * at / (te * d)
    hoop_ratio = min(horizontal * t / te, D('.012'))
    lam = min(max(span, D(1)), D(3))
    concrete = (D('.068') if mean else D('.053')) * pt_percent ** D('.23') * (fc + 18)
    denominator = (lam + D('.12')).sqrt() if mean else lam + D('.12')
    return (concrete / denominator + D('.85') * (hoop_ratio * fy).sqrt() + D('.1') * axial) * te * D('.875') * d
print('Column: b400 D500 fc25 at2000 sy400 ag4000; Nmin=-1600000, Nbal=2000000, Nmax=6600000')
for n in [-1600000,-800000,-1,0,1,1000000,1999999,2000000,2000001,4300000,6600000]:
    n=D(n)
    if n < 0: mu=D(320000000)+D(200)*n
    elif n<=2000000: mu=D(320000000)+D(250)*n*(1-n/D(5000000))
    else: mu=D(620000000)*(D(6600000)-n)/D(4600000)
    print('N',n,'Mu_Nmm',mu)
print('Core wall: fc25 te=t200 D4000 dc0 at8000 fy250 horizontal=.004 axial=1')
for mean in [False,True]:
 for span in ['.5','1','3','4']:
    print('mean',mean,'span',span,'Qu_N',wall(*map(D,['25','200','200','4000','0','8000','250','.004','1',span]),mean))
 for horizontal in ['0','.012','.02']:
    print('mean',mean,'horizontal',horizontal,'Qu_N',wall(*map(D,['25','200','200','4000','0','8000','250',horizontal,'1','1']),mean))
print('Public wall: 300-square columns, t=te300 D4300 d4150 at=10*pi*11^2, fy295, N0, lambda1.5; vertical=.002')
for mean in [False,True]:
 for horizontal in ['.004','.012']:
    print('mean',mean,'horizontal',horizontal,'Qu_N',wall(*map(D,['24','300','300','4300','300']),10*pi*121,D(295),D(horizontal),D(0),D('1.5'),mean))
