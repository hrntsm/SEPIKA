//! 要素ループの並列/逐次足場（全解析経路共通）。
//!
//! 要素ごとの計算は要素間にデータ依存がないため、並列度設定
//! （`sepika_math::parallelism`）が `Auto`/`Threads` のときは rayon で並列化する。
//! いずれのヘルパも**結果は逐次実行と完全にビット一致する**。

use sepika_core::dof::DofMap;
use sepika_core::model::MemberLoad;
use sepika_core::model::Model;
use sepika_element::behavior::{Ctx, ElementBehavior, LocalVec};
use smallvec::SmallVec;

/// 要素ごとの読み取り計算 `compute(要素番号, behavior)` の結果を、**要素番号順**に
/// `consume` へ渡す。
pub(crate) fn fold_behaviors_ordered<T, F, G>(
    behaviors: &[Box<dyn ElementBehavior>],
    compute: F,
    mut consume: G,
) where
    T: Send,
    F: Fn(usize, &dyn ElementBehavior) -> T + Sync,
    G: FnMut(T),
{
    if sepika_math::parallelism::is_parallel() {
        use rayon::prelude::*;
        let per_elem: Vec<T> = behaviors
            .par_iter()
            .enumerate()
            .map(|(i, b)| compute(i, b.as_ref()))
            .collect();
        for t in per_elem {
            consume(t);
        }
    } else {
        for (i, b) in behaviors.iter().enumerate() {
            consume(compute(i, b.as_ref()));
        }
    }
}

/// 全自由 DOF 空間のベクトル `u_free` から、要素のグローバル DOF 添字
/// `gdofs`（`ElementBehavior::global_dofs`。拘束済み DOF は `usize::MAX`）に
/// 対応する成分を収集して要素局所ベクトルを作る。拘束済み・範囲外の成分は 0。
pub(crate) fn gather_u_elem(gdofs: &[usize], u_free: &[f64]) -> Vec<f64> {
    let mut u_elem = vec![0.0; gdofs.len()];
    for (k, &g) in gdofs.iter().enumerate() {
        if g != usize::MAX && g < u_free.len() {
            u_elem[k] = u_free[g];
        }
    }
    u_elem
}

