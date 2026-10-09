use crate::app::App;
use sepika_core::ids::{FloorPlateAssignmentRegionId, FloorRegionId, NodeId, SlabId};
use sepika_core::model::{AreaLoad, DistributionMethod, LoadPurpose, OneWayDir, SlabUsage};
use sepika_core::model::{LoadTransfer, RegionAnchor, SlabShape};
use sepika_core::units::to_display::area_load_kn_per_m2;
use sepika_core::units::to_internal;
use sepika_edit::{
    AssignSlabToFloorPlateRegion, DeleteSlab, SetAttachedAnchor, SetAttachedExtent,
    SetFloorPlateRegionNoPlate, SetFloorRegionName, SetSlabOneWay, SetSlabUsage,
    UnsetFloorPlateRegion,
};

/// スラブ追加フォームのドラフト状態（GUI 専用）。
/// `nodes` は境界4節点（頂点0→1→2→3→0 の順で外周を辿る）の選択状態。
#[derive(Clone, Debug)]
pub struct SlabDraft {
    /// 境界節点スロット（外周順。3〜N 個、可変長）。
    pub nodes: Vec<Option<NodeId>>,
    /// 荷重種別（既定 "DL"）
    pub load_kind: String,
    /// 荷重値の入力文字列。**UI 表示は kN/m²**（内部は `to_internal::area_load_kn_per_m2`）。
    pub load_value: String,
    pub method: DistributionMethod,
    /// 一方向スラブの伝達方向。
    pub one_way: Option<OneWayDir>,
    /// スラブ用途（積載荷重プリセット。`None` は積載寄与なし）。
    pub usage: Option<SlabUsage>,
    /// 任意入力の積載荷重 [kN/m²]（床用・小梁用・大梁用・地震用の順）。
    pub custom_live_kn_m2: [String; 4],
    /// スラブ断面（板厚・コンクリート材料を持つ断面。`None` は未割当）。
    pub section: Option<sepika_core::ids::SectionId>,
    /// 取り付き領域の取付き先の節点（線なら両端、点なら 1 つ目だけを使う）。
    pub attached_nodes: [Option<NodeId>; 2],
    /// 取り付き先を点（柱）にするか。false は線（取付き線）。
    pub attached_point: bool,
    /// 張り出し量の入力文字列 [mm]（線: 始端側・終端側、点: X 方向・Y 方向）。
    pub attached_extent: [String; 2],
    /// 取付き線に載る領域の荷重の出口。
    pub attached_transfer: sepika_core::model::LoadTransfer,
    /// 取付き線上の無次元区間 `[t_i, t_j]`（0.0〜1.0）。全長は `[0.0, 1.0]`。点取付きでは使わない。
    pub attached_span: [f64; 2],
}

impl Default for SlabDraft {
    fn default() -> Self {
        Self {
            nodes: vec![None; 4],
            load_kind: "DL".to_string(),
            load_value: "0".to_string(),
            method: DistributionMethod::TriTrapezoid,
            one_way: None,
            usage: None,
            custom_live_kn_m2: std::array::from_fn(|_| "0".to_string()),
            section: None,
            attached_nodes: [None; 2],
            attached_point: false,
            attached_extent: ["1000".to_string(), "1000".to_string()],
            attached_transfer: sepika_core::model::LoadTransfer::Anchor,
            attached_span: [0.0, 1.0],
        }
    }
}

/// 用途選択で提示するプリセット（令別表第1／国交省営繕基準・令和3年度版）。
/// `None` は「なし（積載寄与なし）」。任意入力（`Custom`）は値を保持するため
/// ここには入れず、コンボ内の特別な項目から初期化する。並びは国交省営繕基準の表に概ね沿う。
const USAGE_PRESETS: &[Option<SlabUsage>] = &[
    None,
    Some(SlabUsage::Residential),
    Some(SlabUsage::Office),
    Some(SlabUsage::ResearchRoom),
    Some(SlabUsage::Classroom),
    Some(SlabUsage::Store),
    Some(SlabUsage::AssemblyFixed),
    Some(SlabUsage::AssemblyOther),
    Some(SlabUsage::Corridor),
    Some(SlabUsage::RegistryArchive),
    Some(SlabUsage::GeneralArchive),
    Some(SlabUsage::MobileArchive),
    Some(SlabUsage::LabChemistry),
    Some(SlabUsage::LabPhysics),
    Some(SlabUsage::ComputerRoom),
    Some(SlabUsage::MachineRoom),
    Some(SlabUsage::Gymnasium),
    Some(SlabUsage::Garage),
    Some(SlabUsage::Balcony),
    Some(SlabUsage::RoofResidential),
    Some(SlabUsage::RoofStore),
    Some(SlabUsage::RoofUnused),
    Some(SlabUsage::RoofSteelGym),
];

fn usage_label(u: Option<SlabUsage>) -> &'static str {
    match u {
        None => "なし",
        Some(SlabUsage::Residential) => "住宅の居室・寝室・病室",
        Some(SlabUsage::Office) => "事務室・会議室・食堂",
        Some(SlabUsage::ResearchRoom) => "研究室",
        Some(SlabUsage::Classroom) => "教室",
        Some(SlabUsage::Store) => "百貨店・店舗の売場",
        Some(SlabUsage::AssemblyFixed) => "集会室・客席（固定席）",
        Some(SlabUsage::AssemblyOther) => "集会室・客席（その他）",
        Some(SlabUsage::Corridor) => "廊下・玄関・階段",
        Some(SlabUsage::RegistryArchive) => "法務局登記書庫",
        Some(SlabUsage::GeneralArchive) => "一般書庫・倉庫等",
        Some(SlabUsage::MobileArchive) => "移動書架書庫・電算室空調機室・用具庫等",
        Some(SlabUsage::LabChemistry) => "一般実験室（化学系）",
        Some(SlabUsage::LabPhysics) => "一般実験室（物理系）",
        Some(SlabUsage::ComputerRoom) => "電算室",
        Some(SlabUsage::MachineRoom) => "機械室",
        Some(SlabUsage::Gymnasium) => "体育館・武道場等",
        Some(SlabUsage::Garage) => "自動車車庫・通路",
        Some(SlabUsage::Balcony) => "片持バルコニー・庇等",
        Some(SlabUsage::RoofResidential) => "屋上（学校・百貨店の類を除く）",
        Some(SlabUsage::RoofStore) => "屋上（学校・百貨店の類）",
        Some(SlabUsage::RoofUnused) => "屋上（通常人が使用しない）",
        Some(SlabUsage::RoofSteelGym) => "屋上（鉄骨造体育館・武道場等／短期）",
        Some(SlabUsage::Custom { .. }) => "任意入力",
    }
}

