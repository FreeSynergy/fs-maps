//! Model-based property test: the cache behaves exactly like a reference LRU and never exceeds
//! its limit (index and disk).

use std::collections::VecDeque;

use fs_maps_core::{CacheConfig, TileCache};
use proptest::prelude::*;

const HEADER: usize = 8;

#[derive(Debug, Clone)]
enum Op {
    Put(u8, usize),
    Get(u8),
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(
        prop_oneof![
            (0u8..12, 0usize..60).prop_map(|(k, n)| Op::Put(k, n)),
            (0u8..12).prop_map(Op::Get),
        ],
        1..80,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn size_limit_and_lru_model(ops in ops(), max in 20u64..300) {
        let d = tempfile::tempdir().unwrap();
        let mut cfg = CacheConfig::new(d.path(), max);
        cfg.ttl = None;
        let c = TileCache::open(cfg).unwrap();
        let mut model: VecDeque<(u8, u64)> = VecDeque::new(); // oldest first
        for op in ops {
            match op {
                Op::Put(k, n) => {
                    let size = (HEADER + n) as u64;
                    let stored = c.put("s", 3, u32::from(k), 0, &vec![k; n]).unwrap();
                    model.retain(|e| e.0 != k);
                    if size <= max {
                        prop_assert!(stored);
                        model.push_back((k, size));
                        while model.iter().map(|e| e.1).sum::<u64>() > max {
                            model.pop_front();
                        }
                    } else {
                        prop_assert!(!stored);
                    }
                }
                Op::Get(k) => {
                    let got = c.get("s", 3, u32::from(k), 0);
                    let pos = model.iter().position(|e| e.0 == k);
                    prop_assert_eq!(got.is_some(), pos.is_some());
                    if let Some(p) = pos {
                        let e = model.remove(p).unwrap();
                        model.push_back(e);
                    }
                }
            }
            let s = c.stats();
            prop_assert!(s.bytes <= max);
            prop_assert_eq!(s.bytes, model.iter().map(|e| e.1).sum::<u64>());
            prop_assert_eq!(c.disk_bytes(), s.bytes);
        }
        let order: Vec<u32> = c.keys_oldest_first().iter().map(|k| k.2).collect();
        let want: Vec<u32> = model.iter().map(|e| u32::from(e.0)).collect();
        prop_assert_eq!(order, want);
    }
}
