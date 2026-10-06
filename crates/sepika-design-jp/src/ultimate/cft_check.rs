//! CFT 柱の軸終局耐力・N-M 相互作用の検定ドライバ（CFT指針）。
//!
//! - [`CftUltimateCheck`] — 1 CFT 柱の軸終局検定結果。
//! - [`collect_cft_ultimate_checks`] — モデルの CFT 柱を一括検定する。
//! - [`cft_mu_nm`] — N-M 相互作用による終局曲げ耐力 Mu(N)。

use sepika_core::ids::ElemId;
use sepika_core::model::Model;
use sepika_core::section_shape::SectionShape;

use super::cft::{
    self, cft_axial_ultimate, cft_concrete_buckling_axial, cft_concrete_slenderness, cft_ncu1,
    CftColumnClass,
};
use super::cft_nm::{
    cft_long_medium_column_mu, cft_nk, cft_short_column_mu, CftBendingInput, CftLongMediumInput,
};

/// 1 CFT 柱の軸終局検定結果。
#[derive(Clone, Debug)]
pub struct CftUltimateCheck {
    /// 部材 ID。
    pub elem: ElemId,
    /// 柱分類（短柱/中柱/長柱）。
    pub class: CftColumnClass,
    /// 軸圧縮終局耐力 Ncu [N]。
    pub ncu: f64,
    /// 軸引張終局耐力 Ntu [N]。
    pub ntu: f64,
    /// 設計軸力における N-M 相互作用の終局曲げ耐力 Mu [N·mm]
    /// （柱分類に応じて短柱／中柱／長柱の式を用いる）。
    pub mu_nm: Result<f64, String>,
    /// 設計軸力 [N]（圧縮正）。
    pub n_design: f64,
    /// 軸余裕度（圧縮 Ncu/N、引張 Ntu/|N|。N=0 は `f64::INFINITY`）。
    pub axial_margin: f64,
    /// 判定（軸余裕度 ≥ 1.0 で true）。
    pub ok: bool,
    /// 詳細（表示用）。
    pub detail: String,
}

/// CFT 断面（角型/円形）の (円形か, 断面せい D, cA, sA, cI(弱軸), sI(弱軸)) を返す。
type CftSectionProps = (bool, f64, f64, f64, f64, f64);

fn cft_section_props(shape: &SectionShape) -> Result<Option<CftSectionProps>, String> {
    let Some(core) = shape.try_cft_core_props()? else {
        return Ok(None);
    };
    let s_area = shape.try_calc_area()?;
    let s_inertia = shape.try_calc_iy()?.min(shape.try_calc_iz()?);
    let c_inertia = core.iy.min(core.iz);
    if ![core.area, s_area, c_inertia, s_inertia]
        .into_iter()
        .all(|v| v.is_finite() && v > 0.0)
    {
        return Err("CFT の材料領域性能が有限な正値ではありません".into());
    }
    Ok(match *shape {
        SectionShape::CftBox { height, width, .. } => {
            let d = height.min(width);
            Some((false, d, core.area, s_area, c_inertia, s_inertia))
        }
        SectionShape::CftPipe { outer_dia, .. } => {
            Some((true, outer_dia, core.area, s_area, c_inertia, s_inertia))
        }
        _ => None,
    })
}

/// モデルの CFT 柱（`CftBox`/`CftPipe`）について軸終局検定を一括実行する
/// （CFT指針）。
///
/// - `axial_by_elem`: 設計軸力 [N]（**圧縮正**）。なければ軸力 0（安全側）。
/// - 座屈長さ lk は部材の幾何長（K=1 相当）を用いる。鋼管の降伏強さ Fy は
///   `Section.steel_material` の材料名・板厚または有効な fy から解決し、ヤング係数は
///   205000 N/mm²（鋼）を用いる。Fc は材料の `fc`（未設定はスキップ）。
pub fn collect_cft_ultimate_checks(
    model: &Model,
    axial_by_elem: &[(ElemId, f64)],
) -> Result<Vec<CftUltimateCheck>, String> {
    let mut out = Vec::new();
    for elem in &model.elements {
        let Some(sec) = elem.section.and_then(|sid| model.sections.get(sid.index())) else {
            continue;
        };
        let Some(shape) = sec.shape.as_ref() else {
            continue;
        };
        let Some((circular, d_section, c_area, s_area, c_inertia, s_inertia)) =
            cft_section_props(shape).map_err(|error| format!("CFT 部材 {}: {error}", elem.id.0))?
        else {
            continue;
        };
        let mat = model
            .element_material(elem)
            .ok_or_else(|| format!("CFT 部材 {} の充填コンクリート材料が未設定です", elem.id.0))?;
        let steel_mat = model
            .element_steel_material(elem)
            .ok_or_else(|| format!("CFT 部材 {} の鋼管材料が未設定です", elem.id.0))?;
        let fc = mat
            .fc
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| format!("CFT 部材 {} の Fc が未設定または不正です", elem.id.0))?;
        let thick = match *shape {
            SectionShape::CftBox { thick, .. } | SectionShape::CftPipe { thick, .. } => thick,
            _ => 0.0,
        };
        let fy = sepika_core::material_grade::cft_steel_f_value(steel_mat, thick)
            .ok_or_else(|| format!("CFT 部材 {} の鋼管 F 値が未設定または不正です", elem.id.0))?;
        let lk = model.member_length(elem);
        if !lk.is_finite() || lk < 0.0 {
            return Err(format!("CFT 部材 {} の座屈長さが不正です", elem.id.0));
        }

        let inp = cft::CftAxialInput {
            circular,
            d_section,
            c_area,
            s_area,
            c_inertia,
            s_inertia,
            fc,
            fy,
            s_young: 205000.0,
            lk,
        };
        let r = cft_axial_ultimate(&inp);
        let n_design = axial_by_elem
            .iter()
            .find(|(id, _)| *id == elem.id)
            .map(|(_, n)| *n)
            .unwrap_or(0.0);

        let mu_nm = cft_mu_nm(shape, fc, fy, n_design, lk, false);
        if !n_design.is_finite() {
            return Err(format!("CFT 部材 {} の設計軸力が非有限値です", elem.id.0));
        }
        let mu_detail = match &mu_nm {
            Ok(mu) => format!("{mu:.0} N·mm"),
            Err(reason) => format!("未算定（{reason}）"),
        };

        let axial_margin = if n_design > 0.0 {
            if r.ncu > 0.0 {
                r.ncu / n_design
            } else {
                0.0
            }
        } else if n_design < 0.0 {
            if r.ntu > 0.0 {
                r.ntu / (-n_design)
            } else {
                0.0
            }
        } else {
            f64::INFINITY
        };
        let class_label = match r.class {
            CftColumnClass::Short => "短柱",
            CftColumnClass::Medium => "中柱",
            CftColumnClass::Long => "長柱",
        };
        let detail = format!(
            "分類={class_label}, Ncu={:.0} N, Ntu={:.0} N, Mu(N-M)={mu_detail}, N={:.0} N, \
             lk={:.0} mm, cA={:.0} mm², sA={:.0} mm², Fc={:.1}, Fy={:.1}, 軸余裕度={:.3}",
            r.ncu, r.ntu, n_design, lk, c_area, s_area, fc, fy, axial_margin
        );
        out.push(CftUltimateCheck {
            elem: elem.id,
            class: r.class,
            ncu: r.ncu,
            ntu: r.ntu,
            mu_nm,
            n_design,
            axial_margin,
            ok: axial_margin >= 1.0,
            detail,
        });
    }
    Ok(out)
}

