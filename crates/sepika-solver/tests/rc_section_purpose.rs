use sepika_core::{dof::Dof6Mask, ids::*, model::*, section_shape::*};
use sepika_solver::statics::{
    analysis::{
        precheck::{model_issues, IssueSeverity},
        Analysis,
    },
    linear::linear_static_once,
};

fn shapes() -> Vec<(
    SectionShape,
    FrameSectionUse,
    SecondaryMemberKind,
    &'static str,
)> {
    vec![
        (
            SectionShape::RcBeamRect {
                b: 300.0,
                d: 500.0,
                rebar: RcBeamRebar {
                    main_dia: 22.0,
                    top: vec![3],
                    bottom: vec![4],
                    cover: 40.0,
                    stirrup: BeamStirrup {
                        dia: 10.0,
                        pitch: 150.0,
                        legs: 2,
                    },
                },
            },
            FrameSectionUse::Girder,
            SecondaryMemberKind::Post,
            "RC 梁用断面は大梁・小梁専用",
        ),
        (
            SectionShape::RcColumnRect {
                b: 600.0,
                d: 600.0,
                rebar: RcRectColumnRebar {
                    main_dia: 19.0,
                    x: vec![4],
                    y: vec![4],
                    cover: 40.0,
                    hoop: RectColumnHoop {
                        dia: 10.0,
                        pitch: 150.0,
                        legs_x: 2,
                        legs_y: 2,
                    },
                },
            },
            FrameSectionUse::Column,
            SecondaryMemberKind::Beam,
            "RC 柱用断面は柱・間柱専用",
        ),
        (
            SectionShape::RcColumnCircle {
                d: 600.0,
                rebar: RcCircleColumnRebar {
                    main_dia: 19.0,
                    count: 12,
                    cover: 40.0,
                    hoop: CircleColumnHoop {
                        dia: 10.0,
                        pitch: 150.0,
                    },
                },
            },
            FrameSectionUse::Column,
            SecondaryMemberKind::Beam,
            "RC 柱用断面は柱・間柱専用",
        ),
    ]
}

#[test]
fn rc_purpose_rejects_inverse_primary_and_secondary_uses_before_public_analysis() {
    for (shape, correct, wrong_kind, reason) in shapes() {
        shape.validate_rebar().unwrap();
        for secondary_use in [None, Some(None), Some(Some(correct))] {
            let mut model = Model::default();
            let mut section = shape.clone().to_section(SectionId(0), "RC用途".into());
            if let Some(usage) = secondary_use {
                section.frame_use = usage;
                let sm = SecondaryMember {
                    id: SecondaryMemberId(674),
                    kind: wrong_kind,
                    ends: SecondaryMemberEnds::Detached([[0.0; 3], [1000.0, 0.0, 0.0]]),
                    section: Some(SectionId(0)),
                    name: "逆用途".into(),
                    gravity_end_shares: None,
                };
                match wrong_kind {
                    SecondaryMemberKind::Beam => model.unassigned_beams.push(sm),
                    SecondaryMemberKind::Post => model.unassigned_posts.push(sm),
                }
            } else {
                section.frame_use = Some(if correct == FrameSectionUse::Column {
                    FrameSectionUse::Girder
                } else {
                    FrameSectionUse::Column
                });
                for id in 0..2 {
                    model.nodes.push(Node {
                        id: NodeId(id),
                        coord: [f64::from(id) * 3000.0, 0.0, 0.0],
                        restraint: Dof6Mask::FIXED,
                        mass: None,
                        story: None,
                        support_spring: None,
                    });
                }
                model.elements.push(ElementData {
                    id: ElemId(0),
                    kind: ElementKind::Beam,
                    nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
                    section: Some(SectionId(0)),
                    local_axis: LocalAxis {
                        ref_vector: [0.0, 0.0, 1.0],
                    },
                    end_cond: [EndCondition::Fixed; 2],
                    force_regime: ForceRegime::Auto,
                    rigid_zone: Default::default(),
                    plastic_zone: None,
                    spring: None,
                });
            }
            model.sections.push(section);
            let before = model.clone();
            let issues = model_issues(&model);
            assert_eq!(issues.len(), 1);
            assert_eq!(issues[0].severity, IssueSeverity::Error);
            assert!(issues[0].message.contains(reason), "{}", issues[0].message);
            let error = match Analysis::prepare(&model) {
                Ok(_) => panic!("逆用途を受理"),
                Err(error) => error.to_string(),
            };
            assert!(error.contains(reason), "{error}");
            let error = match linear_static_once(&model, LoadCaseId(0)) {
                Ok(_) => panic!("逆用途を受理"),
                Err(error) => error.to_string(),
            };
            assert!(error.contains(reason), "{error}");
            assert!(model.eq_ignoring_dofmap(&before));
        }
    }
}
