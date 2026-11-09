//! Synaptic functions (NETWORK.md §4.8): callbacks or variable functions at a synapse, applied in order to each
//! stimulus crossing it, so the output comes out in the right logical order (R7).
//!
//! A function takes one stimulus and gives back none, one or several. It may keep state (what it holds back, the
//! last value it passed), and a time-based one asks to be ticked (`due`, `tick`). A chain belongs to one link, and so
//! to one signal.
//!
//! The eight built-ins are declared as data (`{"fn": "throttle", "seconds": 2}`), so any language runs them, at
//! either end (decision 51). Any closure is a function too, at the receiver's own end (decision 50).

use crate::errors::NetError;
use crate::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tokio::time::Instant;

/// What crosses a synapse towards the dendrites: here, a signal.
#[derive(Debug, Clone, PartialEq)]
pub struct Stimulus {
    pub signal: String,
    pub from: String,
    pub seq: u64,
    pub data: Value,
}

impl Stimulus {
    pub fn new(signal: impl Into<String>, from: impl Into<String>, seq: u64, data: Value) -> Stimulus {
        Stimulus {
            signal: signal.into(),
            from: from.into(),
            seq,
            data,
        }
    }
}

/// One synaptic function.
pub trait SynapticFunction: Send {
    /// Takes one stimulus in; gives back none, one or several.
    fn apply(&mut self, stimulus: Stimulus, now: Instant) -> Vec<Stimulus>;
    /// When it next wants to be ticked, if it holds something back.
    fn due(&self) -> Option<Instant> {
        None
    }
    /// Releases what its time has come for.
    fn tick(&mut self, _now: Instant) -> Vec<Stimulus> {
        Vec::new()
    }
}

/// Any closure is a synaptic function: the receiver's own, at its end.
impl<F> SynapticFunction for F
where
    F: FnMut(Stimulus) -> Vec<Stimulus> + Send,
{
    fn apply(&mut self, stimulus: Stimulus, _now: Instant) -> Vec<Stimulus> {
        self(stimulus)
    }
}

/// The functions of one link, applied in order.
#[derive(Default)]
pub struct Chain {
    functions: Vec<Box<dyn SynapticFunction>>,
}

impl Chain {
    pub fn new() -> Chain {
        Chain::default()
    }

    /// The built-ins, declared as data, in order.
    pub fn declared(declarations: &[Value]) -> Result<Chain, NetError> {
        let mut chain = Chain::new();
        for declaration in declarations {
            chain.functions.push(built_in(declaration)?);
        }
        Ok(chain)
    }

    /// This chain, then `f`.
    pub fn then(mut self, f: impl SynapticFunction + 'static) -> Chain {
        self.functions.push(Box::new(f));
        self
    }

    pub fn push(&mut self, f: Box<dyn SynapticFunction>) {
        self.functions.push(f);
    }

    /// This chain, then all of `other`'s functions.
    pub fn extend(&mut self, other: Chain) {
        self.functions.extend(other.functions);
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.functions.len()
    }

    pub fn apply(&mut self, stimulus: Stimulus, now: Instant) -> Vec<Stimulus> {
        self.run_from(0, vec![stimulus], now)
    }

    fn run_from(&mut self, start: usize, mut items: Vec<Stimulus>, now: Instant) -> Vec<Stimulus> {
        for f in &mut self.functions[start..] {
            if items.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for item in items {
                next.extend(f.apply(item, now));
            }
            items = next;
        }
        items
    }

    pub fn due(&self) -> Option<Instant> {
        self.functions.iter().filter_map(|f| f.due()).min()
    }

    /// Releases what has come due, each released stimulus passing on through the functions after its own.
    pub fn tick(&mut self, now: Instant) -> Vec<Stimulus> {
        let mut out = Vec::new();
        for i in 0..self.functions.len() {
            if self.functions[i].due().is_some_and(|due| due <= now) {
                let released = self.functions[i].tick(now);
                out.extend(self.run_from(i + 1, released, now));
            }
        }
        out
    }
}

