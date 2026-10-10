//! 外部 STB の原階・節点識別子の意味的往復を確認し、標準出力を保存する。

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let input = args.get(1).ok_or("入力STBが必要です")?;
    let output = args.get(2).ok_or("出力STBが必要です")?;
    let xml = sepika_io::stbridge::read_stbridge_file(std::path::Path::new(input))?;
    let (model, report) = sepika_io::stbridge::import_stbridge_with_report(&xml)?;
    model.validate()?;
    let (result, export_report) = sepika_io::stbridge::export_stbridge_with_report(&model)?;
    let second = sepika_io::stbridge::import_stbridge(&result)?;
    assert_eq!(model.source_stories, second.source_stories);
    assert_eq!(model.stb_node_ids, second.stb_node_ids);
    assert_eq!(result, sepika_io::stbridge::export_stbridge(&model)?);
    std::fs::write(output, result)?;
    println!(
        "原階{}件・節点識別子{}件の意味的往復と無編集連続exportが一致",
        model.source_stories.len(),
        model.stb_node_ids.len()
    );
    for warning in report.warnings.iter().chain(&export_report.warnings) {
        eprintln!("{warning}");
    }
    Ok(())
}