/// CFT 柱の N-M 相互作用による終局曲げ耐力 `Mu(N)` [N·mm]（CFT指針。
/// 柱分類（短柱／中柱・長柱）に応じた式を選択する）。
///
/// - `n_design`: 設計軸力 [N]（**圧縮正**）。
/// - `fy`: 鋼管の降伏強さ（F 値）[N/mm²]、`lk`: 座屈長さ [mm]。
/// - `weak_axis`: 角形で幅方向（弱軸）まわりの曲げを評価する場合 true
///   （円形は同値。柱分類・軸終局は断面代表せい `d_section` のまま評価する近似）。
///
/// 許容応力度検定の設計用せん断力 `QD1 = ΣcMy/h′` の cMy（=Mu(N)）にも用いる
/// （[`crate::cft`]）。対象外・不正入力・未知角R・正の角Rは理由付きエラー。
pub fn cft_mu_nm(
    shape: &SectionShape,
    fc: f64,
    fy: f64,
    n_design: f64,
    lk: f64,
    weak_axis: bool,
) -> Result<f64, String> {
    if !n_design.is_finite() || !lk.is_finite() || lk < 0.0 {
        return Err("CFT 終局 N–M の軸力・座屈長さが不正です".into());
    }
    if !fc.is_finite() || !fy.is_finite() || fc <= 0.0 || fy <= 0.0 {
        return Err("CFT 終局 N–M の Fc/Fy が未設定または不正です".into());
    }
    let (circular, d_section, c_area, s_area, c_inertia, s_inertia) =
        cft_section_props(shape)?.ok_or("CFT 終局 N–M の対象形状ではありません")?;
    if let SectionShape::CftBox {
        corner_r: Some(r), ..
    } = shape
    {
        if *r > 0.0 {
            return Err("角形 CFT の角Rが正の終局 N–M は未対応です（内角Rが 0 でも対象。設計仕様は #418 で策定）".into());
        }
    }
    let (bd, bb, thick) = match *shape {
        SectionShape::CftBox {
            height,
            width,
            thick,
            ..
        } => {
            if weak_axis {
                (width, height, thick)
            } else {
                (height, width, thick)
            }
        }
        SectionShape::CftPipe { outer_dia, thick } => (outer_dia, outer_dia, thick),
        _ => return Err("CFT 終局 N–M の対象形状ではありません".into()),
    };
    let inp = cft::CftAxialInput {
        circular,
        d_section,
        c_area,
        s_area,
        c_inertia,
        s_inertia,
        fc,
        fy,
        s_young: 205000.0,
        lk,
    };
    let r = cft_axial_ultimate(&inp);
    let bending = CftBendingInput {
        circular,
        d_steel: bd,
        b_steel: bb,
        c_d: (bd - 2.0 * thick).max(0.0),
        c_b: (bb - 2.0 * thick).max(0.0),
        t: thick,
        fc,
        fy,
    };
    let mu = match r.class {
        CftColumnClass::Short => cft_short_column_mu(&bending, n_design, cft_ncu1(&inp), r.ntu),
        CftColumnClass::Medium | CftColumnClass::Long => cft_long_medium_column_mu(
            &CftLongMediumInput {
                bending,
                is_long: r.class == CftColumnClass::Long,
                c_ncr: cft_concrete_buckling_axial(c_inertia, c_area, fc, lk),
                c_lambda1: cft_concrete_slenderness(c_inertia, c_area, fc, lk),
                nk: cft_nk(c_inertia, s_inertia, 205000.0, fc, lk),
                ncu_axial: r.ncu,
                ntu: r.ntu,
            },
            n_design,
        ),
    };
    Ok(mu)
}
