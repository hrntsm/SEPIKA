//! 地震用重量の入力識別と階生成結果の適用。

use sepika_core::model::{Constraint, LoadSource, Model, SeismicWeightGeneration};
use sepika_load::story_gen::StoryGenResult;

/// 派生重量・自動荷重・表示名を除いた重量依存入力を返す。
pub fn weight_input_key(model: &Model, mass_method: sepika_core::model::MassMethod) -> Vec<u8> {
    let mut input = model.clone();
    input.seismic_weight_generation = None;
    input.damper_mass_generation = None;
    input.axes.clear();
    input.stb_node_ids.clear();
    input.vibration_cases.clear();
    input.lumped_vibration_cases.clear();
    input.combinations.clear();
    input.mass_method = mass_method;
    input
        .nodes
        .retain(|node| !model.generated_masters.contains(&node.id));
    for node in &mut input.nodes {
        node.story = None;
    }
    for element in &mut input.elements {
        element.rigid_zone.face_i = None;
        element.rigid_zone.face_j = None;
        if element.rigid_zone.source_i == sepika_core::model::ZoneSource::Auto {
            element.rigid_zone.length_i = 0.0;
        }
        if element.rigid_zone.source_j == sepika_core::model::ZoneSource::Auto {
            element.rigid_zone.length_j = 0.0;
        }
    }
    for story in &mut input.stories {
        story.name.clear();
        story.node_ids.clear();
        story.seismic_weight = None;
        story.dynamic_mass = None;
        story.structure = Default::default();
    }
    for section in &mut input.sections {
        section.name.clear();
    }
    for material in &mut input.materials {
        material.name.clear();
    }
    for story in &mut input.source_stories {
        story.name.clear();
    }
    input.generated_masters.clear();
    input
        .constraints
        .retain(|constraint| !model.is_automatic_seismic_diaphragm(constraint));
    let manual_master_settings: Vec<_> = model
        .nodes
        .iter()
        .filter(|node| {
            model.generated_masters.contains(&node.id)
                && (!model.is_automatic_seismic_master_restraint(node.id)
                    || node.support_spring.is_some())
        })
        .map(|node| (node.id, node.restraint, node.support_spring))
        .collect();
    for case in &mut input.load_cases {
        case.nodal.retain(|load| load.source == LoadSource::Manual);
        case.member.retain(|load| load.source == LoadSource::Manual);
    }
    input
        .load_cases
        .retain(|case| !(case.nodal.is_empty() && case.member.is_empty()));
    for region in &mut input.floor_regions {
        region.name.clear();
        for member in &mut region.secondary_beams {
            member.name.clear();
        }
    }
    for region in &mut input.wall_regions {
        region.name.clear();
        for member in &mut region.posts {
            member.name.clear();
        }
    }
    for member in input
        .unassigned_beams
        .iter_mut()
        .chain(&mut input.unassigned_posts)
    {
        member.name.clear();
    }
    bincode::serialize(&(input, manual_master_settings)).expect("重量依存入力の直列化")
}

/// 現在保持している派生重量・質量の内容を識別する。
pub fn weight_output_key(model: &Model) -> Vec<u8> {
    let stories: Vec<_> = model
        .stories
        .iter()
        .map(|story| {
            (
                story.id,
                &story.node_ids,
                story.seismic_weight,
                story.dynamic_mass,
            )
        })
        .collect();
    let nodes: Vec<_> = model
        .nodes
        .iter()
        .map(|node| {
            (
                node.id,
                node.story,
                model.generated_masters.contains(&node.id).then_some(node),
            )
        })
        .collect();
    bincode::serialize(&(
        stories,
        nodes,
        &model.constraints,
        &model.generated_masters,
        model.mass_method,
        &model.damper_mass_generation,
    ))
    .expect("重量生成出力の直列化")
}

/// 入力一致に加え、保存・編集後の生成出力の改変も検出する。
pub fn weights_are_current(model: &Model, mass_method: sepika_core::model::MassMethod) -> bool {
    model
        .seismic_weight_generation
        .as_ref()
        .is_some_and(|record| {
            record.input_key == weight_input_key(model, mass_method)
                && record.output_key == weight_output_key(model)
        })
}

/// 階生成結果を作業コピーへ反映する。非剛床拘束は保持する。
pub fn apply_generated_weights(
    model: &mut Model,
    generated: StoryGenResult,
    mass_method: sepika_core::model::MassMethod,
) {
    let calculated_weights = generated.calculated_weights.clone();
    let automatic_diaphragms: Vec<Constraint> = generated
        .constraints
        .iter()
        .filter(|constraint| {
            let Constraint::RigidDiaphragm {
                master,
                ci_override,
                ..
            } = constraint
            else {
                return false;
            };
            let previous: Vec<_> = model
                .constraints
                .iter()
                .filter(|old| {
                    matches!(old,
            Constraint::RigidDiaphragm { master: old_master, .. } if old_master == master)
                })
                .collect();
            ci_override.is_none()
                && (previous.is_empty()
                    || previous
                        .iter()
                        .all(|old| model.is_automatic_seismic_diaphragm(old)))
        })
        .cloned()
        .collect();
    let automatic_master_restraints = generated
        .rep_nodes
        .iter()
        .filter(|node| {
            !model.constraints.iter().any(|constraint| {
                matches!(constraint,
                Constraint::RigidDiaphragm { master, .. } if *master == node.id)
            }) || model.is_automatic_seismic_master_restraint(node.id)
        })
        .map(|node| (node.id, node.restraint))
        .collect();
    model.stories = generated.stories;
    for (node, story) in model.nodes.iter_mut().zip(generated.node_story) {
        node.story = story;
    }
    model
        .constraints
        .retain(|constraint| !matches!(constraint, Constraint::RigidDiaphragm { .. }));
    model.constraints.extend(generated.constraints);
    for node in generated.rep_nodes {
        let index = node.id.index();
        if index < model.nodes.len() {
            model.nodes[index] = node;
        } else {
            model.nodes.push(node);
        }
    }
    model.generated_masters = generated.generated_masters;
    model.mass_method = mass_method;
    model.damper_mass_generation = Some(generated.damper_mass_generation);
    model.seismic_weight_generation = Some(SeismicWeightGeneration {
        input_key: Vec::new(),
        output_key: Vec::new(),
        calculated_weights,
        automatic_diaphragms,
        automatic_master_restraints,
    });
    let input_key = weight_input_key(model, mass_method);
    let output_key = weight_output_key(model);
    let record = model.seismic_weight_generation.as_mut().expect("生成記録");
    record.input_key = input_key;
    record.output_key = output_key;
}

/// 生成済み物理質量が現在の入力・生成出力と一致することを確認する。
/// 利用者の手入力質量だけを持つ未生成モデルには適用しない。
pub fn require_current_generated_mass(model: &Model) -> Result<(), crate::error::JobError> {
    if model.seismic_weight_generation.is_some() && !weights_are_current(model, model.mass_method) {
        return Err(crate::error::JobError::InvalidInput("派生した物理質量が現在入力と一致しません。重量生成の原因を修正して再生成してください。".into()));
    }
    Ok(())
}
