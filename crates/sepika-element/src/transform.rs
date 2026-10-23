use crate::behavior::LocalMat;
use sepika_core::ids::NodeId;
use sepika_core::model::{ElementData, Model};

/// 2 節点線材の端部幾何（節点 ID・節点座標・節点間距離）。
///
/// 節点 ID が範囲外のときは原点へ落とす。
#[derive(Clone, Copy, Debug)]
pub(crate) struct EndGeometry {
    /// 両端の節点 ID（i 端, j 端）。
    pub(crate) nodes: [NodeId; 2],
    /// 両端の節点座標 \[mm\]（i 端, j 端）。
    pub(crate) coords: [[f64; 3]; 2],
    /// 節点間距離（芯々長さ）\[mm\]。
    pub(crate) length: f64,
}

impl EndGeometry {
    /// 要素データとモデルから端部幾何を求める。
    pub(crate) fn of_element(data: &ElementData, model: &Model) -> Self {
        let nodes = [data.nodes[0], data.nodes[1]];
        let coords = nodes.map(|n| {
            model
                .nodes
                .get(n.index())
                .map(|node| node.coord)
                .unwrap_or([0.0; 3])
        });
        Self {
            nodes,
            coords,
            length: sepika_core::geom::vec3::dist(coords[0], coords[1]),
        }
    }

    /// i 端から j 端へ向かう局所座標系。
    pub(crate) fn local_frame(&self, ref_vec: [f64; 3]) -> Result<LocalFrame, &'static str> {
        LocalFrame::from_nodes(self.coords[0], self.coords[1], ref_vec)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LocalFrame {
    pub rot: [[f64; 3]; 3],
}

impl LocalFrame {
    /// 局所 x は i→j、局所 y は断面参照方向の直交成分、局所 z は右手系とする。
    /// 有限な零・平行参照方向は全体 X/Y で補い、非有限・基底不能は理由付きエラーを返す。
    pub fn from_nodes(
        p_i: [f64; 3],
        p_j: [f64; 3],
        ref_vec: [f64; 3],
    ) -> Result<Self, &'static str> {
        sepika_core::geom::vec3::local_frame_axes(p_i, p_j, ref_vec).map(|rot| Self { rot })
    }

    fn make_r12(&self) -> Vec<f64> {
        let n = 12;
        let mut r = vec![0.0; n * n];
        for b in 0..4 {
            let base = b * 3;
            for i in 0..3 {
                for j in 0..3 {
                    r[(base + i) * n + (base + j)] = self.rot[i][j];
                }
            }
        }
        r
    }

    fn make_r12_transpose(&self) -> Vec<f64> {
        let n = 12;
        let mut rt = vec![0.0; n * n];
        for b in 0..4 {
            let base = b * 3;
            for i in 0..3 {
                for j in 0..3 {
                    rt[(base + i) * n + (base + j)] = self.rot[j][i];
                }
            }
        }
        rt
    }

    pub fn to_global(&self, k_local: &LocalMat) -> LocalMat {
        let n = 12;
        let rt = self.make_r12_transpose();
        let r = self.make_r12();
        let mut tmp = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                let mut s = 0.0;
                for k in 0..n {
                    s += k_local.get(i, k) * r[k * n + j];
                }
                tmp[i * n + j] = s;
            }
        }
        let mut kg = LocalMat::zeros(n);
        for i in 0..n {
            for j in 0..n {
                let mut s = 0.0;
                for k in 0..n {
                    s += rt[i * n + k] * tmp[k * n + j];
                }
                kg.set(i, j, s);
            }
        }
        kg
    }

    pub fn to_local(&self, k_global: &LocalMat) -> LocalMat {
        let r = self.make_r12();
        let rt = self.make_r12_transpose();
        let mut tmp = vec![0.0; 144];
        for i in 0..12 {
            for j in 0..12 {
                for k in 0..12 {
                    tmp[i * 12 + j] += r[i * 12 + k] * k_global.get(k, j);
                }
            }
        }
        let mut local = LocalMat::zeros(12);
        for i in 0..12 {
            for j in 0..12 {
                let value = (0..12).map(|k| tmp[i * 12 + k] * rt[k * 12 + j]).sum();
                local.set(i, j, value);
            }
        }
        local
    }

    pub fn rotate_to_global(&self, v_local: &[f64; 12]) -> [f64; 12] {
        let rt = self.make_r12_transpose();
        let mut vg = [0.0; 12];
        for i in 0..12 {
            let mut s = 0.0;
            for j in 0..12 {
                s += rt[i * 12 + j] * v_local[j];
            }
            vg[i] = s;
        }
        vg
    }

    pub fn rotate_to_local(&self, v_global: &[f64; 12]) -> [f64; 12] {
        let r = self.make_r12();
        let mut vl = [0.0; 12];
        for i in 0..12 {
            let mut s = 0.0;
            for j in 0..12 {
                s += r[i * 12 + j] * v_global[j];
            }
            vl[i] = s;
        }
        vl
    }

    /// 全体座標系の要素変位から、局所座標系の材端力 `f_local = K_local·(R·u)` を返す。
    ///
    /// `u_elem` が 12 成分に満たない場合は `None`。
    pub(crate) fn local_end_forces(&self, k_local: &LocalMat, u_elem: &[f64]) -> Option<[f64; 12]> {
        if u_elem.len() < 12 {
            return None;
        }
        let mut arr = [0.0; 12];
        arr.copy_from_slice(&u_elem[..12]);
        let u_local = self.rotate_to_local(&arr);
        let mut f_local = [0.0; 12];
        for (i, fi) in f_local.iter_mut().enumerate() {
            let mut s = 0.0;
            for (j, &uj) in u_local.iter().enumerate() {
                s += k_local.get(i, j) * uj;
            }
            *fi = s;
        }
        Some(f_local)
    }
}

#[cfg(test)]
mod tests {
    use super::LocalFrame;

    #[test]
    fn local_frame_rotates_member_load_and_force_components_with_reversed_and_rotated_axes() {
        let i = [0.0; 3];
        let j = [0.0, 0.0, 3000.0];
        let frame = LocalFrame::from_nodes(i, j, [1.0, 0.0, 0.0]).unwrap();
        let mut global = [0.0; 12];
        global[0..3].copy_from_slice(&[2.0, 3.0, 5.0]);
        global[6..9].copy_from_slice(&[-7.0, 11.0, 13.0]);
        let local = frame.rotate_to_local(&global);
        assert_eq!(&local[0..3], &[5.0, 2.0, 3.0]);
        assert_eq!(&local[6..9], &[13.0, -7.0, 11.0]);
        assert_eq!(frame.rotate_to_global(&local), global);

        let reversed = LocalFrame::from_nodes(j, i, [1.0, 0.0, 0.0]).unwrap();
        assert_eq!(&reversed.rotate_to_local(&global)[0..3], &[-5.0, 2.0, -3.0]);
        let rotated = LocalFrame::from_nodes(i, j, [0.0, 1.0, 0.0]).unwrap();
        assert_eq!(&rotated.rotate_to_local(&global)[0..3], &[5.0, 3.0, -2.0]);
    }

    #[test]
    fn local_frame_rejects_invalid_reference_before_load_or_force_rotation() {
        for component in 0..3 {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut reference = [1.0, 0.0, 0.0];
                reference[component] = value;
                assert_eq!(
                    LocalFrame::from_nodes([0.0; 3], [0.0, 0.0, 3000.0], reference).unwrap_err(),
                    "断面参照方向に非有限値があります"
                );
            }
        }
    }
}
