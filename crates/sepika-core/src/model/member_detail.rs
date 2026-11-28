//! 部材の付帯情報（ハンチ・継手位置）。
//!
//! - [`Haunch`] — 部材端部のハンチ（1 端分の形状）。
//! - [`JointKind`] — 継手の種別（現場継手／工場継手）。
//! - [`MemberJoint`] — 継手 1 箇所（部材軸上の位置）。
//! - [`MemberDetailAttr`] — 部材 1 本分の付帯情報（`Model::member_detail_attrs`）。
//!
//! いずれも**剛性・応力解析には影響しない**（「ハンチ／テーパーの
//! 剛性は一様断面として無視する（慣用）」の方針どおり、剛性は基準断面のまま）。
//! 検定位置の追加、短期地震時のRC梁QD1の材端降伏曲げ、数量拾い・製作情報の保持に用いる。

use super::*;

/// 部材端部のハンチ（1 端分）。剛性には影響しない付帯情報。
///
/// 鉛直ハンチ（せい増分）・水平ハンチ（幅増分）の双方を表現できる。
/// 寸法は数量拾い（増打ちコンクリート量・鉄骨重量等）の基礎データとして保持し、
/// 設計上は「ハンチ端」（フェースから `length` だけ内側）が基準断面の始まりと
/// なるため検定位置に追加する。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Haunch {
    /// ハンチ長 `[mm]`（柱フェース（`RigidZone::face_i/j`）から部材内側へ測る）。
    pub length: f64,
    /// せい増分 `[mm]`（鉛直ハンチ。フェース位置での基準断面せいへの増分。0=なし）。
    #[serde(default)]
    pub depth_increase: f64,
    /// 幅増分 `[mm]`（水平ハンチ。フェース位置での基準断面幅への増分。0=なし）。
    #[serde(default)]
    pub width_increase: f64,
    /// RC梁QD1用に設計者が確認したハンチ付き材端フェース。未指定ならハンチ形状だけから補わない。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qd1_face: Option<HaunchQd1Face>,
}

/// 梁軸に直交する実矩形フェース。軸力の影響を無視できること、両方向の有効引張主筋の付着・定着を設計者が確認する。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HaunchQd1Face {
    /// 当該i/j側の解析材端からフェースまでの材軸方向距離 [mm]（非負）。
    pub distance_mm: f64,
    /// フェースでの実幅 [mm]。
    pub width_mm: f64,
    /// フェースでの実せい [mm]。
    pub depth_mm: f64,
    pub top: HaunchQd1Tension,
    pub bottom: HaunchQd1Tension,
}

/// フェースを梁軸方向に平行に横切る、上端引張または下端引張の全有効主筋。同じ降伏点の筋に限る。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HaunchQd1Tension {
    /// 有効引張鉄筋面積の合計 [mm²]。
    pub area_mm2: f64,
    /// 実降伏応力度 [N/mm²]。
    pub yield_strength_n_mm2: f64,
    /// 圧縮縁から有効引張鉄筋の重心までの距離 [mm]。
    pub effective_depth_mm: f64,
}

impl HaunchQd1Face {
    /// 入力値の数値・断面内の位置関係に矛盾があれば理由を返す。モデル形状や配筋条件は照合しない。
    pub fn input_issue(&self) -> Option<&'static str> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !self.distance_mm.is_finite() || self.distance_mm < 0.0 {
            return Some("材端からフェースまでの距離は有限の非負値が必要です");
        }
        let area_mm2 = self.width_mm * self.depth_mm;
        if ![self.width_mm, self.depth_mm, area_mm2]
            .into_iter()
            .all(positive)
        {
            return Some("矩形フェースの実幅・実せい・面積は有限の正値が必要です");
        }
        let valid_tension = |t: HaunchQd1Tension| {
            [t.area_mm2, t.yield_strength_n_mm2, t.effective_depth_mm]
                .into_iter()
                .all(positive)
                && t.effective_depth_mm < self.depth_mm
        };
        if !valid_tension(self.top) {
            return Some("上端引張筋の面積・降伏点・断面内の有効せいを確認できません");
        }
        if !valid_tension(self.bottom) {
            return Some("下端引張筋の面積・降伏点・断面内の有効せいを確認できません");
        }
        if self.top.area_mm2 + self.bottom.area_mm2 >= area_mm2 {
            return Some("上下引張鉄筋面積の合計が矩形フェースの面積以上です");
        }
        if self.top.effective_depth_mm <= self.depth_mm - self.bottom.effective_depth_mm {
            return Some("上端引張筋の重心が下端引張筋の重心より上にありません");
        }
        None
    }
}