/// 用途の実効 4 値 [N/mm²]（床用・小梁用・大梁用・地震用）。`None` はすべて 0。
fn usage_custom_values(u: Option<SlabUsage>) -> [f64; 4] {
    match u {
        Some(SlabUsage::Custom {
            floor,
            beam,
            frame,
            seismic,
        }) => [floor, beam, frame, seismic],
        Some(u) => [
            u.live_load(LoadPurpose::Floor),
            u.live_load(LoadPurpose::Beam),
            u.live_load(LoadPurpose::Frame),
            u.live_load(LoadPurpose::Seismic),
        ],
        None => [0.0; 4],
    }
}

/// 4 値 [N/mm²] を任意入力の積載荷重へまとめる。
fn custom_usage(values: [f64; 4]) -> SlabUsage {
    SlabUsage::Custom {
        floor: values[0],
        beam: values[1],
        frame: values[2],
        seismic: values[3],
    }
}

/// 用途の 4 値を kN/m² で並べた 1 行。
fn usage_values_text(u: SlabUsage) -> String {
    let [floor, beam, frame, seismic] = usage_custom_values(Some(u));
    format!(
        "床 {:.2} / 小梁 {:.2} / 大梁 {:.2} / 地震 {:.2} kN/m²",
        area_load_kn_per_m2(floor),
        area_load_kn_per_m2(beam),
        area_load_kn_per_m2(frame),
        area_load_kn_per_m2(seismic),
    )
}

fn method_label(m: DistributionMethod) -> &'static str {
    match m {
        DistributionMethod::TriTrapezoid => "三角/台形(45°法)",
        DistributionMethod::OneWay => "一方向",
        DistributionMethod::TributaryArea => "負担面積",
    }
}

fn kind_label(slab: &sepika_core::model::Slab) -> &'static str {
    if slab.is_attached() {
        "取り付き"
    } else {
        "囲まれ"
    }
}

fn one_way_label(o: Option<OneWayDir>) -> &'static str {
    match o {
        None => "なし",
        Some(OneWayDir::X) => "X",
        Some(OneWayDir::Y) => "Y",
        Some(OneWayDir::Short) => "短辺",
    }
}

