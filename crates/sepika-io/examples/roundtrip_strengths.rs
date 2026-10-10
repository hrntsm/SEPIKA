//! STB材料の元指定と採用結果を実入出力で比較する。
use sepika_core::ids::{NodeId, SectionId};
use sepika_core::model::Model;
use sepika_core::model::{StbMemberStrength, StrengthTarget};
fn point(model: &Model, id: NodeId) -> String {
    format!("{:?}", model.node(id).expect("reference node").coord)
}
fn section_key(model: &Model, id: SectionId) -> String {
    let s = model.section(id).expect("section");
    format!(
        "{:?}/{:?}/{}/{}/{}",
        s.name, s.floor, s.area, s.depth, s.width
    )
}
fn member_key(model: &Model, input: &StbMemberStrength) -> String {
    let (kind, section) = match input.target {
        StrengthTarget::Element(id) => {
            let e = model.element(id).unwrap();
            (format!("{:?}", e.kind), e.section)
        }
        StrengthTarget::Secondary(id) => {
            let e = model.secondary_member(id).unwrap();
            (format!("{:?}", e.kind), e.section)
        }
        StrengthTarget::Slab(id) => (
            "Slab".into(),
            model
                .slabs
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .plate
                .section,
        ),
        StrengthTarget::Wall(id) => (
            "Wall".into(),
            model
                .wall_plates
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .section,
        ),
    };
    let boundary: Vec<_> = input
        .node_order
        .iter()
        .map(|id| point(model, *id))
        .collect();
    format!(
        "{kind}/{boundary:?}/reference={}/section={:?}",
        point(model, input.node),
        section.map(|id| section_key(model, id))
    )
}
fn strengths(model: &Model) -> Vec<(String, Option<String>, String, String, f64)> {
    let mut out = Vec::new();
    for input in &model.stb_strengths.members {
        let resolved = model.resolve_stb_concrete(input).expect("Fc resolution");
        out.push((
            member_key(model, input),
            input.concrete.clone(),
            resolved.grade,
            format!("{:?}", resolved.source),
            resolved.value,
        ));
    }
    out.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
            .then(a.4.total_cmp(&b.4))
    });
    out
}
fn section_strengths(model: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for s in &model.stb_strengths.sections {
        let key = section_key(model, s.section);
        out.push(format!("{key}/concrete={:?}", s.concrete));
        for bar in &s.reinforcement {
            out.push(format!(
                "{key}/bar={}/{}/{:?}/{:?}/{:?}/resolved={:?}",
                bar.element,
                bar.part,
                bar.position,
                bar.diameter,
                bar.strength,
                model.resolve_stb_rebar(bar)
            ));
        }
        for steel in &s.steel {
            out.push(format!(
                "{key}/steel={}/{}/{:?}/{}/resolved={:?}",
                steel.element,
                steel.part,
                steel.position,
                steel.strength,
                model.resolve_stb_steel(steel)
            ));
        }
    }
    out.sort();
    out
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let input = args.get(1).ok_or("input STB")?;
    let output = args.get(2).ok_or("output STB")?;
    let xml = sepika_io::stbridge::read_stbridge_file(std::path::Path::new(input))?;
    let (model, report) = sepika_io::stbridge::import_stbridge_with_report(&xml)?;
    model.validate()?;
    println!("import: elements={} secondaries={} slabs={} concrete_inputs={} section_inputs={} diagnostics={:?}",model.elements.len(),model.beams().chain(model.posts()).count(),model.slabs.len(),model.stb_strengths.members.len(),model.stb_strengths.sections.len(),model.stb_strength_diagnostics());
    for warning in report.warnings {
        eprintln!("import: {warning}");
    }
    let (result, report) = sepika_io::stbridge::export_stbridge_with_report(&model)?;
    std::fs::write(output, &result)?;
    let second = sepika_io::stbridge::import_stbridge(&result)?;
    assert_eq!(strengths(&model), strengths(&second));
    assert_eq!(model.stb_strengths.common, second.stb_strengths.common);
    assert_eq!(section_strengths(&model), section_strengths(&second));
    for warning in report.warnings {
        eprintln!("export: {warning}");
    }
    println!(
        "target/reference/section-bound strength and original Common/rebar/steel fields matched"
    );
    Ok(())
}
