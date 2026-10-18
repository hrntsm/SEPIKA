use super::super::StbError;
use super::xml::{get_opt_u32, get_u32, Attrs};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum SectionFamily {
    ColumnRc,
    ColumnS,
    ColumnSrc,
    ColumnCft,
    BeamRc,
    BeamS,
    BeamSrc,
    BraceS,
    SlabRc,
    SlabDeck,
    SlabPrecast,
    WallRc,
    FoundationRc,
    PileRc,
    PileS,
    PileProduct,
    OpenRc,
    ParapetRc,
    Undefined,
}

impl SectionFamily {
    pub(super) fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "StbSecColumn_RC" => Self::ColumnRc,
            "StbSecColumn_S" => Self::ColumnS,
            "StbSecColumn_SRC" => Self::ColumnSrc,
            "StbSecColumn_CFT" => Self::ColumnCft,
            "StbSecBeam_RC" => Self::BeamRc,
            "StbSecBeam_S" => Self::BeamS,
            "StbSecBeam_SRC" => Self::BeamSrc,
            "StbSecBrace_S" => Self::BraceS,
            "StbSecSlab_RC" => Self::SlabRc,
            "StbSecSlabDeck" => Self::SlabDeck,
            "StbSecSlabPrecast" => Self::SlabPrecast,
            "StbSecWall_RC" => Self::WallRc,
            "StbSecFoundation_RC" => Self::FoundationRc,
            "StbSecPile_RC" => Self::PileRc,
            "StbSecPile_S" => Self::PileS,
            "StbSecPileProduct" => Self::PileProduct,
            "StbSecOpen_RC" => Self::OpenRc,
            "StbSecParapet_RC" => Self::ParapetRc,
            "StbSecUndefined" => Self::Undefined,
            _ => return None,
        })
    }

    pub(super) fn tag(self) -> &'static str {
        match self {
            Self::ColumnRc => "StbSecColumn_RC",
            Self::ColumnS => "StbSecColumn_S",
            Self::ColumnSrc => "StbSecColumn_SRC",
            Self::ColumnCft => "StbSecColumn_CFT",
            Self::BeamRc => "StbSecBeam_RC",
            Self::BeamS => "StbSecBeam_S",
            Self::BeamSrc => "StbSecBeam_SRC",
            Self::BraceS => "StbSecBrace_S",
            Self::SlabRc => "StbSecSlab_RC",
            Self::SlabDeck => "StbSecSlabDeck",
            Self::SlabPrecast => "StbSecSlabPrecast",
            Self::WallRc => "StbSecWall_RC",
            Self::FoundationRc => "StbSecFoundation_RC",
            Self::PileRc => "StbSecPile_RC",
            Self::PileS => "StbSecPile_S",
            Self::PileProduct => "StbSecPileProduct",
            Self::OpenRc => "StbSecOpen_RC",
            Self::ParapetRc => "StbSecParapet_RC",
            Self::Undefined => "StbSecUndefined",
        }
    }

    pub(super) fn is_supported(self) -> bool {
        matches!(
            self,
            Self::ColumnRc
                | Self::ColumnS
                | Self::ColumnSrc
                | Self::ColumnCft
                | Self::BeamRc
                | Self::BeamS
                | Self::BeamSrc
                | Self::BraceS
                | Self::SlabRc
                | Self::SlabDeck
                | Self::WallRc
        )
    }
}

/// 採用済み ST-Bridge 名前空間内の具体的断面系列と正規化元 ID。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct SectionKey {
    pub(super) family: SectionFamily,
    pub(super) id: u32,
}

impl fmt::Display for SectionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.family.tag(), self.id)
    }
}

pub(super) fn section_key(tag: &str, a: &Attrs) -> Result<SectionKey, StbError> {
    let family = SectionFamily::from_tag(tag)
        .ok_or_else(|| StbError::Parse(format!("具体的断面系列ではありません: {tag}")))?;
    let id = get_u32(a, "id")
        .map_err(|error| StbError::Parse(format!("{tag} id={:?}: {error}", a.get("id"))))?;
    Ok(SectionKey { family, id })
}

#[derive(Clone, Debug)]
pub(super) struct MemberSource {
    pub(super) tag: String,
    pub(super) id: Option<u32>,
    pub(super) kind_structure: Option<String>,
    pub(super) id_section: Option<String>,
}

impl MemberSource {
    pub(super) fn error(&self, family: Option<SectionFamily>, reason: &str) -> StbError {
        let family = family
            .or_else(|| {
                self.kind_structure
                    .as_deref()
                    .and_then(|kind| member_family(&self.tag, kind))
            })
            .map(SectionFamily::tag)
            .unwrap_or("解決不能");
        StbError::Unmappable(format!(
            "{} id={:?}, kind_structure={:?}, 要求系列={family}, id_section={:?}: {reason}",
            self.tag, self.id, self.kind_structure, self.id_section
        ))
    }
}

pub(super) fn member_section(
    tag: &str,
    a: &Attrs,
) -> Result<(Option<SectionKey>, MemberSource), StbError> {
    let source = MemberSource {
        tag: tag.to_owned(),
        id: get_opt_u32(a, "id")?,
        kind_structure: a.get("kind_structure").cloned(),
        id_section: a.get("id_section").cloned(),
    };
    if source.id_section.is_none() {
        return Ok((None, source));
    }
    let kind = source
        .kind_structure
        .as_deref()
        .ok_or_else(|| source.error(None, "kind_structure がありません"))?;
    let family = member_family(tag, kind).ok_or_else(|| {
        let reason = if matches!((tag, kind), ("StbBrace", "RC" | "SRC")) {
            "kind_structure は規格上有効ですが対応する断面系列が未対応です"
        } else {
            "kind_structure が不正です"
        };
        source.error(None, reason)
    })?;
    let id = get_u32(a, "id_section").map_err(|error| {
        source.error(
            Some(family),
            &format!("id_section が欠落または不正です: {error}"),
        )
    })?;
    Ok((Some(SectionKey { family, id }), source))
}

fn member_family(tag: &str, kind: &str) -> Option<SectionFamily> {
    use SectionFamily::*;
    Some(match (tag, kind) {
        ("StbColumn" | "StbPost", "RC") => ColumnRc,
        ("StbColumn" | "StbPost", "S") => ColumnS,
        ("StbColumn" | "StbPost", "SRC") => ColumnSrc,
        ("StbColumn" | "StbPost", "CFT") => ColumnCft,
        ("StbColumn" | "StbPost", "UNDEFINED") => Undefined,
        ("StbGirder" | "StbBeam", "RC") => BeamRc,
        ("StbGirder" | "StbBeam", "S") => BeamS,
        ("StbGirder" | "StbBeam", "SRC") => BeamSrc,
        ("StbGirder" | "StbBeam", "UNDEFINED") => Undefined,
        ("StbBrace", "S") => BraceS,
        ("StbSlab", "RC") => SlabRc,
        ("StbSlab", "DECK") => SlabDeck,
        ("StbSlab", "PRECAST") => SlabPrecast,
        ("StbWall", "RC") => WallRc,
        _ => return None,
    })
}