pub fn slabs_table(ui: &mut egui::Ui, app: &mut App) {
    if let Some(reason) = app.core.scoped.undo.last_error() {
        ui.colored_label(crate::theme::ERROR_RED, reason);
    }
    use crate::table_util::{self, Col};

    ui.label(
        "床領域は大梁が囲む1区画です（名前・所属する小梁・所属する床板を持ちます）。床板（スラブ）は、\
         大梁または小梁で囲まれた版、または主架構に取り付く版（片持ち・バルコニー・出隅）です。\
         版の仕様（断面・荷重・用途・分配法）は床板が持ちます。断面が未割当の床板には床荷重・\
         スラブ検定・協力幅は生じません（結果タブ/モデルタブの3Dビューで表示モード「CMQ図」を\
         選ぶと分配結果を確認できます）。",
    );
    ui.separator();

    ui.strong("床領域（大梁の区画）");
    let mut pending_region_name: Vec<(FloorRegionId, String)> = Vec::new();
    table_util::standard_table(
        ui,
        "floor_regions_tbl",
        &[
            Col::id(),
            Col::name("名前"),
            Col::text("境界節点"),
            Col::label("床板"),
            Col::label("小梁"),
        ],
        app.core.model.floor_regions.len(),
        |row| {
            let i = row.index();
            let region = &app.core.model.floor_regions[i];
            row.col(|ui| {
                table_util::id_label(ui, region.id.0);
            });
            row.col(|ui| {
                let mut name = region.name.clone();
                let resp = table_util::cell_text_edit(ui, &mut name);
                if resp.changed() {
                    pending_region_name.push((region.id, name));
                }
            });
            row.col(|ui| {
                let s = region
                    .boundary
                    .iter()
                    .map(|n| n.0.to_string())
                    .collect::<Vec<_>>()
                    .join("-");
                table_util::text_cell(ui, &s);
            });
            row.col(|ui| {
                let cnt = region.slab_ids.len();
                if cnt == 0 {
                    table_util::muted_cell(ui, "―", "床板が割り当たっていません");
                } else {
                    ui.label(format!("{cnt}枚"));
                }
            });
            row.col(|ui| {
                let cnt = region.secondary_beams.len();
                if cnt == 0 {
                    table_util::muted_cell(ui, "―", "小梁が配置されていません");
                } else {
                    ui.label(format!("{cnt}本"));
                }
            });
        },
    );
    for (id, name) in pending_region_name {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(SetFloorRegionName { id, name }),
        );
        app.core.scoped.staleness.mark_edited();
    }

    ui.add_space(8.0);
    ui.strong("床板（スラブ）");

    let n = app.core.model.slabs.len();
    let mut pending_delete: Option<SlabId> = None;
    let mut pending_one_way: Vec<(SlabId, Option<OneWayDir>)> = Vec::new();
    let mut pending_usage: Vec<(SlabId, Option<SlabUsage>)> = Vec::new();
    let mut pending_section: Vec<(SlabId, Option<sepika_core::ids::SectionId>)> = Vec::new();
    let mut pending_extent: Vec<(SlabId, [f64; 2])> = Vec::new();
    let mut pending_anchor: Vec<(SlabId, RegionAnchor)> = Vec::new();
    let node_ids: Vec<NodeId> = app.core.model.nodes.iter().map(|n| n.id).collect();
    let slab_sections: Vec<(sepika_core::ids::SectionId, String)> = app
        .core
        .model
        .sections
        .iter()
        .filter(|sec| sec.thickness.is_some_and(|t| t > 0.0))
        .map(|sec| (sec.id, sec.display_name()))
        .collect();

    table_util::standard_table(
        ui,
        "slabs_tbl",
        &[
            Col::id(),
            Col::text("所属床領域"),
            Col::text("境界節点"),
            Col::text("荷重"),
            Col::name("分配法"),
            Col::name("種別"),
            Col::name("一方向"),
            Col::text("用途"),
            Col::text("断面"),
            Col::actions(),
        ],
        n,
        |row| {
            let i = row.index();
            let slab = &app.core.model.slabs[i];
            row.col(|ui| {
                table_util::id_label(ui, slab.id.0);
            });
            row.col(|ui| {
                let owner = app
                    .core
                    .model
                    .floor_regions
                    .iter()
                    .find(|r| r.slab_ids.contains(&slab.id));
                match owner {
                    Some(r) if !r.name.is_empty() => table_util::text_cell(ui, &r.name),
                    Some(r) => table_util::text_cell(ui, &format!("#{}", r.id.0)),
                    None => table_util::muted_cell(ui, "―", "どの床領域からも参照されていません"),
                }
            });
            row.col(|ui| match &slab.shape {
                SlabShape::Enclosed => match app.core.model.slab_assignment_region(slab.id) {
                    Some(region) => {
                        table_util::text_cell(
                            ui,
                            &format!("R{}（{}辺）", region.id.0, region.boundary.len()),
                        );
                    }
                    None => table_util::muted_cell(ui, "―", "割当領域に属していません"),
                },
                SlabShape::Attached { anchor, extent } => {
                    attached_boundary_cell(
                        ui,
                        slab.id,
                        *anchor,
                        *extent,
                        &node_ids,
                        &mut pending_extent,
                        &mut pending_anchor,
                    );
                }
            });
            row.col(|ui| {
                let s = slab
                    .plate
                    .loads
                    .iter()
                    .map(|l| format!("{} {:.2}kN/m²", l.kind, area_load_kn_per_m2(l.value)))
                    .collect::<Vec<_>>()
                    .join(", ");
                if s.is_empty() {
                    table_util::muted_cell(ui, "―", "床荷重が登録されていません");
                } else {
                    table_util::text_cell(ui, &s);
                }
            });
            row.col(|ui| {
                table_util::text_cell(ui, method_label(slab.method()));
            });
            row.col(|ui| {
                table_util::text_cell(ui, kind_label(slab));
            });
            row.col(|ui| {
                table_util::cell_combo(
                    ui,
                    ("slab_one_way", slab.id.0),
                    one_way_label(slab.one_way()),
                    |ui| {
                        for ow in [
                            None,
                            Some(OneWayDir::X),
                            Some(OneWayDir::Y),
                            Some(OneWayDir::Short),
                        ] {
                            if ui
                                .selectable_label(slab.one_way() == ow, one_way_label(ow))
                                .clicked()
                                && slab.one_way() != ow
                            {
                                pending_one_way.push((slab.id, ow));
                            }
                        }
                    },
                );
            });
            row.col(|ui| {
                ui.vertical(|ui| {
                    table_util::cell_combo(
                        ui,
                        ("slab_usage", slab.id.0),
                        usage_label(slab.usage()),
                        |ui| {
                            for &u in USAGE_PRESETS {
                                if ui
                                    .selectable_label(slab.usage() == u, usage_label(u))
                                    .clicked()
                                    && slab.usage() != u
                                {
                                    pending_usage.push((slab.id, u));
                                }
                            }
                            ui.separator();
                            let is_custom = matches!(slab.usage(), Some(SlabUsage::Custom { .. }));
                            if ui.selectable_label(is_custom, "任意入力").clicked() && !is_custom
                            {
                                let v = usage_custom_values(slab.usage());
                                pending_usage.push((slab.id, Some(custom_usage(v))));
                            }
                        },
                    );
                    match slab.usage() {
                        Some(SlabUsage::Custom {
                            floor,
                            beam,
                            frame,
                            seismic,
                        }) => {
                            let mut values = [floor, beam, frame, seismic];
                            let mut changed = false;
                            ui.horizontal_wrapped(|ui| {
                                for (label, value) in
                                    ["床", "小梁", "大梁", "地震"].iter().zip(values.iter_mut())
                                {
                                    ui.label(*label);
                                    let mut kn = area_load_kn_per_m2(*value);
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut kn)
                                                .speed(0.01)
                                                .suffix(" kN/m²"),
                                        )
                                        .changed()
                                    {
                                        *value = to_internal::area_load_kn_per_m2(kn);
                                        changed = true;
                                    }
                                }
                            });
                            if changed {
                                pending_usage.push((slab.id, Some(custom_usage(values))));
                            }
                        }
                        Some(u) => {
                            table_util::text_cell(ui, &usage_values_text(u));
                        }
                        None => {}
                    }
                });
            });
            row.col(|ui| {
                let label = app
                    .core
                    .model
                    .slab_section(slab)
                    .map(|sec| sec.display_name())
                    .unwrap_or_else(|| "―".to_string());
                table_util::cell_combo(ui, ("slab_section", slab.id.0), &label, |ui| {
                    if ui.selectable_label(slab.section().is_none(), "―").clicked()
                        && slab.section().is_some()
                    {
                        pending_section.push((slab.id, None));
                    }
                    for (sid, name) in &slab_sections {
                        if ui
                            .selectable_label(slab.section() == Some(*sid), name)
                            .clicked()
                            && slab.section() != Some(*sid)
                        {
                            pending_section.push((slab.id, Some(*sid)));
                        }
                    }
                });
            });
            row.col(|ui| {
                if table_util::delete_cell(ui, "この床板を削除", None) {
                    pending_delete = Some(slab.id);
                }
            });
        },
    );

    let revision_before = app.core.scoped.undo.revision();
    for (id, one_way) in pending_one_way {
        app.core
            .scoped
            .undo
            .run(&mut app.core.model, Box::new(SetSlabOneWay { id, one_way }));
    }
    for (id, usage) in pending_usage {
        app.core
            .scoped
            .undo
            .run(&mut app.core.model, Box::new(SetSlabUsage { id, usage }));
    }
    for (id, section) in pending_section {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetSlabSection { id, section }),
        );
    }
    apply_attached_boundary_changes(app, pending_extent, pending_anchor);
    if let Some(id) = pending_delete {
        app.core
            .scoped
            .undo
            .run(&mut app.core.model, Box::new(DeleteSlab { id }));
    }
    if app.core.scoped.undo.revision() != revision_before {
        app.core.scoped.staleness.mark_edited();
    }

    tip_load_section(ui, app);

    ui.add_space(8.0);
    ui.strong("小梁（二次部材）");
    ui.label(
        "小梁は解析要素ではなく、床板から受けた荷重を大梁へ伝えます。端部支持条件「自由」は\
         片持ち小梁（基端支持・先端自由）を表し、荷重は基端の鉛直反力として伝達します。",
    );
    crate::tables::secondary::secondary_member_placement_form(
        app,
        ui,
        sepika_core::model::SecondaryMemberKind::Beam,
    );
    ui.add_space(4.0);
    crate::tables::secondary::secondary_member_list(
        app,
        ui,
        sepika_core::model::SecondaryMemberKind::Beam,
    );

    ui.separator();
    ui.strong("床板の割当");
    ui.label(
        "囲まれた床板は、大梁・小梁で分割された床板割当領域へ1枚ずつ割り当てます。\
         任意の節点境界からは作成できません。",
    );

    let region_ids: Vec<FloorPlateAssignmentRegionId> = app
        .core
        .model
        .floor_assignment_regions
        .regions
        .iter()
        .map(|r| r.id)
        .collect();
    if region_ids.is_empty() {
        ui.label(
            "床板割当領域がありません。解析前処理（割当領域の再構築）を実行すると、\
             大梁・小梁で分割された領域が作られます。",
        );
    }
    table_util::standard_table(
        ui,
        "floor_assignment_regions_tbl",
        &[
            Col::id(),
            Col::text("所属床領域"),
            Col::text("状態"),
            Col::text("床板"),
            Col::text("境界支持部材"),
        ],
        region_ids.len(),
        |row| {
            let id = region_ids[row.index()];
            let region = app
                .core
                .model
                .floor_assignment_region(id)
                .expect("region_ids は同じモデルから取得");
            row.col(|ui| {
                table_util::id_label(ui, id.0);
            });
            row.col(|ui| {
                let owner = region.assignment.plate().and_then(|sid| {
                    app.core
                        .model
                        .floor_regions
                        .iter()
                        .find(|r| r.slab_ids.contains(&sid))
                });
                match owner {
                    Some(r) if !r.name.is_empty() => table_util::text_cell(ui, &r.name),
                    Some(r) => table_util::text_cell(ui, &format!("#{}", r.id.0)),
                    None => table_util::muted_cell(ui, "―", "未所属"),
                }
            });
            row.col(|ui| {
                let text = match region.assignment {
                    sepika_core::model::PlateAssignment::Unset => "未設定",
                    sepika_core::model::PlateAssignment::NoPlate => "版なし",
                    sepika_core::model::PlateAssignment::Plate(_) => "版あり",
                };
                table_util::text_cell(ui, text);
            });
            row.col(|ui| match region.assignment.plate() {
                Some(sid) => table_util::text_cell(ui, &format!("#{}", sid.0)),
                None => table_util::muted_cell(ui, "―", "床板が割り当てられていません"),
            });
            row.col(|ui| {
                table_util::text_cell(ui, &format!("{}辺", region.boundary.len()));
            });
        },
    );

    ui.label(
        "下の「割り当てる床板の仕様」を設定し、未設定・版なしの領域へ割り当てます。\
         版ありの領域は上の床板一覧で編集してください。",
    );

    ui.horizontal(|ui| {
        ui.label("荷重種別:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.slab_draft.load_kind).desired_width(60.0),
        );
        ui.label("荷重 [kN/m²]:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.slab_draft.load_value)
                .desired_width(80.0),
        );
    });

    ui.horizontal(|ui| {
        ui.horizontal(|ui| {
            ui.label("断面:");
            let resolved = app
                .ui
                .scoped
                .slab_draft
                .section
                .and_then(|sid| app.core.model.sections.get(sid.index()))
                .filter(|sec| sec.thickness.is_some_and(|t| t > 0.0));
            if resolved.is_none() {
                app.ui.scoped.slab_draft.section = None;
            }
            let label = resolved
                .map(|sec| sec.display_name())
                .unwrap_or_else(|| "―".to_string());
            egui::ComboBox::from_id_salt("slab_draft_section")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.ui.scoped.slab_draft.section, None, "―");
                    for sec in &app.core.model.sections {
                        if sec.thickness.is_some_and(|t| t > 0.0) {
                            ui.selectable_value(
                                &mut app.ui.scoped.slab_draft.section,
                                Some(sec.id),
                                sec.display_name(),
                            );
                        }
                    }
                });
        })
        .response
        .on_hover_text(
            "床の板厚と自重は断面から決まります。断面が未割当の床は解析前チェックで止まります",
        );
        ui.label("用途（積載荷重）:")
            .on_hover_text("令別表第1 の積載荷重（大梁用）を「LL(架構用)」ケースへ分配します");
        egui::ComboBox::from_id_salt("slab_draft_usage")
            .selected_text(usage_label(app.ui.scoped.slab_draft.usage))
            .show_ui(ui, |ui| {
                for &u in USAGE_PRESETS {
                    ui.selectable_value(&mut app.ui.scoped.slab_draft.usage, u, usage_label(u));
                }
                ui.separator();
                let is_custom = matches!(
                    app.ui.scoped.slab_draft.usage,
                    Some(SlabUsage::Custom { .. })
                );
                if ui.selectable_label(is_custom, "任意入力").clicked() && !is_custom {
                    let v = usage_custom_values(app.ui.scoped.slab_draft.usage);
                    for (slot, value) in
                        app.ui.scoped.slab_draft.custom_live_kn_m2.iter_mut().zip(v)
                    {
                        *slot = format!("{:.2}", area_load_kn_per_m2(value));
                    }
                    app.ui.scoped.slab_draft.usage = Some(custom_usage(v));
                }
            });
        if let Some(SlabUsage::Custom { .. }) = app.ui.scoped.slab_draft.usage {
            ui.horizontal(|ui| {
                for (label, slot) in ["床用", "小梁用", "大梁用", "地震用"]
                    .iter()
                    .zip(app.ui.scoped.slab_draft.custom_live_kn_m2.iter_mut())
                {
                    ui.label(*label);
                    ui.add(egui::TextEdit::singleline(slot).desired_width(55.0));
                }
            });
            let v: [f64; 4] = std::array::from_fn(|i| {
                to_internal::area_load_kn_per_m2(
                    app.ui.scoped.slab_draft.custom_live_kn_m2[i]
                        .trim()
                        .parse::<f64>()
                        .unwrap_or(0.0),
                )
            });
            app.ui.scoped.slab_draft.usage = Some(custom_usage(v));
        } else if let Some(u) = app.ui.scoped.slab_draft.usage {
            ui.label(usage_values_text(u));
        }
    });

    ui.horizontal(|ui| {
        ui.label("分配法:");
        ui.selectable_value(
            &mut app.ui.scoped.slab_draft.method,
            DistributionMethod::TriTrapezoid,
            "三角/台形(45°法)",
        );
        ui.selectable_value(
            &mut app.ui.scoped.slab_draft.method,
            DistributionMethod::OneWay,
            "一方向",
        );
        ui.selectable_value(
            &mut app.ui.scoped.slab_draft.method,
            DistributionMethod::TributaryArea,
            "負担面積",
        );
        ui.label("一方向:");
        egui::ComboBox::from_id_salt("slab_draft_one_way")
            .selected_text(one_way_label(app.ui.scoped.slab_draft.one_way))
            .show_ui(ui, |ui| {
                for one_way in [
                    None,
                    Some(OneWayDir::X),
                    Some(OneWayDir::Y),
                    Some(OneWayDir::Short),
                ] {
                    ui.selectable_value(
                        &mut app.ui.scoped.slab_draft.one_way,
                        one_way,
                        one_way_label(one_way),
                    );
                }
            });
    });

    let value_kn_m2 = app
        .ui
        .scoped
        .slab_draft
        .load_value
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0);
    let value = to_internal::area_load_kn_per_m2(value_kn_m2);
    let kind = app.ui.scoped.slab_draft.load_kind.trim();
    let kind = if kind.is_empty() { "DL" } else { kind }.to_string();
    let plate = sepika_core::model::SlabPlate {
        section: app.ui.scoped.slab_draft.section,
        loads: vec![AreaLoad { kind, value }],
        usage: app.ui.scoped.slab_draft.usage,
        method: app.ui.scoped.slab_draft.method,
        one_way: app.ui.scoped.slab_draft.one_way,
    };

    let mut pending_assign: Vec<FloorPlateAssignmentRegionId> = Vec::new();
    let mut pending_no_plate: Vec<FloorPlateAssignmentRegionId> = Vec::new();
    let mut pending_unset: Vec<FloorPlateAssignmentRegionId> = Vec::new();
    ui.horizontal_wrapped(|ui| {
        for &region_id in &region_ids {
            let Some(state) = app
                .core
                .model
                .floor_assignment_region(region_id)
                .map(|r| r.assignment)
            else {
                continue;
            };
            ui.group(|ui| {
                ui.label(format!("領域 R{}", region_id.0));
                if let sepika_core::model::PlateAssignment::Plate(sid) = state {
                    ui.label(format!("床板 #{}", sid.0));
                } else if ui.button("この仕様で割当").clicked() {
                    pending_assign.push(region_id);
                }
                if !state.is_no_plate() && ui.button("版なし").clicked() {
                    pending_no_plate.push(region_id);
                }
                if !state.is_unset() && ui.button("未設定へ戻す").clicked() {
                    pending_unset.push(region_id);
                }
            });
        }
    });
    let pending =
        !pending_assign.is_empty() || !pending_no_plate.is_empty() || !pending_unset.is_empty();
    for &region in &pending_assign {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(AssignSlabToFloorPlateRegion {
                region,
                plate: plate.clone(),
            }),
        );
    }
    for &region in &pending_no_plate {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(SetFloorPlateRegionNoPlate { region }),
        );
    }
    for &region in &pending_unset {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(UnsetFloorPlateRegion { region }),
        );
    }
    if pending {
        app.core.scoped.staleness.mark_edited();
    }

    attached_section(ui, app);
}

