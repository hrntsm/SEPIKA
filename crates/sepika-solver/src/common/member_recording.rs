use sepika_core::model::Model;
use sepika_element::behavior::{ElementBehavior, LoadedFrameResponse};

/// 同一試行状態の荷重応答を取得し、所有要素の欠落・不正応答を拒否する。
pub(crate) fn member_recording_responses(
    model: &Model,
    behaviors: &[Box<dyn ElementBehavior>],
    analysis: &str,
) -> Result<Vec<Option<LoadedFrameResponse>>, String> {
    if model.elements.len() != behaviors.len() {
        return Err(format!(
            "{analysis}の部材記録: 要素数と応答数が一致しません"
        ));
    }
    model
        .elements
        .iter()
        .zip(behaviors)
        .map(|(elem, behavior)| {
            if !behavior.owns_frame_member_loads() {
                return Ok(None);
            }
            let error = |reason| format!("要素 ID {} の{analysis}部材記録: {reason}", elem.id.0);
            let response = behavior
                .state_frame_load_response()
                .map_err(error)?
                .ok_or_else(|| error("荷重所有要素の同一試行応答がありません".into()))?;
            if elem.nodes.len() != 2 || response.nodal_residual.data.len() != behavior.n_dof() {
                return Err(error("荷重応答の自由度数が一致しません".into()));
            }
            if response
                .nodal_residual
                .data
                .iter()
                .chain(&response.physical_frame_end_force_global)
                .chain(&response.flex_face_force_local)
                .any(|v| !v.is_finite())
            {
                return Err(error("荷重応答に非有限値があります".into()));
            }
            response.section.recover(&[0.0, 0.5, 1.0]).map_err(error)?;
            Ok(Some(response))
        })
        .collect()
}
