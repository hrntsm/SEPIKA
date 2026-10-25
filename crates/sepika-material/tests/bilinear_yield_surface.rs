use approx::assert_relative_eq;
use sepika_material::uniaxial::{Bilinear, UniaxialMaterial};

#[test]
fn committed_yield_surface_preserves_stress_and_directional_slopes() {
    for peak in [0.001, 0.0001] {
        for direction in [-1.0, 1.0] {
            let mut material = Bilinear::new(1e12, 1e7, 0.01);
            let strain = direction * peak;
            let expected = direction * (9_900_000.0 + 1e10 * peak);
            let loading = material.trial(strain);
            assert_relative_eq!(loading.0, expected, epsilon = 1e-6);
            assert_relative_eq!(loading.1, 1e10, epsilon = 1e-5);
            material.commit();
            let checkpoint = material.serialize_state();
            let same = material.probe(strain);
            println!("strain={strain:.17} loading={loading:?} committed_probe={same:?}");
            assert_relative_eq!(same.0, expected, epsilon = 1e-6);
            assert_eq!(checkpoint, material.serialize_state());
            for (increment, stress_increment, tangent) in [
                (direction * 1e-8, direction * 100.0, 1e10),
                (-direction * 1e-8, -direction * 10_000.0, 1e12),
            ] {
                let probe = material.probe(strain + increment);
                assert_relative_eq!(probe.0, expected + stress_increment, epsilon = 1e-6);
                assert_relative_eq!(probe.1, tangent, epsilon = 1e-5);
                assert_eq!(checkpoint, material.serialize_state());
                assert_eq!(probe, material.trial(strain + increment));
                material.revert();
                assert_eq!(checkpoint, material.serialize_state());
            }
            for _ in 0..32 {
                assert_relative_eq!(material.trial(strain).0, expected, epsilon = 1e-6);
                material.commit();
            }
            let unload = material.trial(strain - direction * 1e-6);
            assert_relative_eq!(unload.0, expected - direction * 1e6, epsilon = 1e-6);
            assert_relative_eq!(unload.1, 1e12, epsilon = 1e-5);
            material.commit();
            let reload = material.trial(strain - direction * 0.5e-6);
            assert_relative_eq!(reload.0, expected - direction * 0.5e6, epsilon = 1e-6);
            assert_relative_eq!(reload.1, 1e12, epsilon = 1e-5);
            material.commit();
            let renewed = material.trial(strain + direction * 1e-6);
            assert_relative_eq!(renewed.0, expected + direction * 10_000.0, epsilon = 1e-6);
            assert_relative_eq!(renewed.1, 1e10, epsilon = 1e-5);
            material.revert();
            let reversed = material.trial(strain - direction * 21e-6);
            assert_relative_eq!(
                reversed.0,
                expected - direction * 20_010_000.0,
                epsilon = 1e-6
            );
            assert_relative_eq!(reversed.1, 1e10, epsilon = 1e-5);
        }
    }
}

#[test]
fn same_strain_reevaluates_updated_material_parameters() {
    for direct in [false, true] {
        let mut material = Bilinear::new(1e12, 1e7, 0.01);
        material.trial(0.001);
        material.commit();
        if direct {
            material.fy = 5e6;
        } else {
            material.set_yield(5e6);
        }
        let checkpoint = material.serialize_state();
        let probe = material.probe(0.001);
        assert_relative_eq!(probe.0, 14_950_000.0, epsilon = 1e-6);
        assert_relative_eq!(probe.1, 1e10, epsilon = 1e-5);
        assert_eq!(checkpoint, material.serialize_state());
        assert_eq!(probe, material.trial(0.001));
    }
    let mut material = Bilinear::new(1e12, 1e7, 0.01);
    material.trial(0.001);
    material.commit();
    material.e = 5e11;
    let probe = material.probe(0.001);
    assert_relative_eq!(probe.0, 9_950_000.0, epsilon = 1e-6);
    assert_relative_eq!(probe.1, 5e11, epsilon = 1e-5);
}
