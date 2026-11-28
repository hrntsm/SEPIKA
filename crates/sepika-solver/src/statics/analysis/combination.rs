//! 荷重組合せの解析（荷重ケース単体の結果の線形和）。
//!
//! 解析の最小単位は荷重ケース単体とし、荷重組合せは解き直さずに
//! 単体結果の係数和として組み立てる（引張専用の合成荷重解とは異なる。
//! [`crate::statics::linear::superpose_static`]）。

use sepika_core::ids::LoadCaseId;
use sepika_core::load_combo::effective_combination_terms;
use sepika_core::model::LoadCombination;
use sepika_math::solver::SolveError;

use super::{Analysis, ConfirmedStaticResult, ConfirmedStaticSource};
use crate::statics::linear::{superpose_static, StaticOnce};

/// 荷重ケース単体と荷重組合せの一括解析の結果
/// （[`Analysis::linear_static_with_combinations`]）。
pub struct StaticBatch {
    /// 荷重ケース単体の結果。入力の荷重ケース列と同順。
    pub cases: Vec<Result<StaticOnce, SolveError>>,
    /// 荷重組合せの結果。入力の組合せ列と同順で、`cases` の線形和。
    pub combos: Vec<Result<StaticOnce, SolveError>>,
}

/// 荷重組合せ群が参照する荷重ケースを、重複を除いて初出順に並べる。
fn referenced_case_ids(combos: &[LoadCombination]) -> Vec<LoadCaseId> {
    let mut ids: Vec<LoadCaseId> = Vec::new();
    for combo in combos {
        for (id, _) in &combo.terms {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
    }
    ids
}

impl<'m> Analysis<'m> {
    /// 荷重組合せを解く。
    ///
    /// 求解は参照する荷重ケース単体（[`Self::linear_static`]）のみで行い、
    /// 組合せの結果はその線形和として組み立てる（モジュールの説明を参照）。
    /// 同じ荷重ケースを 2 回以上参照する組合せでも、その荷重ケースは 1 回だけ解く。
    pub fn linear_combination(&self, combo: &LoadCombination) -> Result<StaticOnce, SolveError> {
        self.confirmed_linear_combination(combo)
            .map(|response| response.result)
    }

    /// 組合せを構成する既存の解析経路で求解し、線形和に実際に使った係数と結果を一体に返す。
    pub fn confirmed_linear_combination(
        &self,
        combo: &LoadCombination,
    ) -> Result<ConfirmedStaticResult<'m>, SolveError> {
        let combos = std::slice::from_ref(combo);
        let ids = referenced_case_ids(combos);
        let cases = self.linear_static_batch(&ids);
        let (result, terms) = self.combine_case_results_with_terms(combo, &ids, &cases)?;
        Ok(ConfirmedStaticResult {
            model: self.model,
            source: ConfirmedStaticSource::Combination(combo.name.clone()),
            terms,
            result,
        })
    }

    /// 複数の荷重組合せを一括で解く（分解済み K を共有）。
    ///
    /// 参照されている荷重ケースを重複なく 1 回ずつ解き
    /// （[`Self::linear_static_batch`] によるケース並列）、各組合せはその結果の
    /// 線形和として組み立てる。
    pub fn linear_combination_batch(
        &self,
        combos: &[LoadCombination],
    ) -> Vec<Result<StaticOnce, SolveError>> {
        let ids = referenced_case_ids(combos);
        let cases = self.linear_static_batch(&ids);
        combos
            .iter()
            .map(|combo| self.combine_case_results(combo, &ids, &cases))
            .collect()
    }

    /// 荷重ケース単体と荷重組合せをまとめて解く（一貫計算の一括解析の入口）。
    ///
    /// `lcs` の荷重ケースを単体で解き（[`Self::linear_static_batch`]）、`combos` の
    /// 各組合せをその結果の線形和として組み立てる。組合せが `lcs` に含まれない
    /// 荷重ケースを参照している場合、その組合せは `Err` になる（`lcs` には
    /// 通常 `Model::load_cases` の全件を渡す）。
    pub fn linear_static_with_combinations(
        &self,
        lcs: &[LoadCaseId],
        combos: &[LoadCombination],
    ) -> StaticBatch {
        let cases = self.linear_static_batch(lcs);
        let combos = combos
            .iter()
            .map(|combo| self.combine_case_results(combo, lcs, &cases))
            .collect();
        StaticBatch { cases, combos }
    }

    /// 荷重ケース単体の結果（`ids` と同順の `cases`）から、1 つの荷重組合せの
    /// 結果を線形和で組み立てる。
    ///
    /// 参照する荷重ケースの結果がない（`ids` に含まれない）、またはその荷重ケースの
    /// 解析が失敗している場合は、水平力の項が黙って 0 になるのを避けるため
    /// `Err` を返す。項を持たない組合せは零の結果とする（節点数・評価断面の
    /// 構成を保つため、解けている荷重ケースの結果を 0 倍して作る）。
    fn combine_case_results(
        &self,
        combo: &LoadCombination,
        ids: &[LoadCaseId],
        cases: &[Result<StaticOnce, SolveError>],
    ) -> Result<StaticOnce, SolveError> {
        self.combine_case_results_with_terms(combo, ids, cases)
            .map(|(result, _)| result)
    }

    fn combine_case_results_with_terms(
        &self,
        combo: &LoadCombination,
        ids: &[LoadCaseId],
        cases: &[Result<StaticOnce, SolveError>],
    ) -> Result<(StaticOnce, Vec<(LoadCaseId, f64)>), SolveError> {
        let lookup = |id: &LoadCaseId| -> Option<&Result<StaticOnce, SolveError>> {
            ids.iter()
                .position(|cid| cid == id)
                .and_then(|i| cases.get(i))
        };
        let invalid =
            |reason| SolveError::InvalidInput(format!("荷重組合せ「{}」: {reason}", combo.name));
        let effective =
            effective_combination_terms(combo, &self.model.load_cases).map_err(invalid)?;
        let mut terms: Vec<(&StaticOnce, f64)> = Vec::with_capacity(effective.len());
        for &(id, factor) in &effective {
            let case = lookup(&id)
                .ok_or_else(|| invalid(format!("荷重ケース {} の解析結果がありません", id.0)))?;
            let res = case.as_ref().map_err(|e| {
                let message = format!(
                    "荷重ケース {} の解析が失敗したため荷重組合せ「{}」を組み立てられません: {}",
                    id.0, combo.name, e
                );
                match e {
                    SolveError::NonConvergence(_) => SolveError::NonConvergence(message),
                    _ => SolveError::InvalidInput(message),
                }
            })?;
            terms.push((res, factor));
        }
        if terms.is_empty() {
            let Some(res) = cases.iter().find_map(|res| res.as_ref().ok()) else {
                return Ok((self.zero_result(), effective));
            };
            terms.push((res, 0.0));
        }
        let result = superpose_static(&terms).map_err(|error| invalid(error.to_string()))?;
        Ok((result, effective))
    }
}
