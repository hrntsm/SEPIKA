use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
use sepika_design_jp::{CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind, SrcDesign};
fn material(name: &str, category: MaterialCategory) -> Material {
 Material { id:MaterialId(0),name:name.into(),category,young:20500.0,poisson:0.2,density:2.4e-9,shear:None,fc:Some(24.0),fy:Some(345.0),strength_factor:None,concrete_class:Default::default() }
}
fn ctx() -> DesignCtx {
 DesignCtx {term:LoadTerm::Long,kind:MemberKind::Girder,rebar_material:Some(material("SD345",MaterialCategory::Rebar)),shear_rebar_material:Some(material("SD295A",MaterialCategory::Rebar)),steel_material:Some(material("SN400B",MaterialCategory::Steel)),..Default::default()}
}
fn section(b:f64) -> sepika_core::model::Section {
 SectionShape::SrcBeamRect {b,d:600.0,rebar:RcBeamRebar{main_dia:16.0,top:vec![2],bottom:vec![2],cover:20.0,stirrup:BeamStirrup{dia:8.0,pitch:100.0,legs:2}},steel_height:400.0,steel_width:200.0,steel_web_thick:8.0,steel_flange_thick:12.0}.to_section(SectionId(0),"独立SRC断面".into())
}
fn forces() -> MemberForcesAt {MemberForcesAt{pos:0.0,n:0.0,qy:0.0,qz:0.0,my:0.0,mz:0.0}}
#[test]
fn source884_beam_fixed_allowable_and_boundary() {
 // 鋭角Hの独立矩形積分Z=1080743.2533333334 mm³、2-D16面積=402.1238596594935 mm²、j=493.5 mm。
 let ma=211_982_789.841_743_65;
 for factor in [0.999999,1.0,1.000001] {
  let f=MemberForcesAt{mz:ma*factor,..forces()};
  let result=SrcDesign.check(&f,&section(300.0),&material("Fc24",MaterialCategory::Concrete),&ctx()).unwrap_checked();
  assert!((result.ratio()-factor).abs()<1e-12);
  println!("moment,factor={factor},ratio={:.15}",result.ratio());
 }
}
#[test]
fn source884_cover_and_unchecked_components_are_not_qualification() {
 for b in [299.8,300.0,300.2] {
  let result=SrcDesign.check(&forces(),&section(b),&material("Fc24",MaterialCategory::Concrete),&ctx());
  assert!(matches!(result,CheckOutcome::Checked(_)));
  println!("steel_cover_mm={},checked=true",(b-200.0)/2.0);
 }
 let ignored=MemberForcesAt{n:100_000_000.0,my:100_000_000_000.0,qz:100_000_000.0,..forces()};
 let result=SrcDesign.check(&ignored,&section(300.0),&material("Fc24",MaterialCategory::Concrete),&ctx()).unwrap_checked();
 assert_eq!(result.ratio(),0.0);
 println!("beam_N_My_Qz_only,ratio={}",result.ratio());
 let mut unknown=ctx();unknown.steel_material.as_mut().unwrap().name="未定義鋼材".into();
 assert!(matches!(SrcDesign.check(&forces(),&section(300.0),&material("Fc24",MaterialCategory::Concrete),&unknown),CheckOutcome::Skipped{..}));
}
