use super::*;
use sepika_solver::dynamic::lumped_mass::{identify_story_trilinear, StoryFitOptions, StoryIdentification, bending_drifts};

fn crossing(previous: f64, next: f64, target: f64) -> Option<f64> {
    if previous.abs() >= target { Some(0.0) }
    else if next.abs() >= target {
        let signed_target = target.copysign(next);
        Some((signed_target-previous)/(next-previous))
    } else { None }
}

fn source_limit(po: &PushoverResult, heights: &[f64]) -> JobResult<(usize,f64)> {
    let target = po.target.ok_or_else(||JobError::InvalidInput("元増分解析の終了目標が未記録です。増分解析を再実行してください".into()))?;
    if po.steps.len()!=po.capacity_curve.len() || po.steps.len()<2 {
        return Err(JobError::InvalidInput("元増分解析の確定ステップと性能曲線が不足・不整合です".into()));
    }
    if target.max_disp.into_iter().chain(target.max_drift_angle).any(|x|!x.is_finite() || x<=0.0) {
        return Err(JobError::InvalidInput("元増分解析の終了目標が不正です".into()));
    }
    for i in 1..po.steps.len() {
        let p=&po.capacity_curve[i-1]; let n=&po.capacity_curve[i];
        if p.story_drift.len()!=heights.len() || n.story_drift.len()!=heights.len() {
            return Err(JobError::InvalidInput("元増分解析の層間変位の数が不整合です".into()));
        }
        let mut fractions=Vec::new();
        if let Some(d)=target.max_disp { fractions.extend(crossing(p.roof_disp,n.roof_disp,d)); }
        if let Some(r)=target.max_drift_angle {
            for (j,h) in heights.iter().enumerate() { fractions.extend(crossing(p.story_drift[j],n.story_drift[j],r*h)); }
        }
        if !target.is_enabled() { fractions.extend(crossing(po.steps[i-1].load_factor,po.steps[i].load_factor,1.0)); }
        if let Some(f)=fractions.into_iter().min_by(f64::total_cmp) { return Ok((i,f)); }
    }
    Err(JobError::InvalidInput("元増分解析の終了目標へ未到達です。同定上限までの追加解析が必要です".into()))
}

pub(super) fn fit(
    inp: &LumpedMassBuildInput<'_>, dir: SeismicDir, base: &LumpedMassModel,
    bending: Option<(&[f64], &[f64])>,
) -> JobResult<Vec<StoryIdentification>> {
    let po=pushover_of(inp,dir)?;
    let layers=inp.model.layers();
    let heights: Vec<_>=layers.iter().map(|l|l.height).collect();
    let (last,fraction)=source_limit(po,&heights)?;
    let elastic=static_of(inp,dir)?;
    let qel=if bending.is_some() { super::bending::signed_shears(inp.model,elastic,dir)? }
        else { story_shears(inp.model,elastic,dir)?.into_iter().map(f64::abs).collect() };
    let del: Vec<_>=layers.iter().enumerate().map(|(i,l)| if inp.dim==StickDim::Spatial {
        story_drift_at(inp.model,elastic,dir,l,base.spatial[i].rigidity_xy)
    } else { story_drifts(inp.model,elastic,dir)[i] }).collect();
    let dbel=if let Some((ei,h))=bending { bending_drifts(h,ei,&qel).map_err(|e|JobError::InvalidInput(e.to_string()))? } else { vec![0.0;layers.len()] };
    let initial:Vec<_>=qel.iter().zip(&del).zip(&dbel).map(|((&q,&d),&b)|q/(d-b)).collect();
    let baseline=po.steps.iter().position(|s|s.load_factor==0.0);
    let zeros=vec![0.0;layers.len()];
    let (d0,q0)=if let Some(i)=baseline {
        (super::bending::step_drifts(inp,dir,&po.steps[i],&po.capacity_curve[i],base)?,po.capacity_curve[i].story_shear.clone())
    } else { (zeros.clone(),zeros.clone()) };
    let mut total=vec![Vec::new();layers.len()];
    let mut curves=vec![Vec::new();layers.len()];
    for j in 0..=last {
        let cp=&po.capacity_curve[j]; let step=&po.steps[j];
        if cp.story_shear.len()!=layers.len() { return Err(JobError::InvalidInput("元増分解析の層せん断力が不足しています".into())); }
        let drift=super::bending::step_drifts(inp,dir,step,cp,base)?;
        let q:Vec<_>=cp.story_shear.iter().zip(&q0).map(|(q,z)|q-z).collect();
        let db=if let Some((ei,h))=bending { bending_drifts(h,ei,&q).map_err(|e|JobError::InvalidInput(e.to_string()))? } else { zeros.clone() };
        for i in 0..layers.len() {
            total[i].push(drift[i]-d0[i]);
            curves[i].push((drift[i]-d0[i]-db[i],q[i]));
        }
    }
    let manual_dir=match dir { SeismicDir::X=>sepika_core::model::LumpedVibrationDir::X, SeismicDir::Y=>sepika_core::model::LumpedVibrationDir::Y };
    layers.iter().enumerate().map(|(i,l)| {
        let fail=|reason: String|JobError::InvalidInput(format!("階 {} / {:?}: 等価降伏未同定・近似不成立: {reason}",l.name,dir));
        if !initial[i].is_finite() || initial[i]<=0.0 { return Err(fail(format!("正の初期せん断剛性を得られません Q={}, δ={}, δb={}",qel[i],del[i],dbel[i]))); }
        let blend=|a:f64,b:f64|a+fraction*(b-a);
        let dfit=blend(curves[i][last-1].0,curves[i][last].0);
        let qfit=blend(curves[i][last-1].1,curves[i][last].1);
        curves[i][last]=(dfit,qfit);
        total[i][last]=blend(total[i][last-1],total[i][last]);
        let target=inp.target_drift_angle*l.height;
        let mut area_limit=None;
        for j in 1..=last {
            if let Some(f)=crossing(total[i][j-1],total[i][j],target) {
                area_limit=Some(curves[i][j-1].0+f*(curves[i][j].0-curves[i][j-1].0)); break;
            }
        }
        let da=area_limit.ok_or_else(||fail("目標全層間変形角までのデータ不足。面積評価上限への追加解析が必要です".into()))?;
        let manual:Vec<_>=inp.model.lumped_skeleton_intervals.iter().filter(|s|s.story==l.bottom && s.dir==manual_dir).collect();
        if manual.len()>1 { return Err(fail("手動同定区間が重複しています".into())); }
        let fit=identify_story_trilinear(&curves[i],StoryFitOptions { initial_stiffness:initial[i],secant_ratio:inp.secant_ratio,area_limit:da,identification_limit:dfit,manual_interval:manual.first().map(|s|s.interval_mm) }).map_err(|e|fail(e.to_string()))?;
        Ok(StoryIdentification { story:l.bottom, dir, bending_yield_drift:dbel[i]*fit.skeleton.q2/qel[i], fit, source_target:po.target.unwrap() })
    }).collect()
}
