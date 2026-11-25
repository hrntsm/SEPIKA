use super::*;
use sepika_core::load_combo::{LoadAction, LoadDuration};
#[cfg(any(feature = "gui", test))]
use sepika_core::structure_kind::{structure_kind_of, StructureKind};
#[cfg(any(feature = "gui", test))]
use sepika_design_jp::MemberKind;
use sepika_design_jp::{
    SrcColumnHeadLimit, SrcColumnRoute21RcShear, SrcColumnRoute21Shear, SrcColumnRoute21ShearAxis,
    SrcColumnSeismicSense,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Route21Source {
    pub combo: String,
    run: String,
    design_conditions: [u8; 32],
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Route21Registration {
    pub elem: ElemId,
    pub source: Route21Source,
    pub input: Route21Input,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Route21Input {
    pub clear_height_mm: f64,
    pub axes: [Option<Route21Axis>; 2],
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Route21Axis {
    pub positive: bool,
    pub steel_n_mm: [f64; 2],
    pub rc: Route21Rc,
    pub evidence: String,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum Route21Rc {
    Bending {
        ultimate_n_mm: [f64; 2],
        connected_beams: Option<(f64, bool)>,
    },
    PrimaryEarthquake {
        shear_n: f64,
    },
}

impl Route21Axis {
    fn common_input(&self) -> SrcColumnRoute21ShearAxis {
        let rc = match self.rc {
            Route21Rc::Bending {
                ultimate_n_mm,
                connected_beams,
            } => SrcColumnRoute21RcShear::Bending {
                ultimate_n_mm,
                head_limit: connected_beams.map_or(
                    SrcColumnHeadLimit::BothEndsYield,
                    |(sum_n_mm, top_story)| SrcColumnHeadLimit::ConnectedBeams {
                        sum_n_mm,
                        top_story,
                    },
                ),
            },
            Route21Rc::PrimaryEarthquake { shear_n } => {
                SrcColumnRoute21RcShear::PrimaryEarthquake { shear_n }
            }
        };
        SrcColumnRoute21ShearAxis {
            sense: if self.positive {
                SrcColumnSeismicSense::Positive
            } else {
                SrcColumnSeismicSense::Negative
            },
            steel_allowable_n_mm: self.steel_n_mm,
            rc,
            evidence: self.evidence.clone(),
        }
    }
}

impl Route21Input {
    fn common_input(&self) -> SrcColumnRoute21Shear {
        let [qy, qz] = self
            .axes
            .each_ref()
            .map(|axis| axis.as_ref().map(Route21Axis::common_input));
        SrcColumnRoute21Shear {
            clear_height_mm: self.clear_height_mm,
            qy,
            qz,
        }
    }
}

#[cfg(any(feature = "gui", test))]
pub(crate) fn is_src_column(
    model: &sepika_core::model::Model,
    elem: &sepika_core::model::ElementData,
) -> bool {
    let Some(mat) = model.element_material(elem) else {
        return false;
    };
    structure_kind_of(model.element_section(elem), Some(mat.category)) == StructureKind::Src
        && MemberKind::try_of_element(elem, model) == Ok(MemberKind::Column)
}

impl App {
    pub(crate) fn selected_route21_source(&self) -> Result<Route21Source, String> {
        if !self.selected_design_load_state().is_ok_and(|state| {
            state.combination
                && state.duration == LoadDuration::Short
                && state.action == LoadAction::Seismic
        }) {
            return Err("短期地震荷重組合せの解析結果を選択してください".into());
        }
        let Some(key @ ResultInputKey::Combo(_)) = self.selected_design_source() else {
            return Err("地震荷重組合せの解析結果がありません".into());
        };
        self.require_result_input(key.clone())?;
        let Some(StaticKey::Combo(index)) = self.core.scoped.last_static else {
            return Err("地震荷重組合せの解析結果がありません".into());
        };
        if !self.core.scoped.results.as_ref().is_some_and(|bundle| {
            bundle.combos.get(index).is_some_and(|(_, response)| {
                result_validity::same_saved_member_forces(
                    &response.member_forces,
                    &bundle.member_forces,
                )
            })
        }) {
            return Err("選択した短期地震荷重組合せと表示中の解析断面力が一致しません。組合せを選択し直してください".into());
        }
        let baseline = self.design_input_without_route21(Some(key.clone()));
        let ResultInputKey::Combo(combo) = key else {
            unreachable!()
        };
        self.route21_source(&combo, &baseline)
            .ok_or_else(|| "解析の実行記録がありません。組合せを再解析してください".into())
    }

    pub(super) fn route21_source(&self, combo: &str, baseline: &[u8]) -> Option<Route21Source> {
        let bundle = self.core.scoped.results.as_ref()?;
        let key = ResultInputKey::Combo(combo.to_owned());
        let (_, run) = bundle
            .reference_runs
            .iter()
            .find(|(source, _)| *source == key)?;
        Some(Route21Source {
            combo: combo.into(),
            run: run.clone(),
            design_conditions: Sha256::digest(baseline).into(),
        })
    }

    pub(super) fn route21_registrations<'a>(
        &'a self,
        source: &'a Route21Source,
    ) -> impl Iterator<Item = &'a Route21Registration> {
        self.core
            .scoped
            .route21_registrations
            .iter()
            .filter(move |r| r.source == *source)
    }

    pub(super) fn has_route21_registrations(&self, combo: &str) -> bool {
        self.core
            .scoped
            .route21_registrations
            .iter()
            .any(|r| r.source.combo == combo)
    }

    pub(super) fn selected_route21_inputs(
        &self,
    ) -> Result<Option<HashMap<ElemId, SrcColumnRoute21Shear>>, String> {
        let Some(ResultInputKey::Combo(combo)) = self.selected_design_source() else {
            return Ok(None);
        };
        if !self.has_route21_registrations(&combo) {
            return Ok(None);
        }
        let source = self.selected_route21_source()?;
        Ok(Some(
            self.route21_registrations(&source)
                .map(|r| (r.elem, r.input.common_input()))
                .collect(),
        ))
    }

    #[cfg(any(feature = "gui", test))]
    pub(crate) fn register_route21(
        &mut self,
        source: Route21Source,
        elem: ElemId,
        input: Route21Input,
    ) -> Result<(), String> {
        if !self.study_is_main() || self.core.scoped.job.is_some() {
            return Err("本案の解析完了後に根拠を登録してください".into());
        }
        if self.selected_route21_source()? != source {
            return Err(
                "解析実行または設計条件が変わりました。値と根拠を確認し直してください".into(),
            );
        }
        let model = &self.core.model;
        if !model.element(elem).is_some_and(|e| is_src_column(model, e)) {
            return Err("対象はSRC柱に限ります".into());
        }
        self.core
            .scoped
            .route21_registrations
            .retain(|r| r.elem != elem || r.source.combo != source.combo);
        self.core
            .scoped
            .route21_registrations
            .push(Route21Registration {
                elem,
                source,
                input,
            });
        self.core.scoped.references.advance();
        self.core.scoped.staleness.mark_non_calc_edited();
        self.core.scoped.staleness.design_stale = true;
        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(crate) fn remove_route21(&mut self, combo: &str, elem: ElemId) {
        self.core
            .scoped
            .route21_registrations
            .retain(|r| r.elem != elem || r.source.combo != combo);
        self.core.scoped.references.advance();
        self.core.scoped.staleness.mark_non_calc_edited();
        self.core.scoped.staleness.design_stale = true;
    }
}