fn tip_direction_label(direction: sepika_core::model::TipLoadDirection) -> &'static str {
    use sepika_core::model::TipLoadDirection::*;
    match direction {
        PosX => "+X",
        NegX => "-X",
        PosY => "+Y",
        NegY => "-Y",
        PosZ => "+Z",
        NegZ => "-Z",
    }
}

fn tip_load_section(ui: &mut egui::Ui, app: &mut App) {
    use sepika_core::model::{SlabTipLoad, TipLoadDirection};

    let mut pending = Vec::new();
    for slab in app
        .core
        .model
        .slabs
        .iter()
        .filter(|slab| slab.supports_tip_loads())
    {
        ui.group(|ui| {
            ui.label(format!(
                "床板 #{} の先端荷重（先端辺の実長あたり kN/m）",
                slab.id.0
            ));
            for (index, load) in slab.tip_loads.iter().enumerate() {
                let mut edited = load.clone();
                let mut delete = false;
                ui.horizontal(|ui| {
                    let name = app
                        .core
                        .model
                        .load_cases
                        .get(edited.case.index())
                        .map(|c| c.name.as_str())
                        .unwrap_or("未定義");
                    egui::ComboBox::from_id_salt(("tip_case", slab.id.0, index))
                        .selected_text(name)
                        .show_ui(ui, |ui| {
                            for case in &app.core.model.load_cases {
                                ui.selectable_value(&mut edited.case, case.id, &case.name);
                            }
                        });
                    egui::ComboBox::from_id_salt(("tip_direction", slab.id.0, index))
                        .selected_text(tip_direction_label(edited.direction))
                        .show_ui(ui, |ui| {
                            for direction in [
                                TipLoadDirection::PosX,
                                TipLoadDirection::NegX,
                                TipLoadDirection::PosY,
                                TipLoadDirection::NegY,
                                TipLoadDirection::PosZ,
                                TipLoadDirection::NegZ,
                            ] {
                                ui.selectable_value(
                                    &mut edited.direction,
                                    direction,
                                    tip_direction_label(direction),
                                );
                            }
                        });
                    let mut kn_per_m = edited.intensity;
                    if ui
                        .add(
                            egui::DragValue::new(&mut kn_per_m)
                                .speed(0.1)
                                .range(0.0..=f64::MAX)
                                .suffix(" kN/m"),
                        )
                        .changed()
                    {
                        edited.intensity = kn_per_m;
                    }
                    delete = ui.button("削除").clicked();
                });
                if delete || edited != *load {
                    let mut loads = slab.tip_loads.clone();
                    if delete {
                        loads.remove(index);
                    } else {
                        loads[index] = edited;
                    }
                    pending.push((slab.id, loads));
                }
            }
            if let Some(case) = app.core.model.load_cases.first() {
                if ui.button("先端荷重を追加").clicked() {
                    let mut loads = slab.tip_loads.clone();
                    loads.push(SlabTipLoad {
                        case: case.id,
                        intensity: 0.0,
                        direction: TipLoadDirection::NegZ,
                    });
                    pending.push((slab.id, loads));
                }
            }
        });
    }
    for (id, loads) in pending {
        if app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetSlabTipLoads { id, loads }),
        ) {
            app.core.scoped.staleness.mark_edited();
        }
    }
}