fn bad(why: impl Into<String>) -> NetError {
    NetError::Protocol(why.into())
}

/// One built-in, from its declaration: `{"fn": name, ...its arguments}`.
pub fn built_in(declaration: &Value) -> Result<Box<dyn SynapticFunction>, NetError> {
    let name = declaration
        .get("fn")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("a function is declared with its \"fn\""))?;
    let field = |key: &str| declaration.get(key);
    Ok(match name {
        "only" => Box::new(Only {
            condition: Condition::parse(field("where").unwrap_or(&Value::Null))?,
        }),
        "pick" => Box::new(Pick {
            fields: strings(field("fields"), "pick")?,
        }),
        "rename" => Box::new(Rename {
            map: string_map(field("map"))?,
        }),
        "changed" => Box::new(Changed {
            fields: field("fields").map(|f| strings(Some(f), "changed")).transpose()?,
            last: HashMap::new(),
        }),
        "throttle" => Box::new(Throttle::new(seconds(field("seconds"), None)?)),
        "debounce" => Box::new(Debounce::new(seconds(field("seconds"), None)?)),
        "in_order" => Box::new(InOrder::new(
            field("by").and_then(Value::as_str).unwrap_or("seq"),
            seconds(field("limit"), Some(1.0))?,
        )),
        "gate" => Box::new(Gate {
            until: Condition::parse(field("until").unwrap_or(&Value::Null))?,
            open: false,
            held: Vec::new(),
        }),
        "batch" => Box::new(Batch::new(
            field("n").and_then(Value::as_u64).unwrap_or(100).max(1) as usize,
            seconds(field("seconds"), Some(0.25))?,
        )),
        other => return Err(bad(format!("there is no built-in function {other:?}"))),
    })
}

fn strings(value: Option<&Value>, what: &str) -> Result<Vec<String>, NetError> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| bad(format!("{what} takes a list of fields")))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| bad(format!("{what}'s fields are names")))
        })
        .collect()
}

fn string_map(value: Option<&Value>) -> Result<Vec<(String, String)>, NetError> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| bad("rename takes a map of old names to new"))?
        .iter()
        .map(|(k, v)| {
            v.as_str()
                .map(|v| (k.clone(), v.to_string()))
                .ok_or_else(|| bad("rename's new names are names"))
        })
        .collect()
}

fn seconds(value: Option<&Value>, default: Option<f64>) -> Result<Duration, NetError> {
    let s = match value {
        Some(v) => v.as_f64().ok_or_else(|| bad("a number of seconds"))?,
        None => default.ok_or_else(|| bad("a number of seconds is needed"))?,
    };
    if !s.is_finite() || s < 0.0 {
        return Err(bad("seconds are a number from 0"));
    }
    Ok(Duration::from_secs_f64(s))
}

/// A stimulus' field: one of its data's, or `$signal`, `$from`, `$seq`.
fn field_of(stimulus: &Stimulus, name: &str) -> Option<Value> {
    match name {
        "$signal" => Some(Value::from(stimulus.signal.clone())),
        "$from" => Some(Value::from(stimulus.from.clone())),
        "$seq" => Some(Value::from(stimulus.seq)),
        _ => stimulus.data.get(name).cloned(),
    }
}

enum Test {
    Equal(Value),
    In(Vec<Value>),
    Less(f64),
    More(f64),
    Present(bool),
    All(Vec<Test>),
}

impl Test {
    fn parse(value: &Value) -> Result<Test, NetError> {
        let Some(m) = value.as_object() else {
            return Ok(Test::Equal(value.clone()));
        };
        let words = ["in", "lt", "gt", "present"];
        if m.is_empty() || !m.keys().all(|k| words.contains(&k.as_str())) {
            return Ok(Test::Equal(value.clone()));
        }
        let mut all = Vec::new();
        for (k, v) in m {
            all.push(match k.as_str() {
                "in" => Test::In(v.as_array().cloned().ok_or_else(|| bad("in takes a list"))?),
                "lt" => Test::Less(v.as_f64().ok_or_else(|| bad("lt takes a number"))?),
                "gt" => Test::More(v.as_f64().ok_or_else(|| bad("gt takes a number"))?),
                _ => Test::Present(v.as_bool().ok_or_else(|| bad("present takes true or false"))?),
            });
        }
        Ok(Test::All(all))
    }

