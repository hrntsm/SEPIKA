use super::{comp_resultant, distributed_intensity, resolve, Comp};
use crate::frame::beam::MemberForces;
use crate::transform::LocalFrame;
use sepika_core::model::{MemberLoad, MemberLoadKind};

/// i 端から j 端へ進む向きでの断面の切断側。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionSide {
    Left,
    Right,
    Continuous,
}

/// 材軸上の位置 `[mm]`、切断側、符号付きせん断力 `[N]`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameShearPeak {
    pub x_mm: f64,
    pub side: SectionSide,
    pub q_n: f64,
}

/// 実材軸 i 端からの区間 `[mm]` の最大絶対せん断力を局所 y、z の順で返す。
/// `forces` は同じ線形弾性線材・係数・実荷重で回収した節点芯を含む応答を要する。
/// 集中荷重位置では区間内側の極限を用い、応答と荷重が不整合な方向はエラーを返す。
pub fn frame_shear_envelope(
    frame: &LocalFrame,
    length_mm: f64,
    [start_mm, end_mm]: [f64; 2],
    loads: &[(f64, &MemberLoad)],
    forces: &MemberForces,
) -> Result<[Result<FrameShearPeak, String>; 2], String> {
    if ![length_mm, start_mm, end_mm].iter().all(|x| x.is_finite())
        || start_mm < 0.0
        || end_mm > length_mm
        || start_mm >= end_mm
    {
        return Err("材端フェース間の区間が不正です".into());
    }
    let mut samples = forces.at.iter();
    let Some((0.0, i_forces)) = samples.next() else {
        return Err("i端節点芯の断面力がありません".into());
    };
    if forces.at.last().is_none_or(|(xi, _)| *xi != 1.0)
        || forces
            .at
            .windows(2)
            .any(|pair| pair[1].0.partial_cmp(&pair[0].0) != Some(std::cmp::Ordering::Greater))
    {
        return Err("j端節点芯または断面力の評価位置が不足・不正です".into());
    }

    let mut directions: [Vec<(f64, Comp)>; 2] = std::array::from_fn(|_| Vec::new());
    for &(factor, load) in loads {
        load.validate_direction()?;
        load.validate_extent(length_mm)?;
        let valid = match load.kind {
            MemberLoadKind::Point { p, .. } => p.is_finite(),
            MemberLoadKind::Distributed { w1, w2, .. } => w1.is_finite() && w2.is_finite(),
        };
        if !factor.is_finite() || !valid {
            return Err("部材荷重の係数・位置・区間・強度が不正です".into());
        }
        for (direction, comp) in directions
            .iter_mut()
            .zip(resolve(load, frame).into_iter().skip(1))
        {
            if let Some(comp) = comp {
                direction.push((factor, comp));
            }
        }
    }

    Ok(std::array::from_fn(|axis| {
        let component = axis + 1;
        direction_peak(
            length_mm,
            start_mm,
            end_mm,
            &directions[axis],
            i_forces[component],
            forces.at.iter().map(|&(xi, f)| (xi, f[component])),
        )
    }))
}

fn direction_peak(
    length_mm: f64,
    start_mm: f64,
    end_mm: f64,
    loads: &[(f64, Comp)],
    qi: f64,
    samples: impl Iterator<Item = (f64, f64)>,
) -> Result<FrameShearPeak, String> {
    let tolerance_mm = 32.0 * f64::EPSILON * length_mm.max(1.0);
    if loads.iter().any(|&(factor, comp)| {
        matches!(comp, Comp::Point { a, p } if factor != 0.0 && p != 0.0
            && [start_mm, end_mm].iter().any(|&edge| a != edge && (a - edge).abs() <= tolerance_mm))
    }) {
        return Err("材端フェースと直近の集中荷重の位置関係を確認できません".into());
    }
    let resultant = |x, right| {
        loads.iter().map(move |(factor, comp)| {
            factor
                * match *comp {
                    Comp::Point { a, p } if a == x && right => p,
                    _ => comp_resultant(comp, 0.0, x),
                }
        })
    };
    for (xi, q) in samples {
        let (calculated, scale) = resultant(xi * length_mm, false).fold(
            (qi, qi.abs().max(q.abs()).max(1.0)),
            |(sum, scale), force| (sum + force, scale + force.abs()),
        );
        if !q.is_finite()
            || !calculated.is_finite()
            || !scale.is_finite()
            || (calculated - q).abs() > 1e-9 * scale
        {
            return Err("保存断面力と部材荷重のせん断釣合いを確認できません".into());
        }
    }

    let mut points = vec![start_mm, end_mm];
    for &(_, comp) in loads {
        let bounds = match comp {
            Comp::Point { a, .. } => [a, a],
            Comp::Dist { a, b, .. } => [a, b],
        };
        points.extend(bounds.into_iter().filter(|&x| start_mm < x && x < end_mm));
    }
    points.sort_by(f64::total_cmp);
    points.dedup();

    let mut peak: Option<FrameShearPeak> = None;
    let mut consider = |x_mm, side| {
        let q_n = resultant(x_mm, side == SectionSide::Right).fold(qi, |q, load| q + load);
        if !q_n.is_finite() {
            return Err("連続位置のせん断力が非有限です".to_string());
        }
        if peak.is_none_or(|previous| q_n.abs() > previous.q_n.abs()) {
            peak = Some(FrameShearPeak { x_mm, side, q_n });
        }
        Ok(())
    };
    for (index, &x) in points.iter().enumerate() {
        let jump: f64 = loads
            .iter()
            .filter_map(|&(factor, comp)| match comp {
                Comp::Point { a, p } if a == x => Some(factor * p),
                _ => None,
            })
            .sum();
        if !jump.is_finite() {
            return Err("集中荷重の合力が非有限です".into());
        }
        if jump == 0.0 {
            consider(x, SectionSide::Continuous)?;
        } else {
            if x > start_mm {
                consider(x, SectionSide::Left)?;
            }
            if x < end_mm {
                consider(x, SectionSide::Right)?;
            }
        }
        let Some(&next) = points.get(index + 1) else {
            break;
        };
        let [w_start, w_end] = loads
            .iter()
            .fold([0.0; 2], |sum, &(factor, comp)| match comp {
                Comp::Dist { a, b, w1, w2 } if a <= x && next <= b => std::array::from_fn(|end| {
                    sum[end] + factor * distributed_intensity(a, b, w1, w2, [x, next][end])
                }),
                _ => sum,
            });
        if !w_start.is_finite() || !w_end.is_finite() {
            return Err("分布荷重の強度が非有限です".into());
        }
        if (w_start < 0.0 && w_end > 0.0) || (w_start > 0.0 && w_end < 0.0) {
            let scale = w_start.abs().max(w_end.abs());
            let fraction = (w_start.abs() / scale) / (w_start.abs() / scale + w_end.abs() / scale);
            let root = x + (next - x) * fraction;
            if x < root && root < next {
                consider(root, SectionSide::Continuous)?;
            }
        }
    }
    Ok(peak.expect("区間の両端は評価済み"))
}
