use sepika_material::{
    Bilinear, Concrete, ConcreteCyclic, ConcreteNewRc, HysteresisMaterial, HysteresisRule,
    TsujiYamada, UniaxialMaterial,
};

#[test]
fn checkpoints_preserve_committed_and_outstanding_trial_history() {
    let mut materials: Vec<Box<dyn UniaxialMaterial>> = vec![
        Box::new(Bilinear::new(205000.0, 235.0, 0.01)),
        Box::new(Concrete::new(30.0, 2.0)),
        Box::new(ConcreteCyclic::kent_park(
            30.0, 0.002, 6.0, 0.006, 2.0, 1000.0,
        )),
        Box::new(ConcreteCyclic::newrc(30.0, 0.01, 2.0, 1000.0)),
        Box::new(ConcreteNewRc::new(30.0, 2.0)),
        Box::new(TsujiYamada::new(205000.0, 235.0, 2050.0, 0.5)),
    ];
    for rule in [
        HysteresisRule::Takeda {
            crack: (40.0, 0.002),
            yield_point: (100.0, 0.01),
            ultimate: (120.0, 0.05),
            alpha: 0.4,
        },
        HysteresisRule::OriginOriented {
            yield_point: (100.0, 0.01),
            ultimate: (120.0, 0.05),
        },
        HysteresisRule::Retrograde {
            crack: (40.0, 0.002),
            yield_point: (100.0, 0.01),
            ultimate: (120.0, 0.05),
        },
        HysteresisRule::Standard {
            crack: (40.0, 0.002),
            yield_point: (100.0, 0.01),
            ultimate: (120.0, 0.05),
        },
        HysteresisRule::MaxPointOriented {
            crack: (40.0, 0.002),
            yield_point: (100.0, 0.01),
            ultimate: (120.0, 0.05),
        },
    ] {
        materials.push(Box::new(HysteresisMaterial::new(rule)));
    }
    for mut direct in materials {
        let mut restored = direct.clone_box();
        for strain in [-0.004, 0.001, -0.002, 0.03, 0.01, -0.005] {
            direct.trial(strain);
            direct.commit();
        }
        let committed = direct.serialize_state();
        direct.trial(0.02);
        direct.revert();
        assert_eq!(direct.serialize_state(), committed);
        let outstanding = direct.trial(0.008);
        let checkpoint = direct.serialize_state();
        for strain in [-0.01, 0.02] {
            let expected = direct.clone_box().trial(strain);
            assert_eq!(direct.probe(strain), expected);
            assert_eq!(direct.serialize_state(), checkpoint);
        }
        restored
            .deserialize_state(&checkpoint)
            .expect("履歴を復元できる");
        assert_eq!(restored.serialize_state(), checkpoint);
        assert!(restored.deserialize_state(&[0xff, 0, 1]).is_err());
        assert_eq!(restored.serialize_state(), checkpoint);
        direct.commit();
        restored.commit();
        assert_eq!(restored.probe(0.008), direct.probe(0.008));
        assert!(outstanding.0.is_finite() && outstanding.1.is_finite());
        for strain in [0.009, -0.001, -0.03, -0.015, 0.005, 0.04] {
            assert_eq!(restored.trial(strain), direct.trial(strain));
            direct.commit();
            restored.commit();
            assert_eq!(restored.serialize_state(), direct.serialize_state());
        }
    }
}
