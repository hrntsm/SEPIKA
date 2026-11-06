use sepika_material::UniaxialMaterial;

/// ファイバ（断面小片）。単位: `y`,`z` `[mm]`, `area` `[mm²]`。
/// `material` は材料種別の参照タグ。状態は `mats[fiber_index]` が持つ。
pub struct Fiber {
    pub y: f64,
    pub z: f64,
    pub area: f64,
    pub material: usize,
}

/// ファイバ断面：多数のファイバで構成される。
pub struct FiberSection {
    pub fibers: Vec<Fiber>,
}

/// 断面ひずみ（平面保持）。`eps0` `[無次元]`, `ky`,`kz` [1/mm]。
pub struct SectionStrain {
    pub eps0: f64,
    pub ky: f64,
    pub kz: f64,
}

/// 断面力（N, My, Mz）。`n` `[N]`, `my`,`mz` [N·mm]。
pub struct SectionForce {
    pub n: f64,
    pub my: f64,
    pub mz: f64,
}

/// 接線断面剛性（3×3：N–My–Mz と ε0–κy–κz の関係）。単位は N, N·mm, mm。
pub struct SectionStiffness {
    pub d: [[f64; 3]; 3],
}

/// 各ファイバのひずみ = ε0 − κz·y + κy·z（平面保持）から (σ, Et) を積分する。
///
/// **契約:** `mats.len() == sec.fibers.len()`。各ファイバは独立した履歴状態を持つこと。
pub fn section_response(
    sec: &FiberSection,
    strain: SectionStrain,
    mats: &mut [Box<dyn UniaxialMaterial>],
) -> (SectionForce, SectionStiffness) {
    assert_eq!(
        mats.len(),
        sec.fibers.len(),
        "section_response: mats.len() must equal fibers.len() (per-fiber state)"
    );
    integrate_fibers(sec, |i, fiber| {
        let eps = strain.eps0 - strain.kz * fiber.y + strain.ky * fiber.z;
        mats[i].trial(eps)
    })
}

/// ファイバー単位の応力と接線係数から、断面力と接線断面剛性 D を積分する。
/// 積分公式はこの関数だけが持つ（2 経路で共有しビット一致を保つ）。
/// 符号規約: `My = Σσ·A·z`、`Mz = −Σσ·A·y`。D は対称。
pub fn integrate_fibers(
    sec: &FiberSection,
    mut per_fiber: impl FnMut(usize, &Fiber) -> (f64, f64),
) -> (SectionForce, SectionStiffness) {
    let mut n = 0.0;
    let mut my = 0.0;
    let mut mz = 0.0;
    let mut d = [[0.0; 3]; 3];
    let mut force_error = [0.0; 3];
    let mut stiffness_error = [[0.0; 3]; 3];

    for (i, fiber) in sec.fibers.iter().enumerate() {
        let (sigma, et) = per_fiber(i, fiber);
        let a = fiber.area;

        add_compensated(&mut n, &mut force_error[0], sigma * a);
        add_compensated(&mut my, &mut force_error[1], sigma * a * fiber.z);
        add_compensated(&mut mz, &mut force_error[2], -sigma * a * fiber.y);

        let ea = et * a;
        let ey = ea * fiber.y;
        let ez = ea * fiber.z;

        add_compensated(&mut d[0][0], &mut stiffness_error[0][0], ea);
        add_compensated(&mut d[0][1], &mut stiffness_error[0][1], ez);
        add_compensated(&mut d[0][2], &mut stiffness_error[0][2], -ey);

        add_compensated(&mut d[1][0], &mut stiffness_error[1][0], ez);
        add_compensated(&mut d[1][1], &mut stiffness_error[1][1], ez * fiber.z);
        add_compensated(&mut d[1][2], &mut stiffness_error[1][2], -ey * fiber.z);

        add_compensated(&mut d[2][0], &mut stiffness_error[2][0], -ey);
        add_compensated(&mut d[2][1], &mut stiffness_error[2][1], -ey * fiber.z);
        add_compensated(&mut d[2][2], &mut stiffness_error[2][2], ey * fiber.y);
    }

    for i in 0..3 {
        for j in 0..3 {
            d[i][j] += stiffness_error[i][j];
        }
    }
    (
        SectionForce {
            n: n + force_error[0],
            my: my + force_error[1],
            mz: mz + force_error[2],
        },
        SectionStiffness { d },
    )
}

fn add_compensated(sum: &mut f64, error: &mut f64, value: f64) {
    let next = *sum + value;
    *error += if sum.abs() >= value.abs() {
        (*sum - next) + value
    } else {
        (value - next) + *sum
    };
    *sum = next;
}

