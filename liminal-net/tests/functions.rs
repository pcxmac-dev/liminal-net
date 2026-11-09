//! The eight built-in synaptic functions (decision 51), a closure as a function, and chains.

use liminal_net::Value;
use liminal_net::functions::{Chain, Stimulus, built_in};
use serde_json::json;
use std::time::Duration;
use tokio::time::Instant;

fn s(seq: u64, data: Value) -> Stimulus {
    Stimulus::new("lines", "a", seq, data)
}

fn chain(declared: Value) -> Chain {
    Chain::declared(declared.as_array().unwrap()).unwrap()
}

fn data(out: &[Stimulus]) -> Vec<Value> {
    out.iter().map(|s| s.data.clone()).collect()
}

const MS: Duration = Duration::from_millis(1);

#[test]
fn only_passes_what_matches() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "only", "where": {"kind": "error"}}]));
    assert_eq!(c.apply(s(1, json!({"kind": "error"})), t).len(), 1);
    assert!(c.apply(s(2, json!({"kind": "info"})), t).is_empty());
    let mut c = chain(json!([{"fn": "only", "where": {"n": {"gt": 2, "lt": 5}, "x": {"present": true}}}]));
    assert_eq!(c.apply(s(1, json!({"n": 3, "x": 1})), t).len(), 1);
    assert!(c.apply(s(2, json!({"n": 3})), t).is_empty(), "x missing");
    assert!(c.apply(s(3, json!({"n": 6, "x": 1})), t).is_empty(), "n too big");
    let mut c = chain(json!([{"fn": "only", "where": {"kind": {"in": ["a", "b"]}, "$signal": "lines"}}]));
    assert_eq!(c.apply(s(1, json!({"kind": "b"})), t).len(), 1);
    assert!(
        c.apply(Stimulus::new("status", "a", 1, json!({"kind": "b"})), t)
            .is_empty()
    );
}

#[test]
fn pick_and_rename_keep_the_datas_order() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "pick", "fields": ["c", "a"]}, {"fn": "rename", "map": {"c": "see"}}]));
    let out = c.apply(s(1, json!({"a": 1, "b": 2, "c": 3})), t);
    assert_eq!(serde_json::to_string(&out[0].data).unwrap(), r#"{"a":1,"see":3}"#);
}

#[test]
fn changed_passes_only_a_change() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "changed", "fields": ["state"]}]));
    let passed: Vec<_> = [
        json!({"state": "a", "n": 1}),
        json!({"state": "a", "n": 2}),
        json!({"state": "b", "n": 3}),
    ]
    .into_iter()
    .enumerate()
    .flat_map(|(i, d)| c.apply(s(i as u64, d), t))
    .collect();
    assert_eq!(
        data(&passed),
        vec![json!({"state": "a", "n": 1}), json!({"state": "b", "n": 3})]
    );
    let mut whole = chain(json!([{"fn": "changed"}]));
    assert_eq!(whole.apply(s(1, json!(1)), t).len(), 1);
    assert!(whole.apply(s(2, json!(1)), t).is_empty());
}

#[test]
fn throttle_keeps_the_latest_and_releases_it_when_its_interval_ends() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "throttle", "seconds": 0.1}]));
    assert_eq!(
        data(&c.apply(s(1, json!(1)), t)),
        vec![json!(1)],
        "the first passes at once"
    );
    assert!(c.apply(s(2, json!(2)), t + 10 * MS).is_empty());
    assert!(c.apply(s(3, json!(3)), t + 20 * MS).is_empty());
    assert_eq!(c.due(), Some(t + 100 * MS));
    assert!(c.tick(t + 50 * MS).is_empty(), "not yet");
    assert_eq!(
        data(&c.tick(t + 100 * MS)),
        vec![json!(3)],
        "the latest, when the interval ends"
    );
    assert_eq!(c.due(), None);
}

#[test]
fn debounce_waits_for_quiet() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "debounce", "seconds": 0.05}]));
    assert!(c.apply(s(1, json!(1)), t).is_empty());
    assert!(c.apply(s(2, json!(2)), t + 40 * MS).is_empty());
    assert!(c.tick(t + 60 * MS).is_empty(), "the second reset the quiet");
    assert_eq!(data(&c.tick(t + 90 * MS)), vec![json!(2)]);
}

