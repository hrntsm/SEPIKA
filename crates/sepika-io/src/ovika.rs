use crate::manifest::Manifest;
use sepika_core::model::Model;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// manifest への記載が必須な zip エントリ。
const REQUIRED_ENTRIES: [&str; 2] = ["model.msgpack", "settings.json"];

/// zip エントリ 1 個あたりの最大展開サイズ [byte]。
const MAX_ENTRY_UNCOMPRESSED: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(String),
    #[error("decode: {0}")]
    Decode(String),
    #[error("hash mismatch for entry {0}")]
    HashMismatch(String),
    #[error("manifest missing required entry {0}")]
    MissingEntry(String),
    #[error("unsupported schema version: {0}")]
    UnsupportedVersion(u32),
    #[error("entry {0} exceeds max uncompressed size")]
    EntryTooLarge(String),
}

fn sha256_of(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// zip エントリを展開サイズ上限付きで読み込む（zip 爆弾対策）。
/// ヘッダ申告サイズで早期に弾き、申告が嘘でも `take` で実バイトを上限に縛る
/// （メモリを使い切る前に止まる）。
fn read_entry_capped(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<Vec<u8>, IoError> {
    let mut zf = archive
        .by_name(name)
        .map_err(|e| IoError::Zip(format!("missing entry {}: {}", name, e)))?;
    if zf.size() > MAX_ENTRY_UNCOMPRESSED {
        return Err(IoError::EntryTooLarge(name.to_string()));
    }
    let mut data = Vec::new();
    let read = (&mut zf)
        .take(MAX_ENTRY_UNCOMPRESSED + 1)
        .read_to_end(&mut data)?;
    if read as u64 > MAX_ENTRY_UNCOMPRESSED {
        return Err(IoError::EntryTooLarge(name.to_string()));
    }
    Ok(data)
}

/// 準備計算の結果を格納する zip エントリ名（任意エントリ）。
pub const PREPARATION_ENTRY: &str = "preparation.msgpack";

/// 解析結果を格納する zip エントリ名（任意エントリ）。
pub const RESULTS_ENTRY: &str = "results.msgpack";

/// 解析タブの設定値（`AnalysisSettings`）を格納する zip エントリ名（任意エントリ）。
pub const ANALYSIS_SETTINGS_ENTRY: &str = "analysis_settings.msgpack";

/// モデル以外に .ovika へ同梱する付随データ。
///
/// - `preparation`・`results` はモデルから再計算できるが、再計算が高価なため
///   保存して復元するもの。
/// - `analysis_settings` はモデルから導出できない独立した設定値（時刻歴の
///   波形パラメータ・減衰モデル、固有値解析のモード数など）。同梱しないと
///   `results` を生成した条件が失われ、結果の再現性が保てない。
///
/// 中身は呼び出し側（アプリ）が msgpack へ直列化したバイト列であり、io 層は
/// 内容を解釈しない（各データの型はアプリ層にあるため）。`None` の項目は
/// エントリを書かない。書く項目は manifest に列挙してハッシュ検証の対象にする。
#[derive(Default, Clone, Copy)]
pub struct OvikaExtras<'a> {
    /// 準備計算の結果。
    pub preparation: Option<&'a [u8]>,
    /// 解析結果。
    pub results: Option<&'a [u8]>,
    /// 解析タブの設定値（`results` を生成した条件。`sepika_job::AnalysisSettings`）。
    pub analysis_settings: Option<&'a [u8]>,
}

/// [`load_ovika`] の返り値。モデルと、同梱されていれば付随データ。
pub struct OvikaContents {
    pub model: Model,
    /// 準備計算の結果（同梱がなければ `None`）。
    pub preparation: Option<Vec<u8>>,
    /// 解析結果（同梱がなければ `None`）。
    pub results: Option<Vec<u8>>,
    /// 解析タブの設定値（同梱がなければ `None`）。
    pub analysis_settings: Option<Vec<u8>>,
}

/// モデルと派生データを .ovika へ保存する。
pub fn save_ovika(path: &Path, model: &Model, extras: OvikaExtras<'_>) -> Result<(), IoError> {
    model
        .validate_member_load_extents()
        .map_err(IoError::Decode)?;
    let tmp_path = path.with_extension("ovika.tmp");

    let model_bytes = rmp_serde::to_vec_named(model).map_err(|e| IoError::Decode(e.to_string()))?;
    let settings_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "code": "JIS B 0001",
        "created_at": "",
    }))
    .map_err(|e| IoError::Decode(e.to_string()))?;

    let model_hash = sha256_of(&model_bytes);
    let settings_hash = sha256_of(&settings_bytes);

    let mut entries = vec![
        crate::manifest::EntryHash {
            name: "model.msgpack".to_string(),
            sha256: model_hash,
        },
        crate::manifest::EntryHash {
            name: "settings.json".to_string(),
            sha256: settings_hash,
        },
    ];
    for (name, data) in [
        (PREPARATION_ENTRY, extras.preparation),
        (RESULTS_ENTRY, extras.results),
        (ANALYSIS_SETTINGS_ENTRY, extras.analysis_settings),
    ] {
        if let Some(data) = data {
            entries.push(crate::manifest::EntryHash {
                name: name.to_string(),
                sha256: sha256_of(data),
            });
        }
    }

    let manifest = Manifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        units: "internal: N-mm-s".to_string(),
        created_by: "SEPIKA".to_string(),
        entries,
    };

    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| IoError::Decode(e.to_string()))?;

    {
        let f = std::fs::File::create(&tmp_path)?;
        let mut zip = zip::ZipWriter::new(f);
        let opts = zip::write::FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("manifest.json", opts)
            .map_err(|e| IoError::Zip(e.to_string()))?;
        zip.write_all(&manifest_bytes)?;

        zip.start_file("model.msgpack", opts)
            .map_err(|e| IoError::Zip(e.to_string()))?;
        zip.write_all(&model_bytes)?;

        zip.start_file("settings.json", opts)
            .map_err(|e| IoError::Zip(e.to_string()))?;
        zip.write_all(&settings_bytes)?;

        for (name, data) in [
            (PREPARATION_ENTRY, extras.preparation),
            (RESULTS_ENTRY, extras.results),
            (ANALYSIS_SETTINGS_ENTRY, extras.analysis_settings),
        ] {
            if let Some(data) = data {
                zip.start_file(name, opts)
                    .map_err(|e| IoError::Zip(e.to_string()))?;
                zip.write_all(data)?;
            }
        }

        let f = zip.finish().map_err(|e| IoError::Zip(e.to_string()))?;
        f.sync_all()?;
    }

    std::fs::rename(&tmp_path, path)?;
    sync_parent_dir(path)?;
    Ok(())
}

