use super::BeamElement;
use crate::behavior::{LoadedFrameResponse, LocalVec};
use crate::frame::member_load::{FrameDeadLoadMap, LoadedFrameSection};
use crate::frame::rigid_arm;
use crate::wall::side_column::release_connection::ReleaseConnection;
use sepika_core::model::MemberLoad;
use sepika_math::dense_lu::CheckedDenseLu;

impl BeamElement {
    pub(crate) fn loaded_response(
        &self,
        u_elem: &[f64],
        loads: &[MemberLoad],
        connection: Option<ReleaseConnection>,
    ) -> Result<LoadedFrameResponse, String> {
        self.try_loaded_response(u_elem, loads, connection)
            .map_err(|reason| format!("部材 {:?} の線形部材荷重応答: {reason}", self.id))
    }

    fn try_loaded_response(
        &self,
        u_elem: &[f64],
        loads: &[MemberLoad],
        connection: Option<ReleaseConnection>,
    ) -> Result<LoadedFrameResponse, String> {
        let u_global: &[f64; 12] = u_elem.try_into().map_err(|_| "変位は12自由度が必要です")?;
        if u_global.iter().any(|v| !v.is_finite()) {
            return Err("変位が非有限です".into());
        }
        if loads.iter().any(|load| load.elem != self.id) {
            return Err("異なる部材IDの荷重が含まれています".into());
        }
        let (li, lj) = self.rigid_lengths();
        let map = FrameDeadLoadMap::new(loads, &self.axis, self.length, li, lj)?;
        let raw = self.local_stiffness_flex_raw();
        let tangent = std::array::from_fn(|i| std::array::from_fn(|j| raw.get(i, j)));
        let mut connection = match connection {
            Some(connection) => connection,
            None => ReleaseConnection::from_end_conditions(
                self.end_cond,
                self.torsion_release,
                self.j > 0.0 && self.g > 0.0,
            )
            .map_err(|error| error.to_string())?,
        };
        connection
            .fix_isolated_torsion_gauge(&tangent)
            .map_err(|e| e.to_string())?;
        let u = rigid_arm::to_flex_disp(&self.axis.rotate_to_local(u_global), li, lj);
        let qf = map.flex_load();
        let d = rigid_arm::to_flex_force(map.rigid_node_load(), li, lj);
        let pa = std::array::from_fn(|i| {
            d[i] + (0..12).map(|j| connection.a()[j][i] * qf[j]).sum::<f64>()
        });
        let pb: Vec<_> = connection
            .b_columns()
            .iter()
            .map(|column| dot(column, qf))
            .collect();
        let n = pb.len();
        let mut b = vec![0.0; n];
        let e = connection
            .flexible_displacement(&u, &b)
            .map_err(|e| e.to_string())?;
        let fel = multiply(&tangent, &e);
        let initial = connection
            .trial_response(&u, &b, &fel, &tangent, &pa, &pb)
            .map_err(|e| e.to_string())?;
        if n > 0 {
            b = CheckedDenseLu::factorize(&initial.kbb, n)
                .and_then(|lu| {
                    lu.solve(&initial.residual.iter().map(|v| -v).collect::<Vec<_>>(), 1)
                })
                .map_err(|error| format!("内部回転の局所線形解に失敗しました: {error}"))?;
        }
        let e = connection
            .flexible_displacement(&u, &b)
            .map_err(|e| e.to_string())?;
        let fel = multiply(&tangent, &e);
        let response = connection
            .trial_response(&u, &b, &fel, &tangent, &pa, &pb)
            .map_err(|e| e.to_string())?;
        let gf = std::array::from_fn(|i| fel[i] - qf[i]);
        let node_elastic = rigid_arm::to_node_force(&gf, li, lj);
        let node_force = std::array::from_fn(|i| node_elastic[i] - map.rigid_node_load()[i]);
        let physical_frame_end_force_global = self.axis.rotate_to_global(&node_force);
        let residual =
            self.axis
                .rotate_to_global(&rigid_arm::to_node_force(&response.retained_force, li, lj));
        if residual
            .iter()
            .chain(&physical_frame_end_force_global)
            .chain(&gf)
            .any(|v| !v.is_finite())
        {
            return Err("荷重込み端力が非有限です".into());
        }
        Ok(LoadedFrameResponse {
            nodal_residual: LocalVec {
                data: smallvec::SmallVec::from_slice(&residual),
            },
            physical_frame_end_force_global,
            flex_face_force_local: gf,
            section: LoadedFrameSection::new(map, node_force),
        })
    }
}

fn dot(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn multiply(matrix: &[[f64; 12]; 12], u: &[f64; 12]) -> [f64; 12] {
    std::array::from_fn(|i| dot(&matrix[i], u))
}
