use sepika_core::section_shape::SectionShape;
use sepika_section::mn_surface::{plastic_fibers, FiberRegion, StrengthParams, YieldModelKind};

#[test]
fn built_box_fiber_regions_preserve_the_exact_steel_and_filled_areas() {
    for (t2, area, core_area) in [(6.0, 7584.0, 52416.0), (12.0, 9768.0, 50232.0)] {
        for cft in [false, true] {
            let shape = if cft {
                SectionShape::CftBox {
                    height: 300.0,
                    width: 200.0,
                    thick: 9.0,
                    horizontal_thick: t2,
                    corner_r: Some(0.0),
                }
            } else {
                SectionShape::SteelBox {
                    height: 300.0,
                    width: 200.0,
                    thick: 9.0,
                    horizontal_thick: t2,
                    corner_r: Some(0.0),
                }
            };
            let fibers = plastic_fibers(
                &shape,
                &StrengthParams::default(),
                YieldModelKind::MultiFiber,
            )
            .unwrap();
            let steel: f64 = fibers
                .iter()
                .filter(|f| f.region == FiberRegion::Steel)
                .map(|f| f.area)
                .sum();
            let core: f64 = fibers
                .iter()
                .filter(|f| f.region == FiberRegion::Concrete)
                .map(|f| f.area)
                .sum();
            assert!((steel - area).abs() < 1e-7);
            assert!((core - if cft { core_area } else { 0.0 }).abs() < 1e-7);
        }
    }
}