/// 継手の種別（数量拾い・工程区分用）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum JointKind {
    /// 現場継手（建方時の接合。既定）。
    #[default]
    Site,
    /// 工場継手（工場溶接等）。
    Shop,
}

/// 継手（ジョイント）1 箇所。剛性には影響しない付帯情報。
///
/// 位置は始端（i 端）節点芯からの距離で保持する。設計上は継手位置の内力で
/// 継手部の検定（継手部の断面欠損は [`SteelDesignAttr`] の欠損率を使用）を
/// 行うため検定位置に追加し、数量上は継手箇所数・種別の集計に用いる。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemberJoint {
    /// 始端（i 端）節点芯からの距離 `[mm]`。
    pub distance: f64,
    /// 継手の種別（現場／工場）。
    #[serde(default)]
    pub kind: JointKind,
}

/// 部材 1 本分の付帯情報（ハンチ・継手位置）。`Model::member_detail_attrs` に
/// 要素 ID と対で保持する側テーブル属性。剛性・応力解析には影響しない。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemberDetailAttr {
    pub elem: ElemId,
    /// 始端（i 端）のハンチ。`None` はハンチなし。
    #[serde(default)]
    pub haunch_i: Option<Haunch>,
    /// 終端（j 端）のハンチ。`None` はハンチなし。
    #[serde(default)]
    pub haunch_j: Option<Haunch>,
    /// 継手の一覧（位置順でなくてよい）。
    #[serde(default)]
    pub joints: Vec<MemberJoint>,
}

impl MemberDetailAttr {
    /// 空の付帯情報（ハンチなし・継手なし）を作る。
    pub fn new(elem: ElemId) -> Self {
        Self {
            elem,
            haunch_i: None,
            haunch_j: None,
            joints: Vec::new(),
        }
    }

    /// ハンチも継手も持たない（＝側テーブルから削除してよい）か。
    pub fn is_empty(&self) -> bool {
        self.haunch_i.is_none() && self.haunch_j.is_none() && self.joints.is_empty()
    }

    /// 付帯情報が追加する検定位置（正規化座標 \[0,1\]）を算定する。
    ///
    /// - ハンチ端: フェース距離＋ハンチ長を正規化し、既定の危険断面位置
    ///   （`design_positions` / `eval_sections`）と同じ規則で
    ///   i 端側は \[0.0, 0.5)、j 端側は (0.5, 1.0\] へクランプする。
    /// - 継手: 始端節点芯からの距離を正規化する。部材長の外側（0 以下・
    ///   部材長以上）の値は不正入力として黙って除外する。
    ///
    /// 応力の評価断面（`sepika_element` の `eval_sections`）と検定位置
    /// （`design_positions`）の双方から呼ばれる
    /// 共通実装。両者の位置一致判定（1e-6）はこの単一実装により保証される。
    /// `geom_len` が 0 以下（不正な幾何）のときは空を返す。
    pub fn extra_check_positions(&self, rigid_zone: &RigidZone, geom_len: f64) -> Vec<f64> {
        let mut xs = Vec::new();
        self.for_each_extra_check_position(rigid_zone, geom_len, |xi, _| xs.push(xi));
        xs
    }