    fn holds(&self, value: Option<&Value>) -> bool {
        match self {
            Test::Equal(want) => value == Some(want),
            Test::In(list) => value.is_some_and(|v| list.contains(v)),
            Test::Less(n) => value.and_then(Value::as_f64).is_some_and(|v| v < *n),
            Test::More(n) => value.and_then(Value::as_f64).is_some_and(|v| v > *n),
            Test::Present(want) => value.is_some_and(|v| !v.is_null()) == *want,
            Test::All(tests) => tests.iter().all(|t| t.holds(value)),
        }
    }
}

/// Fields and their tests: every one must hold.
struct Condition(Vec<(String, Test)>);

impl Condition {
    fn parse(value: &Value) -> Result<Condition, NetError> {
        let m = value
            .as_object()
            .ok_or_else(|| bad("a condition is a map of fields to tests"))?;
        let mut tests = Vec::new();
        for (field, test) in m {
            tests.push((field.clone(), Test::parse(test)?));
        }
        Ok(Condition(tests))
    }

    fn holds(&self, stimulus: &Stimulus) -> bool {
        self.0
            .iter()
            .all(|(field, test)| test.holds(field_of(stimulus, field).as_ref()))
    }
}

/// `only(where)`: passes a stimulus whose fields match.
struct Only {
    condition: Condition,
}

impl SynapticFunction for Only {
    fn apply(&mut self, stimulus: Stimulus, _now: Instant) -> Vec<Stimulus> {
        if self.condition.holds(&stimulus) {
            vec![stimulus]
        } else {
            Vec::new()
        }
    }
}

/// `pick(fields)`: keeps some of the data's fields, in the data's own order.
struct Pick {
    fields: Vec<String>,
}