/// 矩形断面をファイバに分割する。`material` は材料種別タグ。
pub fn rect_fiber_section(
    width: f64,
    depth: f64,
    nw: usize,
    nd: usize,
    material: usize,
) -> FiberSection {
    let mut fibers = Vec::with_capacity(nw * nd);
    let dw = width / nw as f64;
    let dd = depth / nd as f64;
    for i in 0..nw {
        for j in 0..nd {
            let y = (i as f64 + 0.5) * dw - width / 2.0;
            let z = (j as f64 + 0.5) * dd - depth / 2.0;
            fibers.push(Fiber {
                y,
                z,
                area: dw * dd,
                material,
            });
        }
    }
    FiberSection { fibers }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use sepika_material::{Bilinear, Concrete};

    fn steel_mats(sec: &FiberSection) -> Vec<Box<dyn UniaxialMaterial>> {
        (0..sec.fibers.len())
            .map(|_| Box::new(Bilinear::new(205000.0, 235.0, 0.01)) as Box<dyn UniaxialMaterial>)
            .collect()
    }

    #[test]
    fn cancelling_fiber_contributions_retain_small_force_and_coupling() {
        let sec = FiberSection {
            fibers: [1e16, 1.0, -1e16, 1.0]
                .into_iter()
                .map(|y| Fiber {
                    y,
                    z: 2.0,
                    area: 1.0,
                    material: 0,
                })
                .collect(),
        };
        let (force, stiffness) = integrate_fibers(&sec, |_, _| (1.0, 1.0));
        assert_eq!(force.n, 4.0);
        assert_eq!(force.my, 8.0);
        assert_eq!(force.mz, -2.0);
        assert_eq!(stiffness.d[0][2], -2.0);
        assert_eq!(stiffness.d[1][2], -4.0);
        assert_eq!(stiffness.d[2][0], -2.0);
        assert_eq!(stiffness.d[2][1], -4.0);
        let (force, _) = integrate_fibers(&sec, |i, _| ([1e16, 1.0, -1e16, 1.0][i], 1.0));
        assert_eq!(force.n, 2.0);
        assert_eq!(force.my, 4.0);
    }

    #[test]
    fn test_section_axial_strict() {
        let sec = rect_fiber_section(100.0, 200.0, 10, 20, 0);
        let mut mats = steel_mats(&sec);
        let (force, stiff) = section_response(
            &sec,
            SectionStrain {
                eps0: 0.001,
                ky: 0.0,
                kz: 0.0,
            },
            &mut mats,
        );
        assert_relative_eq!(force.n, 4_100_000.0, max_relative = 1e-9);
        assert_relative_eq!(force.my, 0.0, epsilon = 1e-9);
        assert_relative_eq!(force.mz, 0.0, epsilon = 1e-9);
        assert_relative_eq!(stiff.d[0][0], 4_100_000_000.0, max_relative = 1e-9);
    }

    #[test]
    fn test_section_pure_bending_strict() {
        let sec = rect_fiber_section(100.0, 200.0, 20, 40, 0);
        let mut mats = steel_mats(&sec);
        let ky = 1e-6;
        let (force, stiff) = section_response(
            &sec,
            SectionStrain {
                eps0: 0.0,
                ky,
                kz: 0.0,
            },
            &mut mats,
        );
        // 中点則の Iy = b*d³/12*(1 - 1/nd²) = 66,625,000 mm⁴。
        assert_relative_eq!(force.my, 13_658_125.0, max_relative = 1e-9);
        assert_relative_eq!(force.n, 0.0, epsilon = 1e-9);
        assert_relative_eq!(stiff.d[1][1], 13_658_125_000_000.0, max_relative = 1e-9);
    }

    #[test]
    #[should_panic(
        expected = "section_response: mats.len() must equal fibers.len() (per-fiber state)"
    )]
    fn test_section_mismatched_mats_panics() {
        let sec = rect_fiber_section(100.0, 200.0, 10, 20, 0);
        let mut mats: Vec<Box<dyn UniaxialMaterial>> =
            vec![Box::new(Bilinear::new(205000.0, 235.0, 0.01))];
        section_response(
            &sec,
            SectionStrain {
                eps0: 0.0,
                ky: 0.0,
                kz: 0.0,
            },
            &mut mats,
        );
    }

    #[test]
    fn test_section_yield_progression() {
        let sec = rect_fiber_section(100.0, 200.0, 4, 40, 0);
        let mut mats = steel_mats(&sec);
        let eps_y = 235.0 / 205000.0;
        let ky_y = eps_y / 100.0;
        let iy_disc: f64 = sec.fibers.iter().map(|f| f.area * f.z * f.z).sum();
        for &ratio in &[0.5, 1.0, 2.0, 3.0] {
            let ky = ky_y * ratio;
            let (force, _) = section_response(
                &sec,
                SectionStrain {
                    eps0: 0.0,
                    ky,
                    kz: 0.0,
                },
                &mut mats,
            );
            for m in mats.iter_mut() {
                m.commit();
            }
            let elastic_pred = ky * 205000.0 * iy_disc;
            if ratio <= 1.0 {
                assert_relative_eq!(force.my, elastic_pred, max_relative = 1e-9);
            } else {
                assert!(
                    force.my < elastic_pred,
                    "post-yield M ({}) must be below elastic prediction ({})",
                    force.my,
                    elastic_pred
                );
            }
        }
    }

    #[test]
    fn test_section_concrete_softening_mphi() {
        // コンクリート断面の圧縮側が軟化してもファイバ状態が独立していれば
        // 積分が破綻しない（共有状態だと履歴混入で発散する）。
        let sec = rect_fiber_section(100.0, 200.0, 4, 20, 0);
        let mut mats: Vec<Box<dyn UniaxialMaterial>> = (0..sec.fibers.len())
            .map(|_| Box::new(Concrete::new(30.0, 2.0)) as Box<dyn UniaxialMaterial>)
            .collect();
        let y_max = sec.fibers.iter().map(|f| f.y.abs()).fold(0.0, f64::max);
        let kz_peak = -0.002 / y_max;
        let mut moments = Vec::new();
        for &ratio in &[0.0, 0.5, 1.0, 1.5, 3.0] {
            let (force, _) = section_response(
                &sec,
                SectionStrain {
                    eps0: 0.0,
                    ky: 0.0,
                    kz: kz_peak * ratio,
                },
                &mut mats,
            );
            for m in mats.iter_mut() {
                m.commit();
            }
            moments.push(force.mz.abs());
        }
        assert!(moments.iter().all(|m| m.is_finite()));
        let peak = moments.iter().cloned().fold(0.0f64, f64::max);
        assert!(peak > 0.0);
        assert!(
            moments.last().unwrap() < &peak,
            "post-peak |M| must reduce: {moments:?}"
        );
    }
}