fn apply_du_trial_mode(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &mut [Box<dyn ElementBehavior>],
    du_free: &[f64],
    parallel: bool,
) -> Result<(), String> {
    let snapshot = super::transaction::StateSnapshot::capture(behaviors);
    let ctx = Ctx { model };
    let update = |(index, b): (usize, &mut Box<dyn ElementBehavior>)| {
        let gdofs = b.global_dofs(dofmap);
        let du_elem = LocalVec {
            data: SmallVec::from_vec(gather_u_elem(&gdofs, du_free)),
        };
        b.update_state(&du_elem, false, &ctx).map_err(|error| {
            let id = model.elements.get(index).map(|e| e.id);
            format!("要素 {id:?}（要素番号 {index}）の trial 更新失敗: {error}")
        })
    };
    let results: Vec<Result<(), String>> = if parallel {
        use rayon::prelude::*;
        behaviors.par_iter_mut().enumerate().map(update).collect()
    } else {
        behaviors.iter_mut().enumerate().map(update).collect()
    };
    if let Some(error) = results.into_iter().find_map(Result::err) {
        snapshot.restore(behaviors);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn apply_du_trial(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &mut [Box<dyn ElementBehavior>],
    du_free: &[f64],
) -> Result<(), String> {
    apply_du_trial_mode(
        model,
        dofmap,
        behaviors,
        du_free,
        sepika_math::parallelism::is_parallel(),
    )
}

pub(crate) fn apply_du_trial_with_member_loads(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &mut [Box<dyn ElementBehavior>],
    du_free: &[f64],
    absolute_loads: &[Vec<MemberLoad>],
) -> Result<(), String> {
    apply_loaded_trial_mode(
        model,
        dofmap,
        behaviors,
        du_free,
        absolute_loads,
        sepika_math::parallelism::is_parallel(),
    )
}

fn apply_loaded_trial_mode(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &mut [Box<dyn ElementBehavior>],
    du_free: &[f64],
    absolute_loads: &[Vec<MemberLoad>],
    parallel: bool,
) -> Result<(), String> {
    if model.elements.len() != behaviors.len() || absolute_loads.len() != behaviors.len() {
        return Err("部材荷重付き trial: 要素・応答要素・荷重表の数が一致しません".into());
    }
    if du_free.len() != dofmap.n_active() {
        return Err("部材荷重付き trial: 変位増分の自由度数が一致しません".into());
    }
    if du_free.iter().any(|value| !value.is_finite()) {
        return Err("部材荷重付き trial: 変位増分に非有限値があります".into());
    }
    let full = dofmap.including_restrained_node_dofs();
    let mut mappings = Vec::with_capacity(behaviors.len());
    for (index, behavior) in behaviors.iter().enumerate() {
        let elem = &model.elements[index];
        let invalid = |reason| format!("要素 ID {}・部材荷重付き trial: {reason}", elem.id.0);
        if absolute_loads[index]
            .iter()
            .any(|load| load.elem != elem.id)
        {
            return Err(invalid("荷重対象IDと要素の順序が一致しません"));
        }
        let gdofs = behavior.global_dofs(dofmap);
        if gdofs.len() != behavior.n_dof()
            || gdofs.iter().any(|&g| g != usize::MAX && g >= du_free.len())
        {
            return Err(invalid("応答要素の自由度対応が一致しません"));
        }
        if behavior.owns_frame_member_loads() {
            let physical = behavior.global_dofs(&full);
            if physical.len() != behavior.n_dof()
                || physical
                    .iter()
                    .any(|&g| g != usize::MAX && g >= full.n_active())
            {
                return Err(invalid("応答要素の物理自由度対応が一致しません"));
            }
            let mut actual: Vec<_> = physical
                .iter()
                .filter(|&&g| g != usize::MAX)
                .map(|&g| full.global(g as u32))
                .filter(|&g| full.is_node_dof(g))
                .map(|g| g / sepika_core::dof::DOF_PER_NODE)
                .collect();
            actual.sort_unstable();
            actual.dedup();
            let mut expected: Vec<_> = elem.nodes.iter().map(|node| node.index()).collect();
            expected.sort_unstable();
            expected.dedup();
            if actual != expected {
                return Err(invalid("部材荷重所有要素の物理節点が一致しません"));
            }
        }
        mappings.push(gdofs);
    }
    let snapshot = super::transaction::StateSnapshot::capture(behaviors);
    let ctx = Ctx { model };
    let update = |(index, behavior): (usize, &mut Box<dyn ElementBehavior>)| {
        let du = LocalVec {
            data: SmallVec::from_vec(gather_u_elem(&mappings[index], du_free)),
        };
        let result = if behavior.owns_frame_member_loads() {
            behavior.update_state_with_member_loads(&du, &absolute_loads[index], false, &ctx)
        } else {
            behavior.update_state(&du, false, &ctx)
        };
        result.map_err(|reason| {
            format!(
                "要素 ID {}（要素番号 {index}）・部材荷重付き trial 更新失敗: {reason}",
                model.elements[index].id.0
            )
        })
    };
    let results: Vec<_> = if parallel {
        use rayon::prelude::*;
        behaviors.par_iter_mut().enumerate().map(update).collect()
    } else {
        behaviors.iter_mut().enumerate().map(update).collect()
    };
    if let Some(error) = results.into_iter().find_map(Result::err) {
        snapshot.restore(behaviors);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_element::behavior::{ElementUpdateError, LocalMat, MassOption};

    struct Probe {
        committed: f64,
        trial: f64,
        failure: Option<&'static str>,
    }

    impl ElementBehavior for Probe {
        fn n_dof(&self) -> usize {
            1
        }
        fn global_dofs(&self, _: &DofMap) -> SmallVec<[usize; 24]> {
            smallvec::smallvec![0]
        }
        fn tangent_stiffness(&self, _: &Ctx) -> LocalMat {
            let mut k = LocalMat::zeros(1);
            k.set(0, 0, self.trial);
            k
        }
        fn internal_force(&self, _: &Ctx) -> LocalVec {
            LocalVec {
                data: smallvec::smallvec![self.trial],
            }
        }
        fn mass_matrix(&self, _: MassOption) -> LocalMat {
            LocalMat::zeros(1)
        }
        fn update_state(
            &mut self,
            du: &LocalVec,
            _: bool,
            _: &Ctx,
        ) -> Result<(), ElementUpdateError> {
            self.trial += du.data[0];
            self.committed = -999.0;
            self.failure
                .map_or(Ok(()), |reason| Err(ElementUpdateError(reason.into())))
        }
        fn snapshot_state(&self) -> Box<dyn std::any::Any> {
            Box::new((self.committed, self.trial))
        }
        fn restore_state(&mut self, state: &dyn std::any::Any) {
            (self.committed, self.trial) = *state.downcast_ref::<(f64, f64)>().unwrap();
        }
        fn serialize_checkpoint(&self) -> Vec<u8> {
            bincode::serialize(&(self.committed, self.trial)).unwrap()
        }
    }

    #[test]
    fn mixed_batch_failure_restores_all_states_and_returns_first_element_in_both_modes() {
        let model = Model::default();
        let dofmap = DofMap::build(&model);
        for parallel in [false, true] {
            let mut behaviors: Vec<Box<dyn ElementBehavior>> =
                [None, Some("first failure"), Some("second failure")]
                    .into_iter()
                    .enumerate()
                    .map(|(i, failure)| {
                        Box::new(Probe {
                            committed: i as f64,
                            trial: 10.0 + i as f64,
                            failure,
                        }) as Box<dyn ElementBehavior>
                    })
                    .collect();
            let before: Vec<_> = behaviors.iter().map(|b| b.serialize_checkpoint()).collect();
            let error =
                apply_du_trial_mode(&model, &dofmap, &mut behaviors, &[2.0], parallel).unwrap_err();
            assert!(
                error.contains("要素番号 1") && error.contains("first failure"),
                "{error}"
            );
            assert!(!error.contains("second failure"));
            assert_eq!(
                behaviors
                    .iter()
                    .map(|b| b.serialize_checkpoint())
                    .collect::<Vec<_>>(),
                before
            );
        }
    }
}

#[cfg(test)]
mod preserved_tests;