impl SynapticFunction for Pick {
    fn apply(&mut self, mut stimulus: Stimulus, _now: Instant) -> Vec<Stimulus> {
        if let Value::Object(m) = &stimulus.data {
            let kept: Map = m
                .iter()
                .filter(|(k, _)| self.fields.contains(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            stimulus.data = Value::Object(kept);
        }
        vec![stimulus]
    }
}

/// `rename(map)`: renames the data's fields, each keeping its place.
struct Rename {
    map: Vec<(String, String)>,
}

impl SynapticFunction for Rename {
    fn apply(&mut self, mut stimulus: Stimulus, _now: Instant) -> Vec<Stimulus> {
        if let Value::Object(m) = &stimulus.data {
            let renamed: Map = m
                .iter()
                .map(|(k, v)| {
                    let name = self.map.iter().find(|(old, _)| old == k).map_or(k, |(_, new)| new);
                    (name.clone(), v.clone())
                })
                .collect();
            stimulus.data = Value::Object(renamed);
        }
        vec![stimulus]
    }
}

/// `changed(fields)`: passes only when those fields (or the whole data) differ from the last passed.
struct Changed {
    fields: Option<Vec<String>>,
    last: HashMap<String, Value>,
}

impl SynapticFunction for Changed {
    fn apply(&mut self, stimulus: Stimulus, _now: Instant) -> Vec<Stimulus> {
        let seen = match &self.fields {
            Some(fields) => Value::Array(
                fields
                    .iter()
                    .map(|f| field_of(&stimulus, f).unwrap_or(Value::Null))
                    .collect(),
            ),
            None => stimulus.data.clone(),
        };
        if self.last.get(&stimulus.signal) == Some(&seen) {
            return Vec::new();
        }
        self.last.insert(stimulus.signal.clone(), seen);
        vec![stimulus]
    }
}

/// `throttle(seconds)`: at most one per interval, the latest kept. The first of a quiet spell passes at once; what
/// comes within the interval is held, the latest replacing the one before, and released when it ends.
pub struct Throttle {
    every: Duration,
    window_end: Option<Instant>,
    held: Option<Stimulus>,
}

impl Throttle {
    pub fn new(every: Duration) -> Throttle {
        Throttle {
            every,
            window_end: None,
            held: None,
        }
    }
}

impl SynapticFunction for Throttle {
    fn apply(&mut self, stimulus: Stimulus, now: Instant) -> Vec<Stimulus> {
        match self.window_end {
            Some(end) if now < end => {
                self.held = Some(stimulus);
                Vec::new()
            }
            _ => {
                self.window_end = Some(now + self.every);
                vec![stimulus]
            }
        }
    }

    fn due(&self) -> Option<Instant> {
        self.held.as_ref().and(self.window_end)
    }

    fn tick(&mut self, now: Instant) -> Vec<Stimulus> {
        match (self.held.take(), self.window_end) {
            (Some(held), Some(end)) if now >= end => {
                self.window_end = Some(now + self.every);
                vec![held]
            }
            (held, _) => {
                self.held = held;
                Vec::new()
            }
        }
    }
}

/// `debounce(seconds)`: one after a quiet interval, the latest.
struct Debounce {
    quiet: Duration,
    held: Option<(Stimulus, Instant)>,
}

impl Debounce {
    fn new(quiet: Duration) -> Debounce {
        Debounce { quiet, held: None }
    }
}

impl SynapticFunction for Debounce {
    fn apply(&mut self, stimulus: Stimulus, now: Instant) -> Vec<Stimulus> {
        self.held = Some((stimulus, now + self.quiet));
        Vec::new()
    }

    fn due(&self) -> Option<Instant> {
        self.held.as_ref().map(|(_, due)| *due)
    }

    fn tick(&mut self, now: Instant) -> Vec<Stimulus> {
        match self.held.take() {
            Some((held, due)) if now >= due => vec![held],
            other => {
                self.held = other;
                Vec::new()
            }
        }
    }
}

/// A key to order by: a sequence or a number, or text (an ISO time sorts as text).
#[derive(Debug, Clone, PartialEq, PartialOrd)]
enum Key {
    Number(f64),
    Text(String),
}

/// `in_order(by, limit)`: restores the order by a sequence (`seq`, the default) or a time field, holding early ones
/// back until the gap fills or the limit passes.
/// - By `seq`: sequence numbers are consecutive per source and signal. One already passed is dropped.
/// - By another field: each is held up to the limit, and released in that field's order.
struct InOrder {
    by: String,
    limit: Duration,
    next: Option<u64>,
    held: BTreeMap<u64, (Stimulus, Instant)>,
    buffer: Vec<(Key, Instant, Stimulus)>,
}

impl InOrder {
    fn new(by: &str, limit: Duration) -> InOrder {
        InOrder {
            by: by.to_string(),
            limit,
            next: None,
            held: BTreeMap::new(),
            buffer: Vec::new(),
        }
    }

    fn by_seq(&self) -> bool {
        self.by == "seq"
    }

    fn release_run(&mut self, out: &mut Vec<Stimulus>) {
        while let Some(next) = self.next {
            match self.held.remove(&next) {
                Some((stimulus, _)) => {
                    out.push(stimulus);
                    self.next = Some(next + 1);
                }
                None => break,
            }
        }
    }
}

impl SynapticFunction for InOrder {
    fn apply(&mut self, stimulus: Stimulus, now: Instant) -> Vec<Stimulus> {
        if !self.by_seq() {
            let key = match stimulus.data.get(&self.by) {
                Some(Value::Number(n)) => Key::Number(n.as_f64().unwrap_or(0.0)),
                Some(Value::String(s)) => Key::Text(s.clone()),
                _ => return vec![stimulus],
            };
            self.buffer.push((key, now, stimulus));
            return Vec::new();
        }
        let seq = stimulus.seq;
        let Some(next) = self.next else {
            self.next = Some(seq + 1);
            return vec![stimulus];
        };
        if seq < next {
            return Vec::new();
        }
        if seq > next {
            self.held.insert(seq, (stimulus, now));
            return Vec::new();
        }
        let mut out = vec![stimulus];
        self.next = Some(seq + 1);
        self.release_run(&mut out);
        out
    }

    fn due(&self) -> Option<Instant> {
        if self.by_seq() {
            self.held.values().map(|(_, at)| *at + self.limit).min()
        } else {
            self.buffer.iter().map(|(_, at, _)| *at + self.limit).min()
        }
    }

    fn tick(&mut self, now: Instant) -> Vec<Stimulus> {
        let mut out = Vec::new();
        if self.by_seq() {
            // The oldest gap has waited long enough: give up on it, and go on from what was held.
            while self.held.values().any(|(_, at)| *at + self.limit <= now) {
                let Some(first) = self.held.keys().next().copied() else {
                    break;
                };
                self.next = Some(first);
                self.release_run(&mut out);
            }
            return out;
        }
        let overdue = self
            .buffer
            .iter()
            .filter(|(_, at, _)| *at + self.limit <= now)
            .map(|(k, _, _)| k.clone());
        let Some(upto) = overdue.reduce(|a, b| if b > a { b } else { a }) else {
            return out;
        };
        let (mut ready, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.buffer)
            .into_iter()
            .partition(|(k, _, _)| *k <= upto);
        self.buffer = keep;
        ready.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        out.extend(ready.into_iter().map(|(_, _, s)| s));
        out
    }
}

/// `gate(until)`: holds everything back until a stimulus meets the condition, then releases it all in order, that one
/// last, and passes everything after.
struct Gate {
    until: Condition,
    open: bool,
    held: Vec<Stimulus>,
}

impl SynapticFunction for Gate {
    fn apply(&mut self, stimulus: Stimulus, _now: Instant) -> Vec<Stimulus> {
        if self.open {
            return vec![stimulus];
        }
        if self.until.holds(&stimulus) {
            self.open = true;
            let mut out = std::mem::take(&mut self.held);
            out.push(stimulus);
            return out;
        }
        self.held.push(stimulus);
        Vec::new()
    }
}

/// `batch(n, seconds)`: gathers up to n, or what came within the interval, into one stimulus whose data is the list
/// of theirs.
struct Batch {
    n: usize,
    every: Duration,
    held: Vec<Stimulus>,
    started: Option<Instant>,
}

impl Batch {
    fn new(n: usize, every: Duration) -> Batch {
        Batch {
            n,
            every,
            held: Vec::new(),
            started: None,
        }
    }

    fn flush(&mut self) -> Vec<Stimulus> {
        self.started = None;
        let items = std::mem::take(&mut self.held);
        let (Some(first), Some(last)) = (items.first(), items.last()) else {
            return Vec::new();
        };
        let (signal, from, seq) = (first.signal.clone(), first.from.clone(), last.seq);
        let data = Value::Array(items.into_iter().map(|s| s.data).collect());
        vec![Stimulus {
            signal,
            from,
            seq,
            data,
        }]
    }
}

impl SynapticFunction for Batch {
    fn apply(&mut self, stimulus: Stimulus, now: Instant) -> Vec<Stimulus> {
        if self.held.is_empty() {
            self.started = Some(now);
        }
        self.held.push(stimulus);
        if self.held.len() >= self.n {
            self.flush()
        } else {
            Vec::new()
        }
    }

    fn due(&self) -> Option<Instant> {
        self.started.map(|at| at + self.every)
    }

    fn tick(&mut self, now: Instant) -> Vec<Stimulus> {
        match self.started {
            Some(at) if now >= at + self.every => self.flush(),
            _ => Vec::new(),
        }
    }
}
