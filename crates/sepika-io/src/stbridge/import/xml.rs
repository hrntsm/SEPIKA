//! ST-Bridge パースの XML 属性・数値ヘルパ。
//!
//! 属性辞書 [`Attrs`] は参照されたキーを記録し、未参照属性の報告に用いる。

use super::super::StbError;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

pub(super) const STB_NAMESPACE: &[u8] = b"https://www.building-smart.or.jp/dl";

pub(super) fn check_element_namespace(
    namespace: quick_xml::name::ResolveResult<'_>,
    name: quick_xml::name::QName<'_>,
) -> Result<(), StbError> {
    use quick_xml::name::ResolveResult;
    let reason = match namespace {
        ResolveResult::Bound(uri) if uri.as_ref() == STB_NAMESPACE => return Ok(()),
        ResolveResult::Bound(uri) => format!(
            "非採用 namespace={:?}",
            String::from_utf8_lossy(uri.as_ref())
        ),
        ResolveResult::Unbound => "namespace がありません".to_owned(),
        ResolveResult::Unknown(prefix) => {
            format!("未束縛 prefix={:?}", String::from_utf8_lossy(&prefix))
        }
    };
    Err(StbError::Parse(format!(
        "要素 {}: {reason}（要求 namespace={}）",
        String::from_utf8_lossy(name.as_ref()),
        String::from_utf8_lossy(STB_NAMESPACE)
    )))
}

pub(super) fn check_attribute_prefixes(
    resolver: &quick_xml::name::NamespaceResolver,
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<(), StbError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| StbError::Parse(error.to_string()))?;
        if let quick_xml::name::ResolveResult::Unknown(prefix) =
            resolver.resolve_attribute(attribute.key).0
        {
            return Err(StbError::Parse(format!(
                "要素 {} 属性 {}: 未束縛 prefix={:?}",
                String::from_utf8_lossy(element.name().as_ref()),
                String::from_utf8_lossy(attribute.key.as_ref()),
                String::from_utf8_lossy(&prefix)
            )));
        }
    }
    Ok(())
}

/// XML 要素 1 個分の属性辞書。参照されたキーを内部で記録する。
#[derive(Debug, Default)]
pub(super) struct Attrs {
    map: HashMap<String, String>,
    read: RefCell<HashSet<String>>,
}

impl Attrs {
    /// 属性値を取得し、キーを参照済みとして記録する。
    pub(super) fn get(&self, k: &str) -> Option<&String> {
        let seen = self.read.borrow().contains(k);
        if !seen {
            self.read.borrow_mut().insert(k.to_string());
        }
        self.map.get(k)
    }

    /// 要素に存在した属性名（名前昇順）。
    pub(super) fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.map.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    /// 要素に存在したが一度も参照されなかった属性名（名前昇順）。
    pub(super) fn unread(&self) -> Vec<&str> {
        let read = self.read.borrow();
        let mut v: Vec<&str> = self
            .map
            .keys()
            .filter(|k| !read.contains(*k))
            .map(String::as_str)
            .collect();
        v.sort_unstable();
        v
    }
}

impl FromIterator<(String, String)> for Attrs {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self {
            map: iter.into_iter().collect(),
            read: RefCell::new(HashSet::new()),
        }
    }
}

/// `StbNodeIdOrder` の内容文字列を境界へ追加する（スラブ・壁共用）。
pub(super) fn push_node_id_tokens(text: &str, boundary: &mut Vec<u32>) -> Result<(), StbError> {
    for tok in text.split_whitespace() {
        let id = tok.parse::<u32>().map_err(|_| {
            StbError::Parse(format!(
                "bad u32 StbNodeIdOrder: {tok}, expected integer 0..=4294967295"
            ))
        })?;
        boundary.push(id);
    }
    Ok(())
}

pub(super) fn attrs(e: &quick_xml::events::BytesStart) -> Result<Attrs, StbError> {
    let mut m: Vec<(String, String)> = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|err| StbError::Parse(err.to_string()))?;
        let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
        let val = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|err| StbError::Parse(err.to_string()))?
            .to_string();
        m.push((key, val));
    }
    Ok(m.into_iter().collect())
}

pub(super) fn get_f64(a: &Attrs, k: &str) -> Result<f64, StbError> {
    a.get(k)
        .ok_or_else(|| StbError::Parse(format!("missing attr {k}")))?
        .parse::<f64>()
        .map_err(|_| StbError::Parse(format!("bad f64 attr {k}")))
}

/// 複数の候補キーのいずれかから f64 を取る。
pub(super) fn get_f64_any(a: &Attrs, keys: &[&str]) -> Result<f64, StbError> {
    for k in keys {
        if let Some(v) = a.get(k) {
            return v
                .parse::<f64>()
                .map_err(|_| StbError::Parse(format!("bad f64 attr {k}")));
        }
    }
    Err(StbError::Parse(format!("missing attr {:?}", keys)))
}

pub(super) fn get_opt_f64(a: &Attrs, k: &str) -> Option<f64> {
    match a.get(k) {
        Some(v) if !v.is_empty() => v.parse::<f64>().ok(),
        _ => None,
    }
}

pub(super) fn get_u32(a: &Attrs, k: &str) -> Result<u32, StbError> {
    a.get(k)
        .ok_or_else(|| StbError::Parse(format!("missing attr {k}")))?
        .parse::<u32>()
        .map_err(|_| StbError::Parse(format!("bad u32 attr {k}: expected integer 0..=4294967295")))
}

pub(super) fn get_opt_u32(a: &Attrs, k: &str) -> Result<Option<u32>, StbError> {
    a.get(k).map(|_| get_u32(a, k)).transpose()
}