/// 取り付き領域（片持ちスラブ・バルコニー・出隅）の入力セクション。
///
/// 主架構に囲まれていない床板は、囲まれた床板と違って境界を節点で描けない。
/// 取付き先（大梁の 2 節点、または柱の 1 節点）と張り出し量で作る。取り付く床板は
/// どの床領域からも参照されない独立した床板であり、名前は持たない。
/// 張り出し量の符号は、線なら取付き線 1→2 の**左側が正**、点なら全体座標の X/Y の向き。
fn attached_section(ui: &mut egui::Ui, app: &mut App) {
    ui.separator();
    ui.strong("取り付く床板を追加（片持ち・バルコニー・出隅）");
    ui.label(
        "主架構に囲まれない床板です。取付き先（大梁の2節点、または柱の1節点）と張り出し量で作ります。張り出し量の符号は、線なら取付き線 1→2 の左が正、点なら全体座標 X/Y の向きです。",
    );

    ui.horizontal(|ui| {
        ui.label("取付き先:");
        ui.selectable_value(
            &mut app.ui.scoped.slab_draft.attached_point,
            false,
            "線（大梁）",
        );
        ui.selectable_value(
            &mut app.ui.scoped.slab_draft.attached_point,
            true,
            "点（柱）",
        );
    });

    let node_ids: Vec<NodeId> = app.core.model.nodes.iter().map(|n| n.id).collect();
    let n_slots = if app.ui.scoped.slab_draft.attached_point {
        1
    } else {
        2
    };
    ui.horizontal(|ui| {
        for k in 0..n_slots {
            ui.label(format!("節点{}:", k + 1));
            let label = app.ui.scoped.slab_draft.attached_nodes[k]
                .map(|n| n.0.to_string())
                .unwrap_or_else(|| "―".to_string());
            egui::ComboBox::from_id_salt(("attached_node", k))
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.ui.scoped.slab_draft.attached_nodes[k], None, "―");
                    for id in &node_ids {
                        ui.selectable_value(
                            &mut app.ui.scoped.slab_draft.attached_nodes[k],
                            Some(*id),
                            id.0.to_string(),
                        );
                    }
                });
        }
    });

    ui.horizontal(|ui| {
        let labels = if app.ui.scoped.slab_draft.attached_point {
            ["X 方向 [mm]:", "Y 方向 [mm]:"]
        } else {
            ["始端側 [mm]:", "終端側 [mm]:"]
        };
        for (label, value) in labels
            .iter()
            .zip(app.ui.scoped.slab_draft.attached_extent.iter_mut())
        {
            ui.label(*label);
            ui.add(egui::TextEdit::singleline(value).desired_width(70.0));
        }
    });

    if !app.ui.scoped.slab_draft.attached_point {
        ui.horizontal(|ui| {
            ui.label("荷重の出口:");
            ui.selectable_value(
                &mut app.ui.scoped.slab_draft.attached_transfer,
                LoadTransfer::Anchor,
                "取付き線へ分布",
            );
            ui.selectable_value(
                &mut app.ui.scoped.slab_draft.attached_transfer,
                LoadTransfer::Columns,
                "両端の柱へ集中",
            );
        });
        ui.horizontal(|ui| {
            ui.label("取付き線の区間 [0, 1]（既定は全長）:");
            ui.add(
                egui::DragValue::new(&mut app.ui.scoped.slab_draft.attached_span[0])
                    .range(0.0..=1.0)
                    .speed(0.01),
            );
            ui.label("〜");
            ui.add(
                egui::DragValue::new(&mut app.ui.scoped.slab_draft.attached_span[1])
                    .range(0.0..=1.0)
                    .speed(0.01),
            );
        });
    }

    let extent: Option<[f64; 2]> = {
        let a = app.ui.scoped.slab_draft.attached_extent[0]
            .trim()
            .parse::<f64>()
            .ok();
        let b = app.ui.scoped.slab_draft.attached_extent[1]
            .trim()
            .parse::<f64>()
            .ok();
        a.zip(b).map(|(a, b)| [a, b])
    };
    let span = app.ui.scoped.slab_draft.attached_span;
    let candidate = attached_creation_candidate(
        &app.core.model,
        app.ui.scoped.slab_draft.attached_point,
        app.ui.scoped.slab_draft.attached_nodes,
        span,
        app.ui.scoped.slab_draft.attached_transfer,
        extent,
    );
    let error = candidate.as_ref().and_then(|slab| {
        app.core
            .model
            .validate_attached_slab(slab)
            .err()
            .map(|error| error.to_string())
    });
    if candidate.is_none() {
        ui.label("取付き先の節点と張り出し量を指定してください");
    } else if let Some(error) = &error {
        ui.colored_label(crate::theme::ERROR_RED, error);
    }
    let ready = candidate.is_some() && error.is_none();
    if ui
        .add_enabled(ready, egui::Button::new("取り付く床板を追加"))
        .clicked()
    {
        if let Some(candidate) = candidate {
            add_attached_candidate(app, candidate);
        }
    }
}