#[test]
fn in_order_holds_early_ones_until_the_gap_fills() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "in_order", "limit": 0.1}]));
    assert_eq!(c.apply(s(1, json!(1)), t).len(), 1);
    assert!(c.apply(s(3, json!(3)), t).is_empty(), "3 waits for 2");
    assert!(c.apply(s(4, json!(4)), t).is_empty());
    assert_eq!(data(&c.apply(s(2, json!(2)), t)), vec![json!(2), json!(3), json!(4)]);
    assert!(c.apply(s(2, json!(2)), t).is_empty(), "one already passed is dropped");
    // A gap that never fills is given up on when the limit passes.
    assert!(c.apply(s(6, json!(6)), t).is_empty());
    assert!(c.tick(t + 50 * MS).is_empty());
    assert_eq!(data(&c.tick(t + 100 * MS)), vec![json!(6)]);
    assert_eq!(data(&c.apply(s(7, json!(7)), t)), vec![json!(7)]);
}

#[test]
fn in_order_by_a_time_field_releases_in_its_order() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "in_order", "by": "at", "limit": 0.05}]));
    assert!(c.apply(s(1, json!({"at": "2026-11-09T10:00:02"})), t).is_empty());
    assert!(
        c.apply(s(2, json!({"at": "2026-11-09T10:00:01"})), t + 10 * MS)
            .is_empty()
    );
    let out = c.tick(t + 50 * MS);
    assert_eq!(
        data(&out),
        vec![
            json!({"at": "2026-11-09T10:00:01"}),
            json!({"at": "2026-11-09T10:00:02"})
        ],
        "the later-held earlier time goes first with it"
    );
}

#[test]
fn gate_holds_everything_until_its_condition() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "gate", "until": {"started": true}}]));
    assert!(c.apply(s(1, json!({"line": "a"})), t).is_empty());
    assert!(c.apply(s(2, json!({"line": "b"})), t).is_empty());
    assert_eq!(
        data(&c.apply(s(3, json!({"started": true})), t)),
        vec![json!({"line": "a"}), json!({"line": "b"}), json!({"started": true})]
    );
    assert_eq!(c.apply(s(4, json!({"line": "c"})), t).len(), 1, "open from then on");
}

#[test]
fn batch_gathers_by_count_or_by_time() {
    let t = Instant::now();
    let mut c = chain(json!([{"fn": "batch", "n": 3, "seconds": 0.25}]));
    assert!(c.apply(s(1, json!(1)), t).is_empty());
    assert!(c.apply(s(2, json!(2)), t).is_empty());
    let out = c.apply(s(3, json!(3)), t);
    assert_eq!((out[0].data.clone(), out[0].seq), (json!([1, 2, 3]), 3));
    assert!(c.apply(s(4, json!(4)), t).is_empty());
    assert_eq!(
        data(&c.tick(t + 250 * MS)),
        vec![json!([4])],
        "what came within the interval"
    );
}

#[test]
fn a_chain_applies_its_functions_in_order() {
    let t = Instant::now();
    // Throttle first, then changed: what the throttle releases is checked for a change.
    let mut c = chain(json!([{"fn": "throttle", "seconds": 0.1}, {"fn": "changed"}]));
    assert_eq!(c.apply(s(1, json!("a")), t).len(), 1);
    assert!(c.apply(s(2, json!("a")), t + 10 * MS).is_empty());
    assert!(
        c.tick(t + 100 * MS).is_empty(),
        "released by the throttle, dropped as unchanged"
    );
}

#[test]
fn a_closure_is_a_function_at_the_receivers_end() {
    let t = Instant::now();
    let mut c = Chain::new().then(|s: Stimulus| if s.seq.is_multiple_of(2) { vec![s] } else { vec![] });
    let passed: Vec<_> = (1..=6).flat_map(|i| c.apply(s(i, json!(i)), t)).collect();
    assert_eq!(data(&passed), vec![json!(2), json!(4), json!(6)]);
}

#[test]
fn a_declaration_that_is_not_a_built_in_is_refused() {
    assert!(built_in(&json!({"fn": "teleport"})).is_err());
    assert!(built_in(&json!({"seconds": 1})).is_err());
    assert!(
        built_in(&json!({"fn": "throttle"})).is_err(),
        "throttle needs its seconds"
    );
    assert!(built_in(&json!({"fn": "pick", "fields": "a"})).is_err());
}