/// rename というディレクトリエントリ変更自体を永続化する（Unix のみ。
/// Windows はディレクトリを fsync できないため no-op）。
#[cfg(unix)]
fn sync_parent_dir(path: &Path) -> std::io::Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_dir(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// モデルと、同梱されていれば付随データ（準備計算の結果・解析結果・解析タブの
/// 設定値）を読み込む。該当エントリを持たないファイル（それらが最新でない状態で
/// 保存したプロジェクト）では [`OvikaContents`] の該当項目が `None` になる。
pub fn load_ovika(path: &Path) -> Result<OvikaContents, IoError> {
    let f = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| IoError::Zip(e.to_string()))?;

    let manifest_bytes = read_entry_capped(&mut archive, "manifest.json")?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| IoError::Decode(e.to_string()))?;

    if manifest.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(IoError::UnsupportedVersion(manifest.schema_version));
    }

    for required in REQUIRED_ENTRIES {
        if !manifest.entries.iter().any(|e| e.name == required) {
            return Err(IoError::MissingEntry(required.to_string()));
        }
    }

    let mut model_data = None;
    let mut preparation = None;
    let mut results = None;
    let mut analysis_settings = None;
    for entry in &manifest.entries {
        let data = read_entry_capped(&mut archive, &entry.name)?;
        let actual_hash = sha256_of(&data);
        if actual_hash != entry.sha256 {
            return Err(IoError::HashMismatch(entry.name.clone()));
        }
        match entry.name.as_str() {
            "model.msgpack" => model_data = Some(data),
            PREPARATION_ENTRY => preparation = Some(data),
            RESULTS_ENTRY => results = Some(data),
            ANALYSIS_SETTINGS_ENTRY => analysis_settings = Some(data),
            _ => {}
        }
    }

    let model_data = model_data.ok_or_else(|| IoError::MissingEntry("model.msgpack".into()))?;

    let model: Model =
        rmp_serde::from_slice(&model_data).map_err(|e| IoError::Decode(e.to_string()))?;
    model
        .validate_attached_slabs()
        .map_err(|e| IoError::Decode(e.to_string()))?;

    model
        .validate_member_load_extents()
        .map_err(IoError::Decode)?;

    Ok(OvikaContents {
        model,
        preparation,
        results,
        analysis_settings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::*;
    use sepika_core::model::*;
    use sepika_core::section_shape::SectionShape;

    #[test]
    fn full_length_and_fixed_extent_roundtrip_without_numeric_inference() {
        let mut model = make_3node_model();
        model.nodes[1].coord = [8000.0, 0.0, 0.0];
        model.elements.push(ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: vec![NodeId(0), NodeId(1)].into(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
        model.load_cases = default_load_cases();
        let mut full = MemberLoad::full_length_uniform(ElemId(0), [0.0, 0.0, -1.0], 8000.0, 10.0);
        full.name = "機器".into();
        let fixed = MemberLoad::manual(ElemId(0), [0.0, 0.0, -1.0], full.kind.clone());
        model.load_cases[0].member = vec![full, fixed];
        let path = crate::test_util::test_tmp().join("member-load-extents.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let loaded = load_ovika(&path).unwrap().model;
        assert_eq!(loaded.nodes, model.nodes);
        assert_eq!(loaded.load_cases, model.load_cases);
        assert_eq!(
            loaded.load_cases[0].member[0].extent,
            MemberLoadExtent::FullLengthUniform
        );
        assert_eq!(
            loaded.load_cases[0].member[1].extent,
            MemberLoadExtent::FixedDistance
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn load_rejects_attached_slab_with_common_geometry_diagnostic() {
        let mut model = Model::default();
        for (i, x) in [0.0, 4000.0].into_iter().enumerate() {
            model.nodes.push(Node {
                id: NodeId(i as u32),
                coord: [x, 0.0, 0.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        model.slabs.push(Slab {
            id: SlabId(0),
            shape: SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span: [0.0, 1.0],
                    transfer: LoadTransfer::Anchor,
                },
                extent: [1000.0, -1000.0],
            },
            plate: SlabPlate::default(),
            tip_loads: vec![],
        });
        let expected = model.validate_attached_slabs().unwrap_err().to_string();
        let path = crate::test_util::test_tmp().join("attached-invalid.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        assert!(matches!(load_ovika(&path),Err(IoError::Decode(reason)) if reason==expected));
    }
    #[test]
    fn surface_radii_ovika_roundtrip() {
        let mut model = Model::default();
        for radius in [None, Some(0.0), Some(13.0)] {
            for shape in [
                SectionShape::SteelH {
                    height: 400.0,
                    width: 200.0,
                    web_thick: 8.0,
                    flange_thick: 13.0,
                    root_r: radius,
                },
                SectionShape::SteelBox {
                    height: 400.0,
                    width: 300.0,
                    thick: 12.0,
                    corner_r: radius,
                },
                SectionShape::CftBox {
                    height: 400.0,
                    width: 300.0,
                    thick: 12.0,
                    corner_r: radius,
                },
            ] {
                let id = SectionId(model.sections.len() as u32);
                model.sections.push(
                    shape
                        .input_section(id, format!("フィレット半径・角R{}", id.0))
                        .unwrap(),
                );
            }
        }
        let path = crate::test_util::test_tmp().join("surface_radii.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let loaded = load_ovika(&path).unwrap().model;
        assert!(model.eq_ignoring_dofmap(&loaded));
        std::fs::remove_file(path).unwrap();
    }

    fn make_3node_model() -> Model {
        Model {
            nodes: vec![
                Node {
                    id: NodeId(0),
                    coord: [0.0, 0.0, 0.0],
                    restraint: Dof6Mask::FREE,
                    mass: None,
                    story: None,
                    support_spring: None,
                },
                Node {
                    id: NodeId(1),
                    coord: [1000.0, 0.0, 0.0],
                    restraint: Dof6Mask::FIXED,
                    mass: None,
                    story: None,
                    support_spring: None,
                },
                Node {
                    id: NodeId(2),
                    coord: [2000.0, 0.0, 0.0],
                    restraint: Dof6Mask::PINNED,
                    mass: None,
                    story: None,
                    support_spring: None,
                },
            ],
            ..Default::default()
        }
    }

    /// 特徴あるフィールドを一通り持つモデル。`save_ovika` / `load_ovika` を共有する
    /// 個別の往復テストを 1 本へ統合するための fixture。
    ///
    /// - 断面 shape（`SectionShape::SteelH`）
    /// - 一般ブレース要素（`ElementKind::Brace { tension_only: true }`）
    /// - 部材付帯情報（両端ハンチ・片端ハンチ・現場／工場継手）
    /// - スラブ厚・二次部材の次安定 ID・壁版・未割当二次部材
    fn make_rich_model() -> Model {
        let mut model = make_3node_model();
        model.slab_thickness = 150.0;
        model.next_secondary_member_id = 7;
        model.wall_plates.push(sepika_core::model::WallPlate {
            dl_support: None,
            self_weight_shares: vec![0.75, 0.25, 0.0],
            id: sepika_core::ids::WallPlateId(0),
            shape: sepika_core::model::WallPlateShape::Enclosed,
            section: None,
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
        });
        model
            .unassigned_posts
            .push(sepika_core::model::SecondaryMember {
                id: sepika_core::ids::SecondaryMemberId(0),
                gravity_end_shares: Some([0.75, 0.25]),
                kind: sepika_core::model::SecondaryMemberKind::Post,
                ends: sepika_core::model::SecondaryMemberEnds::Detached([
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                ]),
                section: None,
                name: "P1".into(),
            });

        let shape = SectionShape::SteelH {
            root_r: Some(13.0),
            height: 400.0,
            width: 200.0,
            web_thick: 9.0,
            flange_thick: 12.0,
        };
        model
            .sections
            .push(shape.to_section(SectionId(0), "H-400x200x9x12".to_string()));

        model.elements.push(ElementData {
            id: ElemId(0),
            kind: ElementKind::Brace { tension_only: true },
            nodes: smallvec::smallvec![NodeId(0), NodeId(2)],
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Pinned, EndCondition::Pinned],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            plastic_zone: None,
            spring: None,
        });

        model.member_detail_attrs = vec![
            MemberDetailAttr {
                elem: ElemId(0),
                haunch_i: Some(Haunch {
                    length: 700.0,
                    depth_increase: 200.0,
                    width_increase: 50.0,
                }),
                haunch_j: Some(Haunch {
                    length: 500.0,
                    depth_increase: 150.0,
                    width_increase: 0.0,
                }),
                joints: vec![
                    MemberJoint {
                        distance: 1000.0,
                        kind: JointKind::Site,
                    },
                    MemberJoint {
                        distance: 3000.0,
                        kind: JointKind::Shop,
                    },
                ],
            },
            MemberDetailAttr {
                elem: ElemId(1),
                haunch_i: Some(Haunch {
                    length: 400.0,
                    depth_increase: 100.0,
                    width_increase: 0.0,
                }),
                haunch_j: None,
                joints: Vec::new(),
            },
        ];
        model
    }

    /// テスト用: manifest.json を読み出して復元する。
    fn read_manifest(path: &Path) -> Manifest {
        let f = std::fs::File::open(path).unwrap();
        let mut ar = zip::ZipArchive::new(f).unwrap();
        let mut mb = Vec::new();
        ar.by_name("manifest.json")
            .unwrap()
            .read_to_end(&mut mb)
            .unwrap();
        serde_json::from_slice(&mb).unwrap()
    }

    #[test]
    fn wall_weight_generation_mode_and_dl_choice_roundtrip_and_old_default() {
        use sepika_core::model::{
            WallDlSupport, WallPlate, WallPlateShape, WallWeightGenerationMode,
        };
        let mut model = make_3node_model();
        model.wall_weight_generation = Some(WallWeightGenerationMode::GravityCasesOnly);
        model.wall_plates.push(WallPlate {
            id: sepika_core::ids::WallPlateId(0),
            shape: WallPlateShape::Attached {
                anchor: sepika_core::model::RegionAnchor::Point(NodeId(0)),
                extent: Some([1.0, 1.0]),
            },
            section: None,
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
            self_weight_shares: vec![],
            dl_support: Some(WallDlSupport::UpperBeam),
        });
        let path = crate::test_util::test_tmp().join("wall_weight_mode.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let loaded = load_ovika(&path).unwrap().model;
        assert_eq!(loaded.wall_weight_generation, model.wall_weight_generation);
        assert_eq!(
            loaded.wall_plates[0].dl_support,
            model.wall_plates[0].dl_support
        );
        let mut old = serde_json::to_value(&model).unwrap();
        old.as_object_mut()
            .unwrap()
            .remove("wall_weight_generation");
        let decoded: Model = serde_json::from_value(old).unwrap();
        assert_eq!(decoded.wall_weight_generation, None);
    }

    #[test]
    fn saved_model_fields_are_named_and_order_independent() {
        #[derive(serde::Deserialize)]
        struct ReorderedModel {
            slab_thickness: f64,
            #[serde(default)]
            added_field: bool,
            nodes: Vec<ReorderedNode>,
        }
        #[derive(serde::Deserialize)]
        struct ReorderedNode {
            coord: [f64; 3],
            id: NodeId,
        }
        let mut model = make_3node_model();
        model.slab_thickness = 123.0;
        let path = crate::test_util::test_tmp().join("named_model.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut bytes = Vec::new();
        archive
            .by_name("model.msgpack")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let fields: ReorderedModel = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(fields.slab_thickness, model.slab_thickness);
        assert!(!fields.added_field);
        assert_eq!(fields.nodes[1].id, model.nodes[1].id);
        assert_eq!(fields.nodes[1].coord, model.nodes[1].coord);
        let loaded = load_ovika(&path).unwrap().model;
        loaded.validate().unwrap();
        assert!(model.eq_ignoring_dofmap(&loaded));
    }

    #[test]
    fn named_node_reads_added_default_field() {
        #[derive(serde::Serialize)]
        struct NodeWithoutSupportSpring {
            story: Option<StoryId>,
            mass: Option<[f64; 6]>,
            restraint: Dof6Mask,
            coord: [f64; 3],
            id: NodeId,
        }
        let bytes = rmp_serde::to_vec_named(&NodeWithoutSupportSpring {
            story: None,
            mass: None,
            restraint: Dof6Mask::FIXED,
            coord: [100.0, 200.0, 300.0],
            id: NodeId(7),
        })
        .unwrap();
        let node: Node = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(node.id, NodeId(7));
        assert_eq!(node.coord, [100.0, 200.0, 300.0]);
        assert_eq!(node.restraint, Dof6Mask::FIXED);
        assert!(node.support_spring.is_none());
    }

    #[test]
    fn corrupt_model_with_matching_hash_returns_decode_error() {
        let path = crate::test_util::test_tmp().join("corrupt_model.ovika");
        save_ovika(&path, &make_3node_model(), OvikaExtras::default()).unwrap();
        let mut manifest = read_manifest(&path);
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut settings = Vec::new();
        archive
            .by_name("settings.json")
            .unwrap()
            .read_to_end(&mut settings)
            .unwrap();
        drop(archive);
        let corrupt = [0xc1];
        manifest
            .entries
            .iter_mut()
            .find(|e| e.name == "model.msgpack")
            .unwrap()
            .sha256 = sha256_of(&corrupt);
        write_zip_with_manifest(&path, &manifest, &corrupt, &settings);
        assert!(matches!(load_ovika(&path), Err(IoError::Decode(_))));
    }

    #[test]
    #[ignore = "実モデルの codec 比較計測。release・単独で実行する"]
    fn measure_real_model_msgpack() {
        use std::hint::black_box;
        use std::time::Instant;
        let xml = crate::stbridge::read_stbridge_file(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../sepika-app/tests/fixtures/model.stb"),
        )
        .unwrap();
        let (model, _) = crate::stbridge::import_stbridge_with_report(&xml).unwrap();
        model.validate().unwrap();
        println!(
            "nodes={} elements={} secondary={} floor_regions={} sections={}",
            model.nodes.len(),
            model.elements.len(),
            model.beams().count(),
            model.floor_regions.len(),
            model.sections.len()
        );
        let dir = crate::test_util::test_tmp();
        let named_path = dir.join("measure_named.ovika");
        let positional_path = dir.join("measure_positional.ovika");
        save_ovika(&named_path, &model, OvikaExtras::default()).unwrap();
        let mut manifest = read_manifest(&named_path);
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&named_path).unwrap()).unwrap();
        let mut settings = Vec::new();
        archive
            .by_name("settings.json")
            .unwrap()
            .read_to_end(&mut settings)
            .unwrap();
        drop(archive);
        for named in [false, true] {
            let encode = || {
                if named {
                    rmp_serde::to_vec_named(black_box(&model))
                } else {
                    rmp_serde::to_vec(black_box(&model))
                }
                .unwrap()
            };
            let bytes = encode();
            let path = if named { &named_path } else { &positional_path };
            if !named {
                manifest
                    .entries
                    .iter_mut()
                    .find(|e| e.name == "model.msgpack")
                    .unwrap()
                    .sha256 = sha256_of(&bytes);
                write_zip_with_manifest(path, &manifest, &bytes, &settings);
            }
            let loaded = load_ovika(path).unwrap().model;
            loaded.validate().unwrap();
            assert!(model.eq_ignoring_dofmap(&loaded));
            for _ in 0..10 {
                black_box(encode());
                black_box(rmp_serde::from_slice::<Model>(&bytes).unwrap());
            }
            let mut enc = Vec::new();
            let mut dec = Vec::new();
            for _ in 0..7 {
                let start = Instant::now();
                for _ in 0..1000 {
                    black_box(encode());
                }
                enc.push(start.elapsed().as_secs_f64() * 1e6 / 1000.0);
                let start = Instant::now();
                for _ in 0..1000 {
                    black_box(rmp_serde::from_slice::<Model>(black_box(&bytes)).unwrap());
                }
                dec.push(start.elapsed().as_secs_f64() * 1e6 / 1000.0);
            }
            enc.sort_by(f64::total_cmp);
            dec.sort_by(f64::total_cmp);
            println!("named={named} msgpack_bytes={} ovika_bytes={} encode_us median={:.3} range={:.3}..{:.3} decode_us median={:.3} range={:.3}..{:.3}",
                bytes.len(), std::fs::metadata(path).unwrap().len(), enc[3], enc[0], enc[6], dec[3], dec[0], dec[6]);
        }
    }

    /// 断面 shape・一般ブレース・部材付帯情報・スラブ厚・二次部材などを含む
    /// rich なモデルが、保存→読込で各フィールドとも完全一致すること。
    #[test]
    fn test_roundtrip_preserves_rich_model() {
        let mut model = make_rich_model();
        model.load_cfg = Some(sepika_core::model::LoadCfg {
            dampers: vec![sepika_core::model::DamperSpec {
                elem: ElemId(0),
                total_weight: 19613.3,
            }],
            ..Default::default()
        });
        model.stories = [0.0, 3000.0]
            .into_iter()
            .enumerate()
            .map(|(i, elevation)| sepika_core::model::Story {
                wall_weights: Vec::new(),
                id: sepika_core::ids::StoryId(i as u32),
                name: format!("{}F", i + 1),
                elevation,
                node_ids: vec![],
                seismic_weight: None,
                weight_override: None,
                structure: Default::default(),
                level_kind: Default::default(),
                dynamic_mass: None,
                standard_floor_load: None,
                column_finish_area_weight: 0.001 * (i + 1) as f64,
                fireproof: StoryFireproof {
                    steel_kind: FireproofKind::Spray,
                    steel_column_area_weight: 0.001 * (i + 1) as f64,
                    steel_beam_area_weight: 0.002,
                    cft_kind: FireproofKind::Board,
                    cft_column_area_weight: 0.003,
                },
            })
            .collect();
        let dir = crate::test_util::test_tmp();
        let path = dir.join("p_rich_roundtrip.ovika");
        model.stories[1].dynamic_mass = Some(sepika_core::model::StoryDynamicMass {
            mass_equiv_weight_n: 19613.3,
            center_xy_mm: [0.0, 0.0],
            inertia_t_mm2: 8000000.0,
            lumped_mass: Some(sepika_core::model::StoryLumpedMass {
                master: NodeId(0),
                mass_method: sepika_core::model::MassMethod::CorrectedLumped,
                mass: Some([2.0, 2.0, 0.0, 0.0, 0.0, 8000000.0]),
                damper_weight_n: 19613.3,
            }),
        });
        model.damper_mass_generation = Some(
            model
                .capture_damper_mass_generation(
                    &[model.nodes[0].clone()],
                    &[sepika_core::model::Constraint::rigid_diaphragm(
                        sepika_core::ids::StoryId(1),
                        NodeId(0),
                        vec![NodeId(1)],
                    )],
                    &vec![Some(sepika_core::ids::StoryId(1)); model.nodes.len()],
                    &model.stories,
                )
                .unwrap(),
        );
        assert_eq!(
            model.damper_mass_generation.as_ref().unwrap().inputs.len(),
            1
        );
        assert_eq!(
            model
                .damper_mass_generation
                .as_ref()
                .unwrap()
                .placements
                .len(),
            1
        );
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let back = load_ovika(&path).unwrap().model;
        let manifest = read_manifest(&path);
        assert_eq!(back.load_cfg, model.load_cfg);
        assert_eq!(back.damper_mass_generation, model.damper_mass_generation);
        assert_eq!(
            back.damper_mass_generation.as_ref().unwrap().dynamic_masses,
            vec![
                (model.stories[0].id, model.stories[0].dynamic_mass),
                (model.stories[1].id, model.stories[1].dynamic_mass)
            ]
        );
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.created_by, "SEPIKA");
        let file = std::fs::File::open(&path).unwrap();
        let archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(
            archive.file_names().collect::<Vec<_>>(),
            ["manifest.json", "model.msgpack", "settings.json"]
        );

        assert_eq!(back.nodes.len(), model.nodes.len());
        assert_eq!(
            back.stories, model.stories,
            "階共通柱仕上げ面重量を保存読込で保持する"
        );
        assert_eq!(
            back.slab_thickness, model.slab_thickness,
            "床スラブ厚は往復で保持される"
        );
        assert_eq!(
            back.next_secondary_member_id, model.next_secondary_member_id,
            "二次部材の次安定 ID は往復で保持される"
        );
        assert_eq!(back.sections.len(), 1);
        assert!(
            matches!(back.sections[0].shape, Some(SectionShape::SteelH { .. })),
            "断面 shape の種別が往復で保持される"
        );
        assert_eq!(back.sections[0].shape, model.sections[0].shape);
        assert_eq!(back.elements.len(), 1);
        assert_eq!(
            back.elements[0].kind,
            ElementKind::Brace { tension_only: true },
            "一般ブレースの構造体バリアントが往復で保持される"
        );
        assert_eq!(
            back.member_detail_attrs, model.member_detail_attrs,
            "部材付帯情報（ハンチ・継手）が往復で保持される"
        );
        assert_eq!(back.wall_plates, model.wall_plates);
        assert_eq!(back.unassigned_posts, model.unassigned_posts);
        assert!(model.eq_ignoring_dofmap(&back));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 指定なし生成記録の保存往復は未生成と区別する() {
        let mut model = make_rich_model();
        assert!(model.damper_mass_generation.is_none());
        model.damper_mass_generation = Some(Default::default());
        let dir = crate::test_util::test_tmp();
        let path = dir.join("empty_damper_generation.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let back = load_ovika(&path).unwrap().model;
        assert_eq!(back.damper_mass_generation, Some(Default::default()));
    }

    #[test]
    fn property_basis_roundtrip_keeps_individual_inputs_and_radius_updates() {
        use sepika_core::ids::SectionId;
        use sepika_core::model::{PropertyBasis, SectionPropertyBasis};
        use sepika_core::section_shape::SectionShape;
        let shape = SectionShape::SteelH {
            height: 400.0,
            width: 200.0,
            web_thick: 9.0,
            flange_thick: 12.0,
            root_r: Some(13.0),
        };
        let mut section = shape.to_section(SectionId(0), "G1".into());
        section.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
        section.area = 123.0;
        section.property_basis.area = PropertyBasis::Supplied;
        let mut supplied = section.clone();
        supplied.id = SectionId(1);
        supplied.name = "入力値".into();
        supplied.property_basis = SectionPropertyBasis::default();
        let mut model = make_rich_model();
        model.sections = vec![section.clone(), supplied.clone()];
        let dir = crate::test_util::test_tmp();
        let path = dir.join("individual_property_basis.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let loaded = load_ovika(&path).unwrap().model;
        assert_eq!(loaded.sections, model.sections);
        let updated = loaded.sections[0].with_surface_radius(Some(20.0)).unwrap();
        assert_eq!(updated.area, 123.0);
        assert_ne!(updated.iy, section.iy);
        assert_ne!(updated.iz, section.iz);
        let updated = loaded.sections[1].with_surface_radius(None).unwrap();
        assert_eq!(updated.area, supplied.area);
        assert_eq!(updated.iy, supplied.iy);
        assert_eq!(updated.iz, supplied.iz);
    }

    #[test]
    fn tip_load_roundtrip() {
        let mut model = make_3node_model();
        model.load_cases = default_load_cases();
        model.elements.push(ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: vec![NodeId(0), NodeId(1)].into(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
        model.slabs.push(Slab {
            id: SlabId(0),
            shape: SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span: [0.2, 0.8],
                    transfer: LoadTransfer::Anchor,
                },
                extent: [1000.0, 2000.0],
            },
            plate: SlabPlate::default(),
            tip_loads: vec![SlabTipLoad {
                case: LoadCaseId(3),
                intensity: 2.0,
                direction: TipLoadDirection::PosY,
            }],
        });
        let path = crate::test_util::test_tmp().join("tip_load_roundtrip.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let loaded = load_ovika(&path).unwrap().model;
        assert_eq!(loaded.slabs[0].tip_loads, model.slabs[0].tip_loads);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_hash_mismatch() {
        let model = make_3node_model();
        let dir = crate::test_util::test_tmp();
        let path = dir.join("p_hash.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let settings_bytes = {
            let f = std::fs::File::open(&path).unwrap();
            let mut ar = zip::ZipArchive::new(f).unwrap();
            let mut sb = Vec::new();
            ar.by_name("settings.json")
                .unwrap()
                .read_to_end(&mut sb)
                .unwrap();
            sb
        };

        // model.msgpack を改竄相当のバイト列に差し替え、manifest のハッシュと食い違わせる。
        // settings.json は正しいハッシュにして、必須エントリチェックではなく
        // ハッシュ検証そのものでエラーになることを確認する。
        let bad_manifest = Manifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            units: "internal: N-mm-s".to_string(),
            created_by: "test".to_string(),
            entries: vec![
                crate::manifest::EntryHash {
                    name: "model.msgpack".to_string(),
                    sha256: "badhash".to_string(),
                },
                crate::manifest::EntryHash {
                    name: "settings.json".to_string(),
                    sha256: sha256_of(&settings_bytes),
                },
            ],
        };
        write_zip_with_manifest(&path, &bad_manifest, &[0u8; 4], &settings_bytes);

        let result = load_ovika(&path);
        assert!(matches!(result, Err(IoError::HashMismatch(ref name)) if name == "model.msgpack"));
        let _ = std::fs::remove_file(&path);
    }

    /// 未リリースのため後方互換なし: 現行版以外（旧版 2 や未来版 999 を名乗る
    /// ファイル）は version 検証で `UnsupportedVersion` として拒否されること。
    #[test]
    fn test_rejects_unsupported_versions() {
        let dir = crate::test_util::test_tmp();
        for version in [2u32, 999] {
            let path = dir.join(format!("p_unsupported_ver_{version}.ovika"));
            let manifest = Manifest {
                schema_version: version,
                units: "internal: N-mm-s".to_string(),
                created_by: "test".to_string(),
                entries: vec![],
            };
            write_zip_with_manifest(&path, &manifest, &[], &[]);

            let result = load_ovika(&path);
            assert!(
                matches!(result, Err(IoError::UnsupportedVersion(v)) if v == version),
                "schema_version {version} は拒否されるべき"
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    /// manifest.entries から model.msgpack を落としたファイルが、ハッシュ未検証のまま
    /// 読めてしまわないこと（MissingEntry で拒否される）。
    #[test]
    fn test_manifest_missing_required_entry_rejected() {
        let model = make_3node_model();
        let dir = crate::test_util::test_tmp();
        let path = dir.join("p_missing_entry.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let (model_bytes, settings_bytes) = {
            let f = std::fs::File::open(&path).unwrap();
            let mut ar = zip::ZipArchive::new(f).unwrap();
            let mut mb = Vec::new();
            ar.by_name("model.msgpack")
                .unwrap()
                .read_to_end(&mut mb)
                .unwrap();
            let mut sb = Vec::new();
            ar.by_name("settings.json")
                .unwrap()
                .read_to_end(&mut sb)
                .unwrap();
            (mb, sb)
        };

        // model.msgpack のエントリだけを manifest から落とす（zip 内には実体を残す）。
        let manifest = Manifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            units: "internal: N-mm-s".to_string(),
            created_by: "test".to_string(),
            entries: vec![crate::manifest::EntryHash {
                name: "settings.json".to_string(),
                sha256: sha256_of(&settings_bytes),
            }],
        };
        write_zip_with_manifest(&path, &manifest, &model_bytes, &settings_bytes);

        let result = load_ovika(&path);
        assert!(matches!(result, Err(IoError::MissingEntry(ref name)) if name == "model.msgpack"));
        let _ = std::fs::remove_file(&path);
    }

    /// テスト用: 指定 manifest と実バイトで .ovika を書き直す。
    fn write_zip_with_manifest(
        path: &Path,
        manifest: &Manifest,
        model_bytes: &[u8],
        settings_bytes: &[u8],
    ) {
        let manifest_bytes = serde_json::to_vec_pretty(manifest).unwrap();
        let tmp_path = path.with_extension("ovika.tmp");
        {
            let f = std::fs::File::create(&tmp_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::FileOptions::<()>::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("manifest.json", opts).unwrap();
            zip.write_all(&manifest_bytes).unwrap();
            zip.start_file("model.msgpack", opts).unwrap();
            zip.write_all(model_bytes).unwrap();
            zip.start_file("settings.json", opts).unwrap();
            zip.write_all(settings_bytes).unwrap();
            zip.finish().unwrap();
        }
        std::fs::rename(&tmp_path, path).unwrap();
    }

    /// 準備計算の結果と解析タブの設定値（いずれもアプリ層が直列化した任意バイト列）が
    /// 保存→読込で往復し、それぞれ manifest のハッシュ検証対象になること。解析タブの
    /// 設定値は `results` を生成した条件（波形パラメータ・減衰モデル等）を保持しないと
    /// 再現性が保てないため同梱する。
    #[test]
    fn test_roundtrip_preserves_optional_entries() {
        let model = make_3node_model();
        let dir = crate::test_util::test_tmp();
        let path = dir.join("p_optional_roundtrip.ovika");
        let prep = b"preparation payload".to_vec();
        let results = b"results payload".to_vec();
        let cfg = b"analysis settings payload".to_vec();
        save_ovika(
            &path,
            &model,
            OvikaExtras {
                preparation: Some(&prep),
                results: Some(&results),
                analysis_settings: Some(&cfg),
            },
        )
        .unwrap();

        let loaded = load_ovika(&path).unwrap();
        assert!(model.eq_ignoring_dofmap(&loaded.model));
        assert_eq!(loaded.preparation.as_deref(), Some(prep.as_slice()));
        assert_eq!(loaded.results.as_deref(), Some(results.as_slice()));
        assert_eq!(loaded.analysis_settings.as_deref(), Some(cfg.as_slice()));

        // manifest に列挙され、ハッシュ検証の対象になっている。
        let manifest = read_manifest(&path);
        assert_eq!(manifest.schema_version, 1);
        for (name, data) in [
            (PREPARATION_ENTRY, &prep),
            (RESULTS_ENTRY, &results),
            (ANALYSIS_SETTINGS_ENTRY, &cfg),
        ] {
            let entry = manifest
                .entries
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} エントリが manifest にあるはず"));
            assert_eq!(entry.sha256, sha256_of(data));
        }

        let _ = std::fs::remove_file(&path);
    }

    /// 任意エントリ（準備計算の結果・解析タブの設定値）を同梱しないで保存した
    /// ファイルは、いずれも `None` として読める（旧プロジェクトファイル相当）。
    #[test]
    fn test_load_without_optional_entries() {
        let model = make_3node_model();
        let dir = crate::test_util::test_tmp();
        let path = dir.join("p_optional_absent.ovika");
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();

        let loaded = load_ovika(&path).unwrap();
        assert!(model.eq_ignoring_dofmap(&loaded.model));
        assert!(loaded.preparation.is_none());
        assert!(loaded.analysis_settings.is_none());
        let _ = std::fs::remove_file(&path);
    }
}
