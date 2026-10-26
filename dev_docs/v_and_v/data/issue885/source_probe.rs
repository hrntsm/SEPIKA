use sepika_design_jp::secondary::{
    ds_group::{ds_rc, ds_steel, GroupType},
    holding_capacity::MemberRank,
    src_rank::{src_column_rank, src_wall_type},
};

#[test]
fn source885_src_column_table_and_boundaries() {
    use MemberRank::*;
    let cases = [
        (0.2, 0.5, FA, FB), (0.2, 0.3, FB, FC),
        (0.35, 0.5, FB, FC), (0.35, 0.3, FC, FD), (0.5, 0.5, FD, FD),
        (0.299999, 0.4, FA, FB), (0.3, 0.4, FA, FB), (0.300001, 0.4, FB, FC),
        (0.399999, 0.4, FB, FC), (0.4, 0.4, FB, FC), (0.400001, 0.4, FD, FD),
        (0.3, 0.399999, FB, FC), (0.3, 0.400001, FA, FB),
        (0.4, 0.399999, FC, FD), (0.4, 0.400001, FB, FC),
    ];
    for (axial_ratio, steel_ratio, bending, shear) in cases {
        assert_eq!(src_column_rank(axial_ratio, steel_ratio, false), bending);
        assert_eq!(src_column_rank(axial_ratio, steel_ratio, true), shear);
        println!("N/N0={axial_ratio}, sM0/M0={steel_ratio}: bending={bending:?}, shear={shear:?}");
    }
    assert_eq!(src_wall_type(false), FA);
    assert_eq!(src_wall_type(true), FC);
    println!("wall: other=WA, shear=WC; physical failure identification is outside this probe");
}

#[test]
fn source885_other_structure_tables_cannot_substitute_src_wall_frame() {
    use GroupType::A;
    for (beta, src_minimum) in [(0.3, 0.25), (0.300001, 0.3), (0.7, 0.3), (0.700001, 0.35)] {
        let steel = ds_steel(A, beta, A);
        let rc = ds_rc(A, beta, A);
        assert_eq!(steel, 0.25);
        assert!((rc - (src_minimum + 0.05_f64)).abs() < 1e-14);
        if beta > 0.3 { assert!(steel < src_minimum); }
        println!("beta={beta}, SRC original minimum={src_minimum}, steel={steel}, RC={rc}");
    }
}
