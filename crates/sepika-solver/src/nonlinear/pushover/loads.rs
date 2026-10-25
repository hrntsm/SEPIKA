use sepika_core::ids::LoadCaseId;
use sepika_core::model::{LoadCase, LoadCaseKind, MemberLoad, MemberLoadKind, Model};

pub(super) struct PhysicalPushoverLoads {
    pub(super) gravity_cases: Vec<(LoadCase, f64)>,
    pub(super) reference: LoadCase,
}

impl PhysicalPushoverLoads {
    pub(super) fn new(
        model: &Model,
        terms: &[(LoadCaseId, f64)],
        reference: LoadCase,
    ) -> Result<Self, String> {
        let gravity_cases = terms
            .iter()
            .map(|&(id, factor)| {
                let source = model
                    .load_cases
                    .iter()
                    .find(|case| case.id == id)
                    .ok_or_else(|| format!("増分解析: 重力荷重ケース {} がありません", id.0))?;
                Ok((source.clone(), factor))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            gravity_cases,
            reference,
        })
    }

    pub(super) fn gravity_case(&self) -> LoadCase {
        let mut combined = LoadCase {
            id: LoadCaseId(0),
            name: "増分解析の重力荷重".into(),
            nodal: Vec::new(),
            member: Vec::new(),
            kind: LoadCaseKind::Other,
        };
        for (source, factor) in &self.gravity_cases {
            combined
                .nodal
                .extend(source.nodal.iter().cloned().map(|mut load| {
                    for value in &mut load.values {
                        *value *= factor;
                    }
                    load
                }));
            combined.member.extend(
                source
                    .member
                    .iter()
                    .map(|load| scaled_member(load, *factor)),
            );
        }
        combined
    }
}

fn scaled_member(load: &MemberLoad, factor: f64) -> MemberLoad {
    let mut scaled = load.clone();
    match &mut scaled.kind {
        MemberLoadKind::Point { p, .. } => *p *= factor,
        MemberLoadKind::Distributed { w1, w2, .. } => {
            *w1 *= factor;
            *w2 *= factor;
        }
    }
    scaled
}
