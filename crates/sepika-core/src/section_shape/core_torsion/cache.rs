use std::num::NonZeroUsize;
use std::sync::{Mutex, OnceLock};

use lru::LruCache;

use super::controller::rounded_rect_core_torsion;

const CAPACITY: usize = 64;
type ScalarCache = Mutex<LruCache<[u64; 3], f64>>;
static CORE_J: OnceLock<ScalarCache> = OnceLock::new();

pub(in crate::section_shape) fn reused_rounded_rect_core_j(
    b_mm: f64,
    h_mm: f64,
    inner_r_mm: f64,
) -> Result<f64, String> {
    let cache = CORE_J.get_or_init(new_cache);
    reuse(cache, b_mm, h_mm, inner_r_mm, || {
        rounded_rect_core_torsion(b_mm, h_mm, inner_r_mm)
            .map(|solution| solution.j_mm4)
            .map_err(|error| error.to_string())
    })
}

fn new_cache() -> ScalarCache {
    Mutex::new(LruCache::new(NonZeroUsize::new(CAPACITY).unwrap()))
}

fn reuse(
    cache: &ScalarCache,
    b_mm: f64,
    h_mm: f64,
    inner_r_mm: f64,
    solve: impl FnOnce() -> Result<f64, String>,
) -> Result<f64, String> {
    if !b_mm.is_finite()
        || !h_mm.is_finite()
        || !inner_r_mm.is_finite()
        || b_mm <= 0.0
        || h_mm <= 0.0
        || inner_r_mm < 0.0
        || inner_r_mm > b_mm.min(h_mm) / 2.0
    {
        return Err("コアJ再利用: 有限正の内法寸法と0≤内角R≤短辺/2が必要です".into());
    }
    let key = [b_mm.to_bits(), h_mm.to_bits(), inner_r_mm.to_bits()];
    {
        let mut locked = cache
            .lock()
            .map_err(|_| "コアJ再利用: 保存値のロックが破損しています")?;
        if let Some(j_mm4) = locked.get(&key) {
            return Ok(*j_mm4);
        }
    }
    let j_mm4 = solve()?;
    if !j_mm4.is_finite() || j_mm4 <= 0.0 {
        return Err("コアJ再利用: 求解値が正の有限値ではありません".into());
    }
    cache
        .lock()
        .map_err(|_| "コアJ再利用: 保存値のロックが破損しています")?
        .put(key, j_mm4);
    Ok(j_mm4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn identical_geometry_reuses_scalar_and_exact_dimension_bits_separate_entries() {
        let cache = new_cache();
        assert_eq!(reuse(&cache, 276.0, 376.0, 18.0, || Ok(123.0)), Ok(123.0));
        assert_eq!(
            reuse(&cache, 276.0, 376.0, 18.0, || panic!("再求解")),
            Ok(123.0)
        );
        for (b, h, r) in [
            (f64::from_bits(276.0f64.to_bits() + 1), 376.0, 18.0),
            (276.0, f64::from_bits(376.0f64.to_bits() + 1), 18.0),
            (276.0, 376.0, f64::from_bits(18.0f64.to_bits() + 1)),
        ] {
            assert_eq!(reuse(&cache, b, h, r, || Ok(456.0)), Ok(456.0));
        }
        assert_eq!(cache.lock().unwrap().len(), 4);
    }

    #[test]
    fn failed_or_nonpositive_solutions_are_retried_without_holding_the_lock() {
        let cache = new_cache();
        let calls = Cell::new(0);
        for outcome in [Err("未収束".into()), Ok(f64::NAN), Ok(0.0), Ok(-1.0)] {
            assert!(reuse(&cache, 276.0, 376.0, 18.0, || {
                assert!(cache.try_lock().is_ok());
                calls.set(calls.get() + 1);
                outcome
            })
            .is_err());
            assert!(cache.lock().unwrap().is_empty());
        }
        assert_eq!(calls.get(), 4);
        assert_eq!(reuse(&cache, 276.0, 376.0, 18.0, || Ok(123.0)), Ok(123.0));
    }

    #[test]
    fn only_sixty_four_shapes_remain_and_evicted_geometry_is_solved_again() {
        let cache = new_cache();
        for b in 1..=65 {
            reuse(&cache, b as f64, 100.0, 0.0, || Ok(b as f64)).unwrap();
        }
        assert_eq!(cache.lock().unwrap().len(), 64);
        let called = Cell::new(false);
        reuse(&cache, 1.0, 100.0, 0.0, || {
            called.set(true);
            Ok(1.0)
        })
        .unwrap();
        assert!(called.get());
        assert_eq!(cache.lock().unwrap().len(), 64);
    }

    #[test]
    fn invalid_geometry_never_invokes_solver_or_enters_cache() {
        let cache = new_cache();
        for (b, h, r) in [
            (f64::NAN, 100.0, 0.0),
            (100.0, f64::INFINITY, 0.0),
            (100.0, 100.0, f64::NAN),
            (0.0, 100.0, 0.0),
            (100.0, -1.0, 0.0),
            (100.0, 100.0, -1.0),
            (100.0, 100.0, 51.0),
        ] {
            assert!(reuse(&cache, b, h, r, || panic!("不正幾何の求解")).is_err());
        }
        assert!(cache.lock().unwrap().is_empty());
    }

    #[test]
    fn poisoned_cache_returns_reasoned_error_without_solving() {
        let cache = new_cache();
        let _ = std::panic::catch_unwind(|| {
            let _locked = cache.lock().unwrap();
            panic!("破損の再現");
        });
        assert!(reuse(&cache, 276.0, 376.0, 18.0, || panic!("破損後の求解"))
            .unwrap_err()
            .contains("ロック"));
    }
}
