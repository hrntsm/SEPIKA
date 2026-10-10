//! STB材料の元指定と採用結果を実入出力で比較する。
use sepika_core::model::Model;
fn strengths(model: &Model) -> Vec<(String, String, f64)> {
    let mut out = Vec::new();
    for input in &model.stb_strengths.members {
        let resolved = model.resolve_stb_concrete(input).expect("Fc resolution");
        out.push((
            resolved.grade,
            format!("{:?}", resolved.source),
            resolved.value,
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)));
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
    let bars = |model: &Model| {
        let mut values = Vec::new();
        for s in &model.stb_strengths.sections {
            for bar in &s.reinforcement {
                values.push(format!("{:?}", bar));
            }
        }
        values.sort();
        values
    };
    assert_eq!(bars(&model), bars(&second));
    for warning in report.warnings {
        eprintln!("export: {warning}");
    }
    println!("concrete grade/source/value and original Common/rebar fields matched");
    Ok(())
}
