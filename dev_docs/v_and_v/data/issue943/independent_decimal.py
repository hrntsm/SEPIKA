from decimal import Decimal as D, localcontext

with localcontext() as context:
    context.prec = 60
    fc, te, t, wall_d, at = map(D, ('24', '200', '200', '4000', '1000'))
    horizontal, vertical, fy, span = map(D, ('.0025', '.006', '295', '1.5'))
    effective_horizontal = min(horizontal * t / te, D('.012'), 2 * vertical)
    for method, coefficient in [('min', D('.053')), ('mean', D('.068'))]:
        denominator = span + D('.12')
        if method == 'mean':
            denominator = denominator.sqrt()
        for ratio in (D('.95'), D('1')):
            effective_d = ratio * wall_d
            pte = 100 * at / (te * effective_d)
            concrete = coefficient * pte ** D('.23') * (fc + 18) / denominator
            horizontal_term = D('.85') * (effective_horizontal * fy).sqrt()
            force = (concrete + horizontal_term) * te * D('.875') * effective_d
            print(f'{method}, d={effective_d} mm, Qu={force} N')