fn attached_creation_candidate(
    model: &sepika_core::model::Model,
    point: bool,
    nodes: [Option<NodeId>; 2],
    span: [f64; 2],
    transfer: LoadTransfer,
    extent: Option<[f64; 2]>,
) -> Option<sepika_core::model::Slab> {
    let anchor = if point {
        RegionAnchor::Point(nodes[0]?)
    } else {
        RegionAnchor::Line {
            nodes: [nodes[0]?, nodes[1]?],
            span,
            transfer,
        }
    };
    Some(sepika_core::model::Slab {
        id: SlabId(model.slabs.len() as u32),
        shape: SlabShape::Attached {
            anchor,
            extent: extent?,
        },
        plate: sepika_core::model::SlabPlate::default(),
        tip_loads: vec![],
    })
}

fn add_attached_candidate(app: &mut App, candidate: sepika_core::model::Slab) -> bool {
    let SlabShape::Attached { anchor, extent } = candidate.shape else {
        return false;
    };
    app.apply_model_edit(Box::new(sepika_edit::AddAttachedSlab {
        anchor,
        extent,
        plate: candidate.plate,
    }))
}

fn attached_boundary_cell(
    ui: &mut egui::Ui,
    id: SlabId,
    anchor: RegionAnchor,
    extent: [f64; 2],
    node_ids: &[NodeId],
    pending_extent: &mut Vec<(SlabId, [f64; 2])>,
    pending_anchor: &mut Vec<(SlabId, RegionAnchor)>,
) {
    ui.vertical(|ui| {
        match anchor {
            RegionAnchor::Line {
                nodes,
                span,
                transfer,
            } => {
                ui.horizontal(|ui| {
                    for k in 0..2 {
                        let mut sel = nodes[k];
                        egui::ComboBox::from_id_salt(("att_anc", id.0, k))
                            .selected_text(format!("N{}", sel.0))
                            .show_ui(ui, |ui| {
                                for &nid in node_ids {
                                    ui.selectable_value(&mut sel, nid, format!("N{}", nid.0));
                                }
                            });
                        let mut n = nodes;
                        n[k] = sel;
                        queue_attached_anchor(
                            id,
                            anchor,
                            RegionAnchor::Line {
                                nodes: n,
                                span,
                                transfer,
                            },
                            pending_anchor,
                        );
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("区間:");
                    let mut s = span;
                    ui.add(egui::DragValue::new(&mut s[0]).range(0.0..=1.0).speed(0.01));
                    ui.label("〜");
                    ui.add(egui::DragValue::new(&mut s[1]).range(0.0..=1.0).speed(0.01));
                    queue_attached_anchor(
                        id,
                        anchor,
                        RegionAnchor::Line {
                            nodes,
                            span: s,
                            transfer,
                        },
                        pending_anchor,
                    );
                });
            }
            RegionAnchor::Point(n) => {
                let mut sel = n;
                egui::ComboBox::from_id_salt(("att_pt", id.0))
                    .selected_text(format!("N{}", sel.0))
                    .show_ui(ui, |ui| {
                        for &nid in node_ids {
                            ui.selectable_value(&mut sel, nid, format!("N{}", nid.0));
                        }
                    });
                queue_attached_anchor(id, anchor, RegionAnchor::Point(sel), pending_anchor);
            }
            RegionAnchor::FloorRegion { .. } => {}
        }
        ui.horizontal(|ui| {
            let mut e = extent;
            ui.add(egui::DragValue::new(&mut e[0]).suffix(" mm"));
            ui.add(egui::DragValue::new(&mut e[1]).suffix(" mm"));
            queue_attached_extent(id, extent, e, pending_extent);
        });
    });
}

fn queue_attached_anchor(
    id: SlabId,
    previous: RegionAnchor,
    candidate: RegionAnchor,
    pending: &mut Vec<(SlabId, RegionAnchor)>,
) {
    if candidate != previous {
        pending.push((id, candidate));
    }
}

fn apply_attached_boundary_changes(
    app: &mut App,
    extents: Vec<(SlabId, [f64; 2])>,
    anchors: Vec<(SlabId, RegionAnchor)>,
) {
    for (id, extent) in extents {
        app.apply_model_edit(Box::new(SetAttachedExtent { id, extent }));
    }
    for (id, anchor) in anchors {
        app.apply_model_edit(Box::new(SetAttachedAnchor { id, anchor }));
    }
}

fn queue_attached_extent(
    id: SlabId,
    previous: [f64; 2],
    candidate: [f64; 2],
    pending: &mut Vec<(SlabId, [f64; 2])>,
) {
    if candidate != previous {
        pending.push((id, candidate));
    }
}

#[cfg(test)]
mod tip_load_tests {
    use super::*;
    use sepika_core::model::{LoadTransfer, Slab, SlabPlate, TipLoadDirection};

    #[test]
    fn attached_creation_distinguishes_missing_input_from_invalid_complete_candidate() {
        let model = sepika_core::model::Model::default();
        for nodes in [[None, None], [Some(NodeId(0)), None]] {
            assert!(attached_creation_candidate(
                &model,
                false,
                nodes,
                [0.0, 1.0],
                LoadTransfer::Anchor,
                Some([1000.0; 2])
            )
            .is_none());
        }
        assert!(attached_creation_candidate(
            &model,
            false,
            [Some(NodeId(0)); 2],
            [0.0, 1.0],
            LoadTransfer::Anchor,
            None
        )
        .is_none());
        assert!(attached_creation_candidate(
            &model,
            false,
            [Some(NodeId(0)); 2],
            [0.75, 0.25],
            LoadTransfer::Anchor,
            Some([f64::NAN; 2])
        )
        .is_some());
    }

    #[test]
    fn attached_creation_reports_common_diagnostic_and_preserves_state_on_rejection() {
        let mut base = App::default();
        base.core.model = crate::sample::portal_frame();
        base.run_preparation();
        assert!(base.core.scoped.preparation.is_some());
        for (point, nodes, span, extent, missing_support, reason) in [
            (
                false,
                [2, 2],
                [0.25, 0.75],
                [1000.0; 2],
                false,
                "XY 長さがゼロ",
            ),
            (
                false,
                [2, 3],
                [0.8, 0.75],
                [1000.0; 2],
                false,
                "span が不正",
            ),
            (
                false,
                [2, 3],
                [0.75, 0.75],
                [1000.0; 2],
                false,
                "span が不正",
            ),
            (
                false,
                [2, 3],
                [0.25, 0.75],
                [f64::NAN, 1000.0],
                false,
                "extent が非有限",
            ),
            (
                false,
                [2, 3],
                [0.25, 0.75],
                [f64::INFINITY, 1000.0],
                false,
                "extent が非有限",
            ),
            (
                true,
                [99, 3],
                [0.25, 0.75],
                [1000.0; 2],
                false,
                "節点 99 が存在しません",
            ),
            (
                false,
                [2, 3],
                [0.25, 0.75],
                [1000.0; 2],
                true,
                "荷重支持先が欠落",
            ),
        ] {
            let mut app = App::default();
            app.core.model = base.core.model.clone();
            if missing_support {
                app.core.model.elements.clear();
            }
            app.core.scoped.preparation = base.core.scoped.preparation.clone();
            app.core.scoped.results = Some(crate::app::ResultsBundle::default());
            assert!(app.apply_model_edit(Box::new(sepika_edit::AddNode {
                coord: [9000.0, 0.0, 0.0],
                restraint: sepika_core::dof::Dof6Mask::FREE
            })));
            app.core.scoped.undo.undo(&mut app.core.model);
            app.select_node(NodeId(2));
            app.core.scoped.staleness.results_stale = false;
            app.core.scoped.staleness.design_stale = false;
            app.core.scoped.staleness.preparation_stale = false;
            app.core.scoped.staleness.diagnostics_stale = false;
            app.core.scoped.staleness.unsaved_changes = false;
            let state = format!(
                "{:?}{:?}{:?}{:?}{:?}",
                app.core.model,
                app.ui.scoped.selection,
                app.core.scoped.preparation,
                app.core.scoped.results,
                app.core.scoped.staleness
            );
            let revision = app.core.scoped.undo.revision();
            let redo = app.core.scoped.undo.redo_label().map(str::to_owned);
            let candidate = attached_creation_candidate(
                &app.core.model,
                point,
                nodes.map(|id| Some(NodeId(id))),
                span,
                LoadTransfer::Anchor,
                Some(extent),
            )
            .unwrap();
            let diagnostic = app
                .core
                .model
                .validate_attached_slab(&candidate)
                .unwrap_err()
                .to_string();
            assert!(diagnostic.contains("Slab 0:"));
            assert!(diagnostic.contains(reason), "{diagnostic}");
            app.ui.scoped.slab_draft.attached_point = point;
            app.ui.scoped.slab_draft.attached_nodes = nodes.map(|id| Some(NodeId(id)));
            app.ui.scoped.slab_draft.attached_span = span;
            app.ui.scoped.slab_draft.attached_extent = extent.map(|value| value.to_string());
            let ctx = egui::Context::default();
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                attached_section(ui, &mut app)
            });
            assert!(
                output.shapes.iter().any(|shape| matches!(
                    &shape.shape,
                    egui::epaint::Shape::Text(text) if text.galley.job.text == diagnostic
                )),
                "{diagnostic}"
            );
            assert!(!add_attached_candidate(&mut app, candidate));
            assert_eq!(app.core.scoped.undo.last_error(), Some(diagnostic.as_str()));
            assert_eq!(app.core.scoped.undo.revision(), revision);
            assert_eq!(app.core.scoped.undo.redo_label(), redo.as_deref());
            assert!(!app.core.scoped.undo.can_undo());
            assert!(app.core.scoped.undo.can_redo());
            assert_eq!(
                format!(
                    "{:?}{:?}{:?}{:?}{:?}",
                    app.core.model,
                    app.ui.scoped.selection,
                    app.core.scoped.preparation,
                    app.core.scoped.results,
                    app.core.scoped.staleness
                ),
                state
            );
        }
    }

    #[test]
    fn valid_line_and_point_creation_candidates_are_added() {
        for point in [false, true] {
            let mut app = App::default();
            app.core.model = crate::sample::portal_frame();
            let candidate = attached_creation_candidate(
                &app.core.model,
                point,
                [Some(NodeId(2)), Some(NodeId(3))],
                [0.25, 0.75],
                LoadTransfer::Anchor,
                Some([-1000.0, -2000.0]),
            )
            .unwrap();
            assert_eq!(app.core.model.validate_attached_slab(&candidate), Ok(()));
            let expected = candidate.shape.clone();
            assert!(add_attached_candidate(&mut app, candidate));
            assert_eq!(app.core.model.slabs.len(), 1);
            assert_eq!(app.core.model.slabs[0].shape, expected);
            assert!(app.core.scoped.undo.can_undo());
            assert!(app.core.scoped.staleness.unsaved_changes);
        }
    }

    #[test]
    fn existing_attached_boundary_candidates_report_common_rejection_and_preserve_state() {
        let anchor = RegionAnchor::Line {
            nodes: [NodeId(2), NodeId(3)],
            span: [0.25, 0.75],
            transfer: LoadTransfer::Anchor,
        };
        let mut base = App::default();
        base.core.model = crate::sample::portal_frame();
        base.run_preparation();
        assert!(base.core.scoped.preparation.is_some());
        assert!(
            base.apply_model_edit(Box::new(sepika_edit::AddAttachedSlab {
                anchor,
                extent: [1000.0, 2000.0],
                plate: SlabPlate::default(),
            }))
        );
        let id = base.core.model.slabs[0].id;
        let candidate_anchors = [
            RegionAnchor::Line {
                nodes: [NodeId(3); 2],
                span: [0.25, 0.75],
                transfer: LoadTransfer::Anchor,
            },
            RegionAnchor::Line {
                nodes: [NodeId(2), NodeId(3)],
                span: [0.8, 0.75],
                transfer: LoadTransfer::Anchor,
            },
            RegionAnchor::Line {
                nodes: [NodeId(2), NodeId(3)],
                span: [0.75, 0.75],
                transfer: LoadTransfer::Anchor,
            },
        ];
        for (candidate_anchor, candidate_extent) in candidate_anchors
            .into_iter()
            .map(|value| (value, [1000.0, 2000.0]))
            .chain(
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY]
                    .into_iter()
                    .map(|value| (anchor, [value, 2000.0])),
            )
        {
            let mut app = App::default();
            app.core.model = base.core.model.clone();
            app.core.scoped.preparation = base.core.scoped.preparation.clone();
            app.core.scoped.results = Some(crate::app::ResultsBundle::default());
            app.select_node(NodeId(2));
            app.core.scoped.staleness.results_stale = false;
            app.core.scoped.staleness.design_stale = false;
            app.core.scoped.staleness.preparation_stale = false;
            app.core.scoped.staleness.diagnostics_stale = false;
            app.core.scoped.staleness.unsaved_changes = false;
            let model = format!("{:?}", app.core.model);
            let selection = format!("{:?}", app.ui.scoped.selection);
            let preparation = format!("{:?}", app.core.scoped.preparation);
            let results = format!("{:?}", app.core.scoped.results);
            let revision = app.core.scoped.undo.revision();
            let mut candidate = app.core.model.slabs[0].clone();
            candidate.shape = SlabShape::Attached {
                anchor: candidate_anchor,
                extent: candidate_extent,
            };
            let expected = app
                .core
                .model
                .validate_attached_slab(&candidate)
                .unwrap_err()
                .to_string();
            let mut extents = vec![];
            let mut anchors = vec![];
            queue_attached_anchor(id, anchor, candidate_anchor, &mut anchors);
            queue_attached_extent(id, [1000.0, 2000.0], candidate_extent, &mut extents);
            assert_eq!(extents.len() + anchors.len(), 1);
            apply_attached_boundary_changes(&mut app, extents, anchors);
            assert_eq!(app.core.scoped.undo.last_error(), Some(expected.as_str()));
            assert_eq!(format!("{:?}", app.core.model), model);
            assert_eq!(format!("{:?}", app.ui.scoped.selection), selection);
            assert_eq!(format!("{:?}", app.core.scoped.preparation), preparation);
            assert_eq!(format!("{:?}", app.core.scoped.results), results);
            assert_eq!(app.core.scoped.undo.revision(), revision);
            assert!(!app.core.scoped.staleness.results_stale);
            assert!(!app.core.scoped.staleness.design_stale);
            assert!(!app.core.scoped.staleness.preparation_stale);
            assert!(!app.core.scoped.staleness.diagnostics_stale);
            assert!(!app.core.scoped.staleness.unsaved_changes);
        }
    }

    #[test]
    fn gui_shows_tip_controls_only_for_line_anchor_transfer() {
        let mut app = App::default();
        app.core.model.load_cases = sepika_core::model::default_load_cases();
        let shape = SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: [1000.0; 2],
        };
        let slab = Slab {
            id: SlabId(0),
            shape: shape.clone(),
            plate: SlabPlate::default(),
            tip_loads: Vec::new(),
        };
        let visible = |app: &App| {
            app.core
                .model
                .slabs
                .iter()
                .filter(|slab| slab.supports_tip_loads())
                .count()
        };
        app.core.model.slabs.push(slab);
        assert_eq!(visible(&app), 1);
        let context = egui::Context::default();
        let _ = context.run_ui(egui::RawInput::default(), |ui| {
            tip_load_section(ui, &mut app);
        });
        app.core.model.slabs[0].shape = SlabShape::Attached {
            anchor: RegionAnchor::Point(NodeId(0)),
            extent: [1000.0; 2],
        };
        assert_eq!(visible(&app), 0);
        app.core.model.slabs[0].shape = SlabShape::Enclosed;
        assert_eq!(visible(&app), 0);
        app.core.model.slabs[0].shape = shape;
        if let SlabShape::Attached { anchor, .. } = &mut app.core.model.slabs[0].shape {
            *anchor = RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Columns,
            };
        }
        assert_eq!(visible(&app), 0);
        assert_eq!(tip_direction_label(TipLoadDirection::NegY), "-Y");
    }
}
