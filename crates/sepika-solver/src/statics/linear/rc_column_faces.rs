use super::{ElemId, StaticOnce};
use sepika_core::face_distance::RcColumnGirderFace;
use sepika_element::frame::member_load::LoadedFrameSection;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RcColumnFaceForce {
    pub face: RcColumnGirderFace,
    /// 柱局所系の符号付き [N, Qy, Qz, Mx, My, Mz] [N, N·mm]。回収不能時は理由付きエラー。
    pub forces: Result<[f64; 6], String>,
}

/// i/j順。大梁による検定面が適用外の端はNone、未確定の端は理由付きエラー。
pub type RcColumnFaceEnds = [Option<Result<RcColumnFaceForce, String>>; 2];

pub(super) fn recover(
    cuts: [Option<Result<RcColumnGirderFace, String>>; 2],
    section: Option<&LoadedFrameSection>,
) -> RcColumnFaceEnds {
    let recover = |cut: Option<Result<RcColumnGirderFace, String>>, i_face| {
        cut.map(|cut| {
            cut.map(|face| {
                let forces = section
                    .ok_or_else(|| "実材端面の荷重込み断面力を回収できません".into())
                    .and_then(|section| {
                        section.at_inward_face_mm(face.position_mm, face.length_mm, i_face)
                    });
                RcColumnFaceForce { face, forces }
            })
        })
    };
    let [i, j] = cuts;
    [recover(i, true), recover(j, false)]
}

/// 同じモデル・切断位置の柱実材端応答を合成する。非零係数の応答が不足する端は未算定。
pub fn superpose_rc_column_faces(terms: &[(&StaticOnce, f64)]) -> Vec<(ElemId, RcColumnFaceEnds)> {
    let nonzero = || terms.iter().filter(|(_, factor)| *factor != 0.0);
    let base = nonzero().next().or_else(|| terms.first());
    let mut sum = base.map_or_else(Vec::new, |(res, _)| res.rc_column_face_forces.clone());
    for (index, (id, ends)) in sum.iter_mut().enumerate() {
        for (side, end) in ends.iter_mut().enumerate() {
            let Some(Ok(dest)) = end else { continue };
            dest.forces = nonzero().try_fold([0.0; 6], |mut acc, (res, factor)| {
                let source = res
                    .rc_column_face_forces
                    .get(index)
                    .filter(|(source_id, _)| source_id == id)
                    .and_then(|(_, ends)| ends[side].as_ref())
                    .ok_or("合成対象ケースの実材端面応答がありません")?
                    .as_ref()
                    .map_err(Clone::clone)?;
                if source.face != dest.face {
                    return Err("合成対象ケースの実材端面位置が一致しません".into());
                }
                for (out, value) in acc
                    .iter_mut()
                    .zip(source.forces.as_ref().map_err(Clone::clone)?)
                {
                    *out += factor * value;
                }
                if acc.iter().any(|value| !value.is_finite()) {
                    return Err("合成した実材端面の断面力が非有限です".into());
                }
                Ok(acc)
            });
        }
    }
    sum
}
