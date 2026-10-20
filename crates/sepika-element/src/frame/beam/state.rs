use super::BeamElement;
use crate::behavior::{CheckpointError, ElementUpdateError, LoadedFrameResponse, LocalVec};
use crate::wall::side_column::release_connection::ReleaseConnection;
use sepika_core::model::{EndCondition, MemberLoad};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ElasticLoadState {
    committed_disp: [f64; 12],
    trial_disp: [f64; 12],
    committed_loads: Vec<MemberLoad>,
    trial_loads: Vec<MemberLoad>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ElasticLoadCheckpoint {
    state: ElasticLoadState,
    end_cond: [EndCondition; 2],
    torsion_release: [bool; 2],
    wall_connection: Option<ReleaseConnection>,
}

impl BeamElement {
    pub(crate) fn current_loaded_response(
        &self,
        connection: Option<ReleaseConnection>,
    ) -> Result<LoadedFrameResponse, String> {
        self.loaded_response(&self.trial_disp, &self.trial_member_loads, connection)
    }

    pub(crate) fn update_loaded_state(
        &mut self,
        du: &LocalVec,
        loads: &[MemberLoad],
        commit: bool,
        connection: Option<ReleaseConnection>,
    ) -> Result<(), ElementUpdateError> {
        crate::behavior::validate_trial_increment(du, 12)
            .map_err(|error| ElementUpdateError(format!("部材 {:?}: {error}", self.id)))?;
        let trial = std::array::from_fn(|i| self.trial_disp[i] + du.data[i]);
        self.loaded_response(&trial, loads, connection)
            .and_then(|response| response.section.recover(&self.eval_sections))
            .map_err(ElementUpdateError)?;
        self.trial_disp = trial;
        self.trial_member_loads = loads.to_vec();
        if commit {
            self.committed_disp = trial;
            self.committed_member_loads = loads.to_vec();
        }
        Ok(())
    }

    pub(crate) fn elastic_load_state(&self) -> ElasticLoadState {
        ElasticLoadState {
            committed_disp: self.committed_disp,
            trial_disp: self.trial_disp,
            committed_loads: self.committed_member_loads.clone(),
            trial_loads: self.trial_member_loads.clone(),
        }
    }

    pub(crate) fn restore_elastic_load_state(&mut self, state: ElasticLoadState) {
        self.committed_disp = state.committed_disp;
        self.trial_disp = state.trial_disp;
        self.committed_member_loads = state.committed_loads;
        self.trial_member_loads = state.trial_loads;
    }

    pub(crate) fn restore_loaded_checkpoint(
        &mut self,
        data: &[u8],
        connection: Option<ReleaseConnection>,
    ) -> Result<(), CheckpointError> {
        let checkpoint: ElasticLoadCheckpoint = bincode::deserialize(data)
            .map_err(|error| CheckpointError::Decode(error.to_string()))?;
        if checkpoint.end_cond != self.end_cond
            || checkpoint.torsion_release != self.torsion_release
            || checkpoint.wall_connection != connection
        {
            return Err(CheckpointError::Decode(format!(
                "部材 {:?} のチェックポイント接続設定が一致しません",
                self.id
            )));
        }
        let state = checkpoint.state;
        for (disp, loads) in [
            (&state.committed_disp, &state.committed_loads),
            (&state.trial_disp, &state.trial_loads),
        ] {
            self.loaded_response(disp, loads, connection.clone())
                .and_then(|response| response.section.recover(&self.eval_sections))
                .map_err(CheckpointError::Decode)?;
        }
        self.restore_elastic_load_state(state);
        Ok(())
    }

    pub(crate) fn serialize_loaded_checkpoint(
        &self,
        connection: Option<ReleaseConnection>,
    ) -> Vec<u8> {
        bincode::serialize(&ElasticLoadCheckpoint {
            state: self.elastic_load_state(),
            end_cond: self.end_cond,
            torsion_release: self.torsion_release,
            wall_connection: connection,
        })
        .expect("serialize checkpoint")
    }
}
