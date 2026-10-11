//! 単純支持梁の静定反力。
//!
//! - [`static_reactions`] — 部材荷重の等価節点重量（両端反力）

use super::*;

/// 符号付き反力 [N] を (i端, j端) の順で返す。位置・支間は [mm]。
/// 支間≤0は0。点位置・分布区間を支間内へ制限し、空区間は0とする。
pub(crate) fn static_reactions(kind: &MemberLoadKind, len: f64) -> (f64, f64) {
    if len <= 0.0 {
        return (0.0, 0.0);
    }
    match *kind {
        MemberLoadKind::Point { a, p } => {
            let a = a.clamp(0.0, len);
            let ri = p * (len - a) / len;
            let rj = p * a / len;
            (ri, rj)
        }
        MemberLoadKind::Distributed { a, b, w1, w2 } => {
            let a = a.max(0.0);
            let b = b.min(len);
            if b <= a {
                return (0.0, 0.0);
            }
            let span = b - a;
            let total = (w1 + w2) / 2.0 * span;
            let first_moment = a * total + span * span * (w1 + 2.0 * w2) / 6.0;
            let rj = first_moment / len;
            let ri = total - rj;
            (ri, rj)
        }
    }
}
