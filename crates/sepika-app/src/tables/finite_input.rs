#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum EmptyInput {
    Zero,
    Required,
    Optional,
}

pub(super) fn parse_finite(text: &str, empty: EmptyInput) -> Result<Option<f64>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return match empty {
            EmptyInput::Zero => Ok(Some(0.0)),
            EmptyInput::Required => Err("必須空欄"),
            EmptyInput::Optional => Ok(None),
        };
    }
    let value = text.parse::<f64>().map_err(|_| "構文不正")?;
    if !value.is_finite() {
        return Err("非有限");
    }
    Ok(Some(value))
}
