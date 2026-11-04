//! 非線形反復の接線剛性行列と内力ベクトルの組み立て。
//!
//! - [`assemble_k`] — 全体接線剛性行列
//! - [`compute_f_int`] — 全自由節点の内力ベクトル
//! - [`add_support_spring_f_int`] — 支点ばね（`Node::support_spring`）の内力寄与
//!
//! 要素の**現在の状態**（`&[Box<dyn ElementBehavior>]`）から組み立てる経路であり、
//! Newton 反復を回す解析はすべてここを通る（増分解析・非線形時刻歴）。
//! モデルから毎回要素を組み直す線形経路は [`super::assemble`] にある。

use crate::common::assemble::support_spring_terms;
use crate::common::csc_cache::CscCache;
use sepika_core::dof::DofMap;
use sepika_core::model::Model;
use sepika_element::behavior::{Ctx, ElementBehavior};

/// 全体接線剛性行列を組み立てる。
pub(crate) fn assemble_k(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
) -> faer::sparse::SparseColMat<usize, f64> {
    use sepika_math::sparse::assemble_csc;
    let triplets = assemble_k_triplets(model, dofmap, behaviors);
    assemble_csc(dofmap.n_active(), triplets)
}

/// [`assemble_k`] のキャッシュ版。結果は常に [`assemble_k`] とビット一致する。
pub(crate) fn assemble_k_cached(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    cache: &mut CscCache,
) -> faer::sparse::SparseColMat<usize, f64> {
    let triplets = assemble_k_triplets(model, dofmap, behaviors);
    cache.assemble(dofmap.n_active(), &triplets)
}

/// [`assemble_k_cached`] の参照返し版。呼び出し側の triplet バッファ `buf`
/// （[`assemble_k_triplets_into`] 経由で `clear()` して再利用、容量は呼び出し間で
/// 維持される）を使い、`cache` が内部保持する行列への参照を返す（`.clone()` を
/// 伴わない）。非線形時刻歴の Newton 反復のように、結果をすぐ読むだけで所有権が
/// 要らない呼び出し元向け（[`CscCache::assemble_ref`] 参照）。結果は常に
/// [`assemble_k`] とビット一致する。
pub(crate) fn assemble_k_cached_ref<'a>(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    cache: &'a mut CscCache,
    buf: &mut Vec<sepika_math::sparse::Triplet>,
) -> &'a faer::sparse::SparseColMat<usize, f64> {
    assemble_k_triplets_into(model, dofmap, behaviors, buf);
    cache.assemble_ref(dofmap.n_active(), buf)
}

/// [`assemble_k`]・[`assemble_k_cached`] が共有する triplet 列の組立て。
fn assemble_k_triplets(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
) -> Vec<sepika_math::sparse::Triplet> {
    let mut triplets = Vec::new();
    assemble_k_triplets_into(model, dofmap, behaviors, &mut triplets);
    triplets
}

/// [`assemble_k_triplets`] の結果を呼び出し側の既存バッファへ書き込む版
/// （`out` は先頭で `clear()` してから書き込むため、確保済みの容量は保持され、
/// Newton 反復のように毎回呼ぶ場面で再確保が発生しない）。計算内容・順序は
/// [`assemble_k_triplets`] と同一（ビット完全一致）。
fn assemble_k_triplets_into(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    out: &mut Vec<sepika_math::sparse::Triplet>,
) {
    out.clear();
    let ctx = Ctx { model };
    let elem_triplets = |b: &dyn ElementBehavior| -> Vec<sepika_math::sparse::Triplet> {
        let gdofs = b.global_dofs(dofmap);
        let k = b.tangent_stiffness(&ctx);
        k.to_triplets(&gdofs)
    };
    crate::common::elem_loop::fold_behaviors_ordered(
        behaviors,
        |i, b| match model.elements.get(i) {
            Some(_) => elem_triplets(b),
            None => Vec::new(),
        },
        |triplets| out.extend(triplets),
    );
    for (active, k) in support_spring_terms(model, dofmap) {
        out.push(sepika_math::sparse::Triplet {
            row: active,
            col: active,
            val: k,
        });
    }
}

pub(crate) fn compute_f_int(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
) -> Vec<f64> {
    let ctx = Ctx { model };
    let mut f = vec![0.0; dofmap.n_active()];
    crate::common::elem_loop::fold_behaviors_ordered(
        behaviors,
        |_, b| {
            let gdofs = b.global_dofs(dofmap);
            let f_local = b.internal_force(&ctx);
            (gdofs, f_local)
        },
        |(gdofs, f_local)| {
            for (&g, &v) in gdofs.iter().zip(f_local.data.iter()) {
                if g != usize::MAX {
                    f[g] += v;
                }
            }
        },
    );
    f
}

/// 支点ばね（`Node::support_spring`）の内力寄与 `k_i・u_i` を、内力ベクトル `f`
/// （`compute_f_int` と同じ active DOF 順）へ加算する。
/// 試行全体変位 `u_trial`（active DOF 順）を呼び出し側から明示的に渡す。
/// 呼び出し側は `compute_f_int` の結果に本関数の寄与を加算すること。
pub(crate) fn add_support_spring_f_int(
    model: &Model,
    dofmap: &DofMap,
    u_trial: &[f64],
    f: &mut [f64],
) {
    for (active, k) in support_spring_terms(model, dofmap) {
        if let (Some(&u), Some(fa)) = (u_trial.get(active), f.get_mut(active)) {
            *fa += k * u;
        }
    }
}