    /// 付帯検定位置ごとに、全長比 ξ と i 端からの距離 [mm] を渡す。
    #[doc(hidden)]
    pub fn for_each_extra_check_position(
        &self,
        rigid_zone: &RigidZone,
        geom_len: f64,
        mut visit: impl FnMut(f64, f64),
    ) {
        if geom_len <= 1e-12 {
            return;
        }
        if let Some(h) = &self.haunch_i {
            if h.length > 0.0 {
                let distance_mm = rigid_zone.face_i_or_zero() + h.length;
                let raw = distance_mm / geom_len;
                let xi = raw.clamp(0.0, 0.5 - 1e-9);
                visit(
                    xi,
                    if raw == xi {
                        distance_mm
                    } else {
                        xi * geom_len
                    },
                );
            }
        }
        if let Some(h) = &self.haunch_j {
            if h.length > 0.0 {
                let j_distance_mm = rigid_zone.face_j_or_zero() + h.length;
                let raw = 1.0 - j_distance_mm / geom_len;
                let xi = raw.clamp(0.5 + 1e-9, 1.0);
                visit(
                    xi,
                    if raw == xi {
                        geom_len - j_distance_mm
                    } else {
                        xi * geom_len
                    },
                );
            }
        }
        for j in &self.joints {
            let xi = j.distance / geom_len;
            if xi > 0.0 && xi < 1.0 {
                visit(xi, j.distance);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rigid(face_i: f64, face_j: f64) -> RigidZone {
        RigidZone {
            face_i: Some(face_i),
            face_j: Some(face_j),
            ..Default::default()
        }
    }

    /// ハンチ端は「フェース＋ハンチ長」の正規化位置になる（i 端は [0,0.5)、
    /// j 端は (0.5,1] へクランプ）。継手は始端からの距離の正規化位置になる。
    #[test]
    fn test_extra_check_positions() {
        let attr = MemberDetailAttr {
            elem: ElemId(0),
            haunch_i: Some(Haunch {
                length: 700.0,
                depth_increase: 200.0,
                width_increase: 0.0,
                qd1_face: None,
            }),
            haunch_j: Some(Haunch {
                length: 500.0,
                depth_increase: 200.0,
                width_increase: 0.0,
                qd1_face: None,
            }),
            joints: vec![MemberJoint {
                distance: 1000.0,
                kind: JointKind::Site,
            }],
        };
        let xs = attr.extra_check_positions(&rigid(300.0, 250.0), 4000.0);
        // i 端ハンチ端 (300+700)/4000 = 0.25、j 端ハンチ端 1-(250+500)/4000 = 0.8125、
        // 継手 1000/4000 = 0.25（ハンチ端との重複は評価断面側の dedup に任せる）
        assert_eq!(xs.len(), 3);
        assert!((xs[0] - 0.25).abs() < 1e-12);
        assert!((xs[1] - 0.8125).abs() < 1e-12);
        assert!((xs[2] - 0.25).abs() < 1e-12);
    }

    /// 部材長の外の継手位置・長さ 0 のハンチは黙って除外される。
    /// 幾何長 0 では空を返す。
    #[test]
    fn test_extra_check_positions_invalid_input() {
        let attr = MemberDetailAttr {
            elem: ElemId(0),
            haunch_i: Some(Haunch {
                length: 0.0,
                depth_increase: 100.0,
                width_increase: 0.0,
                qd1_face: None,
            }),
            haunch_j: None,
            joints: vec![
                MemberJoint {
                    distance: 0.0,
                    kind: JointKind::Site,
                },
                MemberJoint {
                    distance: 5000.0,
                    kind: JointKind::Shop,
                },
            ],
        };
        assert!(attr
            .extra_check_positions(&rigid(300.0, 300.0), 4000.0)
            .is_empty());
        assert!(attr
            .extra_check_positions(&rigid(300.0, 300.0), 0.0)
            .is_empty());
    }

    /// 過大なハンチ長でも i 端は [0,0.5)、j 端は (0.5,1] を越えない
    /// （既定の危険断面位置と同じクランプ規則）。
    #[test]
    fn test_extra_check_positions_clamped() {
        let attr = MemberDetailAttr {
            elem: ElemId(0),
            haunch_i: Some(Haunch {
                length: 3000.0,
                depth_increase: 0.0,
                width_increase: 0.0,
                qd1_face: None,
            }),
            haunch_j: Some(Haunch {
                length: 3000.0,
                depth_increase: 0.0,
                width_increase: 0.0,
                qd1_face: None,
            }),
            joints: Vec::new(),
        };
        let xs = attr.extra_check_positions(&rigid(0.0, 0.0), 4000.0);
        assert_eq!(xs.len(), 2);
        assert!(xs[0] < 0.5);
        assert!(xs[1] > 0.5);
    }

    /// is_empty はハンチ・継手のいずれもないときだけ真。
    #[test]
    fn test_is_empty() {
        let mut attr = MemberDetailAttr::new(ElemId(1));
        assert!(attr.is_empty());
        attr.joints.push(MemberJoint {
            distance: 100.0,
            kind: JointKind::Shop,
        });
        assert!(!attr.is_empty());
    }
}
