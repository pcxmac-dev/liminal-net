# A node's queue and its thread: the specification of liminal-net 0.3

Written on 2026-11-09 from NETWORK.md §4.11 and its decisions 93 to 110, from three independent designs and three reviews of them, and from the source of liminal-net 0.2 as it stands. Revised twice the same day: after two attacks on the first draft, and after a second round that found 45 flaws in the second draft, each resolved here. Nothing here is built yet. liminal-net 0.3 is built from this document, in the order of section 12.

How to read the citations:
- `node.rs:285` is a line of `liminal-net/src`; `tests/network.rs:444` is under `liminal-net`;
- `bridge:546` is a line of `liminal-net-py/src/lib.rs`; `test_bridge.py:208` is under `liminal-net-py/tests`;
- `tokio-1.54.1 src/...`, `serde_json-1.0.154 src/...`, `pyo3-0.30.1 src/...` and `pyo3-ffi-0.30.1 src/...` are the crates' sources as Cargo fetched them;
- "decision 97" is a row of NETWORK.md §10. NETWORK.md is cited by section and decision, and by line only up to §5.4 (the lines are those of commit c7bd0f1).
- Every line cited was read again against the tree at commit c7bd0f1 for this revision. The numbers move with the next commit, so they are read once more before each step is built.
- What is known of tokio, PyO3, maturin, serde_json or the operating systems, and was not read in a source, is marked (inferred), with the test that checks it.
- Thirteen points are the user's to decide, (Q1) to (Q13), listed in section 14. Q1 to Q6, Q8, Q12 and Q13 are built behind one function or constant each. **Where a decided row speaks, the first behaviour is the row's own words** (NETWORK.md:14-15: the code follows only decided items); the alternative is the question. Q8 to Q13 are sentences of NETWORK.md that 0.3 would change or add to: each is one sentence and one yes, and NETWORK.md is not edited for them before that yes.

## 1. What it is for, and what it rests on

A node wraps a stateful object. Version 0.3 gives every node a queue of its own: the signals that arrived for its state, and the signals it fires. Until a node needs more, a small set of shared server threads serves every node's queue in fair turns. A node whose queue its process does not serve fast enough, or whose own code says so, takes one thread of its own within the process and keeps it until it ends. Requests never wait in the queue: they run beside it.

0.3 also lets nodes of one process deliver to each other without a pipe, bounds every line, counts and tells every loss, gives a node's end an order and a bound, and mends the faults found in 0.1 and 0.2 (section 10). The wire does not change: protocol 1, no new op, the test vectors as they are. What a peer written from NETWORK.md alone would see differently is listed in section 10.2, each point with the row it touches.

| decisions | what they fix here |
|---|---|
| 93, 94, 96, 99, 105 | when a node takes a thread (measured, declared, or by its own call), that it keeps it, its priority, and how many |
| 95, 48, 109 | the queue holds signals, taken one at a time; requests run beside it, uncapped in 0.3 |
| 100, 101, 102 | the shared servers; one line per source, served in fair turns; whatever serves a node also sends what it fires, and a firing thread helps at half |
| 103 | how the measure is sure before a node moves |
| 104, 97, 43, 91 | the caps; nobody waits at a full line; the oldest stream item is dropped, counted and told; a dropped item keeps its number |
| 20, 90 | state signals: coalesced at the sender, the latest wins, a new link gets the latest at once |
| 49 to 55, 92 | synaptic functions, where they run, and that link, unlink and tune are answered |
| 98 | a node's own periodic work, for every node, built when hw-stats is ported |
| 107, 13, 27, 83 | the end: bounded, data secured, what was left said; the lifecycle around it |
| 12, 42, 108, 45, 47 | direct delivery in one process, the encoded test mode, the wire; the same 16 MB rule and errors in all three; no timeout by default |
| 60, NETWORK.md §4.9 | node code is plain synchronous functions on threads that are never tokio's async ones; Python node code never runs on an async thread; every call from Python into the network lets go of the interpreter's lock |
| 106, 79 | priorities low, normal, high, never real-time; `libc` for Unix targets |
| 89 | passing a signal on is the node's own choice, with `fire` |
| 110 | (Q1) what a built-in function may hold. The brief of this revision leaves the row open; the log on disk records it as decided, "Capped, oldest dropped" (commit e4193c5). Both behaviours are specified behind one function; no cap is built until the user confirms the row |

Not part of this document: request-side functions (decision 53 is not built: dendrites.rs:97-128), the macro of decision 38, how a node's report travels to the app that draws Connections (decision 87), and the Unix key file's permissions, which NETWORK.md §4.11's last paragraph gives to 0.3 as a change of `key.rs:83-84, 139-143` with CI-only tests of its own (Q7: a commit beside S0, outside the queue).

## 2. The guarantees

Each is stated exactly, and each names its tests (section 11).

**G1. One at a time.** For one node, the receiver's functions, `integrate`, `lost`, `pulse` and the receiver-side ticks never run two at once: on shared handling, during the move, and on its own thread. Every such call is made through `node_code`, which in one section under the queue's lock checks that the calling thread still holds the in-right, takes what the call is for, and marks the call begun (5.3). `shutdown` starts only when none of them runs, with the exceptions of G14. Requests, `admits`, `attach`, `detach` and lent callables run beside them (decision 95). Tests: X15, Q14, Q23, A1, E7.

**G2. Order in.** Stimuli from one source (a peer's name and scope) are taken in the order they arrived, across that source's signals. What a receiver's functions give back is integrated in the order they gave it, before anything later of that source. Across that source's synapses stimuli are taken synapse by synapse, in the order the synapses were admitted: nothing of a newer synapse is taken before the older one's closing marker. A dropped stimulus is skipped, never reordered. Across sources there is no order (decision 54): sources take turns, one take each. Tests: Q1, Q2, Q16, Q25.

**G3. Order out.** What a node fires passes its links' functions in the order the calls of `fire` took the queue's lock; for one thread that is the order it called. Each target receives what passed in that order, across signals; what a function released at once, or a replaced function still held, goes out in pieces, and nothing fired later passes it. A `link`, `unlink` or `tune` takes a place of its own in the line of fired signals: it takes effect after every signal fired before it arrived and before every signal fired after, and it is answered once it is in effect. A `detach` is written after what waited for that target. Tests: Q13, X3, X4, X5, X16, X18.

**G4. State signals.** A state signal takes at most two places in a line: the value that has waited longest and the latest. A newer value replaces only the latest, and goes to the back, so it never overtakes what arrived before it. A state value is never dropped by a full line, never lost to a write that failed (7.5), and never counted as lost. The first of a quiet spell goes at once (the sender's 250 ms throttle, node.rs:375-383). While a link lives and both nodes run, the last value fired is the last value integrated; a tune, a re-made link and the node's end pass on what a function still held. Three limits, said here:
- a state value that, as framed, is over 16 MB or nested too deep is refused (counted `too_large`): the receiver keeps the value before it, and a link made later gets the last value that fitted;
- a state signal linked by a third node (the op's `to` is free on the wire, message.rs:93-99, while `Remote::link` always names itself, synapse.rs:215-223) is unknown to the receiver as a state and waits as a stream;
- what a `gate` still holds closed is not passed on.
Tests: Q3, Q4, Q5, Q24, X5, X18, X22, X23, E1, E2, P4.

**G5. Bounded memory.**
- One line (one source's arriving line, or one target's line): at most `line_stimuli` (1,024) stream stimuli and `line_bytes` (16 MB) of them. A stimulus always fits an empty line.
- One node: at most `node_stimuli` (4,096) stream stimuli and `node_bytes` (64 MB) over all its lines, arriving and outgoing together (decision 104's words; Q3 asks whether each side should have its own). A push may pass it by one stimulus per pushing thread until the trim that follows it.
- The line of fired signals: at most `fired` (256) plus one per thread that is inside `fire` at that moment, in every mode (7.5), plus the `link`, `unlink` and `tune` ops that wait in it (control frames, uncapped with the requests: decision 109).
- Bytes are a stimulus's size as JSON: its frame's length when it crossed a pipe or the codec, an estimate otherwise. The heap it takes is a small multiple of that.
- Outside the bound, and named. The report shows each (`waiting.state_bytes`, `waiting.released`, `kept`):
  - state values: two per state signal and synapse (arriving) or target (outgoing), each at most 16 MB. They take no part in a line's or the node's count, so a node linked to many large states holds more than `node_bytes`;
  - what `gate` and `in_order` hold (Q1), what a function released at once and what a replaced function held on its way out (`carry`, `passed`), and what a receiver's functions gave back and waits to be integrated (`released`): each is at most what the functions held or gave, so with Q1 answered "capped" each is at most a line;
  - requests in progress and their frames (decision 109);
  - `Node::latest` (node.rs:97), `links_in`, `kinds`, the per-link numbers `Axon.seqs`, `Axon.branches`, `reaching`, and the empty lines of every source ever seen: none is pruned before the node ends. A lane (one signal's place in a line) leaves when it holds nothing, so signal names a peer invents do not pile up there.
Tests: Q5, Q6, Q7, Q9, Q13, X6, A8.

**G6. No silent loss.** A stream stimulus that passed the sender's functions and got its number is lost only in these cases, and each is counted where it happened, with its reason:
- (a) a full line or a full node, at either end;
- (b) a message that, as framed, is over 16 MB or nested deeper than `MAX_DEPTH` (at the sender: `too_large`);
- (c) left at a node's end when its bound passed (`left`);
- (d) left when its link ended (at the sender: `ended`, a count kept per target after the link is gone).
A write that failed loses nothing: the entry goes back to its line (7.5). A number that arrives twice is taken once (`repeated`). While both nodes run, the receiving code is told how many it missed, through `Hillock::lost`: before the first stimulus that follows a gap, and for gaps found later in the same turn at the end of that turn; so at most twice per link per turn, and never later than the turn that saw the gap. For every stream link at rest: numbers given = taken + told + owed + waiting + above, where "above" is the numbers higher than the last one taken that never arrived; each of those is in the sender's counts. What stays inexact, and is said:
- a loss at the tail shows no gap at the receiver until a later stimulus comes;
- a source node made again in a process that keeps its pid, reaching over a pipe, whose first numbers were dropped while the receiver's last number was higher, is under-told by that last number (the sender's count is exact); the same for a link made again whose numbers start at 1 (Q8's first behaviour);
- with Q8 answered "go on": a receiving node made again in a process that keeps its pid, linking again over a pipe, is told its predecessor's numbers at its first stimulus; and a link made by a third node for a target that was made again keeps the old numbers;
- signals in a pipe that the receiving node no longer reads because it is ending are not counted at either end; the synapse closing is the notice (G15).
Tests: Q6, Q10, Q11, Q17, X6, X7, X12, X19, E3, D3.

**G7. Fire never waits on another node's code.** `Node::fire` returns after one push and, when it helps, after at most `help_batch` (64) drain steps of this node's own output work: built-in functions, one weighing and one move per link, or one piece of a held release. One exception, bounded by what this node's own link functions held (Q1): when the line of fired signals is full while such a release is being handed over, the firing thread hands pieces over until one fired signal can be popped. `fire` never waits for a pipe, a reach, a full line, a codec, another node's code or this node's own code. The locks it may wait for: this node's queue lock (L4), this node's output lock (L2, held for one drain step), a target's line (L3), in direct delivery the receiving node's queue lock (L4), and the pool (L5). The length of each section does not depend on how many stimuli wait nor on their size: every stimulus is moved once into a line and once out, values that leave a line are moved out under the lock and dropped after it, and no thread is started under any of them. std locks lend no priority, so a holder of low priority that is preempted inside such a section delays the waiter until it runs again; the sections are kept short for that reason (5.1). Tests: X1, X2, Q13.

**G8. No deadlock from the runtime.** No thread waits on a line: a push replaces, moves a victim out or enqueues, and a take pops. Locks are taken in one order (section 5). No runtime lock is held across a node's code, except a receiver chain's own lock across its own function, which only the holder of the in-right takes. Every wait behind a node's code has a deadline or holds nothing another thread needs (5.3's table). A direct blocking request never runs a node's code on a thread that is already inside that node's code, or that called out of that node: it hops to a worker thread, as the wire would (7.11), so a re-entrant lock or thread-bound state behaves the same on the three deliveries. What node code itself can still hang, the same on all three deliveries: (1) two nodes' code holding their own locks around requests to each other; (2) new in 0.3, a consequence of decision 95 (Q10): a node's `integrate` that waits, through a request, for the same node to take in another signal. In 0.2 integrates of different synapses run side by side (dendrites.rs:86, 210-233); in 0.3 the second signal waits in its line. The report shows it (`in_call_for`), and the node's end abandons the call. Tests: X12, X1, D1, B12.

**G9. Requests never wait behind signals.** A reading task never stops reading because a line is full or a node is slow: it pushes and reads on, and it never takes a node's output lock. A writer writes control frames (requests, answers, link ops, releases) before signal frames. Tests: X11, X20.

**G10. Fair turns.** On shared handling, with R nodes ready, K servers counting and no call longer than `server_stuck`, a ready node is taken within ceil(R / K) turns. A turn makes at most `turn_stimuli` (16) takes, each giving at most one `integrate`, and lasts at most `turn_time` (5 ms) plus the call in hand. What one take's functions release beyond its first stimulus waits under the queue's lock for the next takes, inside the same budget. A server inside one call of a node's code for longer than `server_stuck` stops counting and is replaced. Tests: P1, P2, P3, P15.

**G11. Sure before moving.** By measure a node moves only when its lateness account reaches S = ceil(`sure` / `check_every`) steps (100). The account takes at most one step per `check_every`: up when the node is late at that step, down otherwise, never below zero. So no burst shorter than `sure` moves a node, and a stall of the whole process adds at most one step. A node is late only when something of its own that could be taken has waited longer than `wait`, or what could be taken passes `backlog`, while nobody serves it; or when another node has waited longer than `wait` for a server that this node's call holds (7.7). What cannot be taken yet (a synapse whose `attach` has not returned) and time inside the node's own code are not late otherwise. One node moves per step, and every other account starts again after a move. At most `autonomous_max` (16) nodes of a process hold a thread taken by measure. Tests: Q12, Q21, P5 to P13.

**G12. Autonomous.** Once a node has its thread, its stimuli wait only for its own code. No shared server runs its signal code again. It keeps the thread until it ends (decision 94). What it fires is sent by its own thread at its priority (decision 102 and NETWORK.md:492): before each take the thread sends what was fired, at most one batch (`help_batch`), and a take marks its call begun in the section that takes the stimulus, so "fired before the call" means "fired before that section". Two others may send, both said: a thread that fires helps when the line of fired signals is half full (decision 102); and while the node's thread has been inside one call for longer than `wait`, the shared servers send what waits and apply the link ops, until that call returns. So a signal fired from another thread while the node's thread is inside the node's code waits for that call for at most `wait` (Q6 asks whether it should wait at all). For a node that declared itself from its start, the receiver's functions, `integrate`, `lost`, `pulse` and `shutdown` all run on that one thread, with the exceptions of G14 and of a node stopped before its thread had claimed (G13). Tests: A1, A2, A5, A9, Q22.

**G13. The move.** Moving from shared handling to the node's own thread loses, repeats and reorders nothing: a stimulus is in its line, in the one call of the one holder, or back under the queue's lock, and one holder at a time takes. Output goes on during the move. If the thread cannot be started, or ends before it claimed, or the node begins to end before the thread has claimed, nothing has changed and the node ends as a shared one. Tests: Q14, Q18, A2, A6, A10.

**G14. The end.** From the moment a node begins to end, its dendrites admit and take in nothing more. What was queued is integrated until `take_by`, which is `end` (5 s) less min(`server_stuck`, `end` / 2) after the end began; no call of the node's signal code begins after `take_by`, and what was not integrated, and what was owed and not told, is then counted. `detach(peer)` runs before `shutdown` for every peer whose `detach` returned by `end` (NETWORK.md §4.3). `shutdown` has started by `end`: on the node's own thread if it has one and that thread takes the end in time, else on a thread liminal-net starts for the end itself. So neither a signal call that does not return nor a blocking pool filled by requests can keep it from starting; node code that `shutdown` itself waits for is the node's own affair. Once the end's steps have begun (`steps`, at the latest at `end`), the runtime starts no call of the node's signal code. Three exceptions, each said in the reason:
- (a) a signal call that began before `take_by` and still runs at `end` (it has then run for at least min(`server_stuck`, `end` / 2)) goes on beside `shutdown` on a thread that is abandoned; it starts no further call, what it fires afterwards is discarded and counted, and the reason says `beside a call still running`;
- (b) a `detach(peer)` or a request that was already running goes on beside it, as requests do; a `detach(peer)` that has not begun by `end` is not called, and is counted;
- (c) a node's own thread that is in no call and still has not taken the end by `end` (it was not scheduled between `take_by` and `end`) loses it: the end thread secures the data, and the reason says `off its own thread`.
What the links' functions held is passed on before `shutdown`, and again after it; what the node fired, also from `shutdown`, is given to its targets for at most `end` more; what is fired after that hand-over closed is discarded and counted. The reason keeps today's words, and says what was left only when something was. Tests: E1 to E10, Q23.

**G15. Three deliveries alike.** Every behaviour scenario gives the same results over the pipe (wire), through the codec in-process (encoded) and by direct call (direct): the same values, moved and never shared; the same errors and words, the 16 MB rule and the nesting rule included; the same functions, order and lifecycle; the same thread rule for a request that comes back into its caller (G8). A float crosses the pipe exactly (inferred: serde_json's `float_roundtrip`; D1's float). Three differences are said:
- a blocking direct request whose synapse closes meanwhile gets `Closed` when the callee returns, the wire at once; D1 compares results, not moments;
- a signal on its way to a receiver that begins to end: on the wire it is lost in the pipe and counted nowhere; direct and encoded keep it at the front of the sender's line, where it is delivered to a successor or counted in `ended` (D3 states each delivery's result);
- under overload the wire holds two lines and a pipe between sender and receiver, encoded two lines, direct one: how many are lost and at which end they are counted differ. D1 runs no scenario under overload.
Scenarios that depend on time run on manual processes. Tests: D1, D3.

**G16. Threads are bounded.** Server threads: at most K that count (K is min(4, cores), or less by setting: decision 100), and one more for each node stuck inside one call. One watch thread per process. Own threads: at most `autonomous_max` taken by measure (16, or less by setting: decision 99), and one for each node that declared itself or asked (Q2). One end thread per node that is ending; it returns as soon as the end's steps are another thread's, or done. No server and no watch thread runs while nothing waits: an idle server ends after `server_idle`, and the watch ends when nothing is armed. An own thread stays, asleep, until its node ends (decision 94). Every thread of liminal-net's own carries a guard for its own failure (section 4). Tests: P3, P10, P14, Q19, Q20, A7.

**G17. The wire and the bridge.** Protocol 1, no new op, tests/vectors.rs as it is. Python node code never runs on a tokio async thread, and every call from Python into the network lets go of the interpreter's lock; both have a test that fails when the rule is broken, in a build that turns the checks on (the `rule-checks` feature), and the bridge's rule tests refuse to pass without that build. Tests: the 48 and the 12 as they are; B0, B4, B9, B10, D2.

## 3. The data structures

New modules: `queue.rs` (pure: `Line`, `QueueState`, the measure's step, the watch's alarm and the idle servers' count), `process.rs` (`Process`: the table, the servers, the watch, the clock), `autonomy.rs` (`Autonomy`, `Priority`, the own thread, the end thread, the priority calls), `local.rs` (admission without a pipe, the codec of the encoded mode), `rules.rs` (the two checks of G17, compiled in debug builds and with the `rule-checks` feature).

### 3.1 `Limits` and `Delivery` (lifecycle.rs)

`pub struct Limits`: the fields, first values, floors, ceilings and environment variables are the table of section 8. `Limits::from_env()` is `Limits::from_lookup(|name| std::env::var(name).ok())`. `from_lookup` reads each variable by one rule, stated in full:
- the text is trimmed;
- a time is a number of seconds that is finite, from 0 and at most a year (366 days), turned with `Duration::try_from_secs_f64`;
- a count is a whole number written in digits that fits `usize`;
- anything else (empty, negative, a fraction for a count, `1e300`) is the default.
`Timing::from_env` reads LIMINAL_QUIET_SECONDS with `Duration::from_secs_f64`, which panics on a huge value (lifecycle.rs:42-44; fault 22); it is given the same rule. Then `Limits::clamped()` raises every value to its floor and lowers it to its ceiling. `Process::new` clamps what a test gives, and `Process::limits()` shows the values in use. `Timing` keeps its six fields, so `quick()` (tests/common/mod.rs:41-50) and `timing_from` (bridge:430-450) stand.

`pub enum Delivery { Direct, Encoded, Wire }`. Direct: nodes of one process deliver without encoding. Encoded: the same path, every op through the frame codec (decision 42's test mode). Wire: the process keeps no table, and every node is reached over its pipe (what 0.2 does). `Delivery::from_env()` is `Delivery::from_lookup(..)` on LIMINAL_NET_DELIVERY (`direct`, `encoded`, `wire`, trimmed, any case; anything else is `direct`).

### 3.2 `Process` (process.rs)

`#[derive(Clone)] pub struct Process { inner: Arc<Shared> }`. One per operating-system process by default (`Process::global()`); a private one per test. It is a field of `NodeConfig`.

| `Shared` field | type | what it is for |
|---|---|---|
| `delivery` | `Delivery` | how nodes of this process reach each other |
| `limits` | `Limits` | every number of section 8, clamped |
| `clock` | `Clock` | `Real`, or `Manual { offset: Mutex<Duration> }`. `now()` is `tokio::time::Instant` (functions.rs:15 takes that type): the real time, or a fixed start plus `offset` |
| `manual` | `bool` | a manual process starts no server and no watch thread; the test runs turns and passes itself. A node's own thread and its end thread are real threads there too |
| `table` | `Mutex<HashMap<String, Entry>>` | the nodes of this process by address (address.rs:21-34). `Entry { node: Weak<Inner>, instance: u64 }`. A node enters before it binds, and only over nothing, a dead entry or an Ended node's; it removes only an entry that carries its own instance (T1, N7). A wire process keeps it empty |
| `pool` | `Mutex<Pool>` | lock L5 |
| `work` | `Condvar` | idle servers wait here |
| `alarm` | `Condvar` | the watch waits here |
| `ticked` | `tokio::sync::Notify` | wakes async waits with a deadline when a manual clock is advanced |
| `instances` | `AtomicU64` | numbers every `Node` made in this process, from 1 |

| `Pool` field | type | what it is for |
|---|---|---|
| `ready` | `VecDeque<Weak<Inner>>` | nodes with work for a server, each at most once (`QueueState.queued`) |
| `servers` | `Vec<Server>` | one slot per live server thread |
| `counting` | `usize` | servers that count towards K: alive (or being started) and not marked stuck |
| `idle` | `Idle { idle: usize, claims: usize }` | `idle`: servers waiting on `work` that no notifier has claimed; `claims`: wake-ups given and not yet taken (7.3) |
| `next_server` | `u32` | numbers the threads |
| `alarm` | `Alarm { wake_at: Option<Instant>, watching: bool }` | when the watch must next pass, and whether a watch thread lives (7.7) |
| `last_step` | `Option<Instant>` | when the measure last took a step |
| `measured` | `usize` | nodes that hold a thread taken by measure |
| `replaced` | `u64` | how often a stuck server was replaced |

`Idle` and `Alarm` are pure types of queue.rs, so their rules are tested with no thread (Q19, Q20).

`Server { id: u32, call_since: Arc<AtomicU64>, stuck: Arc<AtomicBool> }`. `call_since` is 0 when the server is in no call, else the time in nanoseconds of the process's clock since which it has been inside one call, plus one (a manual clock's first instant is 0).

`pub struct Check { pub made_ready: usize, pub replaced: usize, pub stepped: bool, pub moved: Option<String>, pub refused: Option<String> }`: what one pass of the watch did.

### 3.3 `Line` (queue.rs)

One type for every run of entries: `Line<P>`, with `P = Arrived` for one synapse's part of a source's line and `P = OutItem` for a target's line.

| field | type | what it is for |
|---|---|---|
| `streams` | `VecDeque<Entry<P>>` | stream entries, in ticket order |
| `lanes` | `HashMap<String, Lane<P>>` | per signal: its counts and its two state places. A lane leaves when it holds nothing |
| `order` | `BTreeMap<u64, String>` | the tickets of the state places that are filled, so a pop looks at two tickets however many signals the line holds |
| `next_ticket` | `u64` | the arrival counter |
| `count`, `bytes` | `usize` | stream entries waiting, and their weights |
| `state_bytes` | `usize` | the weights of the state values waiting (outside the caps: G5) |
| `high_count`, `high_bytes` | `usize` | high-water marks, for the report |

`Entry<P> { ticket: u64, signal: String, at: Instant, weight: usize, payload: P }`. `Lane<P> { kind: SignalKind, count: usize, bytes: usize, oldest: Option<Entry<P>>, latest: Option<Entry<P>> }`.

- `pop()` gives the entry with the smallest ticket: the front of `streams`, or the first place in `order`. When a lane's `oldest` is taken, its `latest` becomes its `oldest`. A replaced latest value gets a new ticket: it goes to the back.
- `unpop(entry)` gives an entry back after a write that failed (7.5): a stream entry returns to the front with its ticket; a state value returns to its lane's `oldest` unless a newer value of that signal waits, which then stands for it (`coalesced`).
- `victim()` is the one function behind (Q4): first behaviour, the front of `streams` (the line's oldest stream item, decision 97; constant time).
- A push, a replace and a victim never drop a value under the line's lock: what leaves is handed back to the caller, which drops it after its locks (7.1).

`SourceLine { parts: VecDeque<Part>, released: VecDeque<Run>, released_count: usize, count, bytes }`: one source's arriving line.
- `Part { syn: Arc<SynIn>, line: Line<Arrived>, closing: bool, flushed: bool }`, one per synapse of that source, in the order admitted. A push goes to its synapse's part. A pop comes from the front part only; when the front part is empty and `closing`, the pop gives its closing marker (7.3).
- `Run { stimuli: std::vec::IntoIter<Stimulus>, syn: Arc<SynIn>, at: Instant }`: what one use of a receiver's functions gave back and has not been integrated yet. A run is put in whole, in constant time, and taken one stimulus at a time, before anything else of that source.
- The caps are over the source's line as a whole; the victim is taken from the front-most part that holds a stream entry. `released` is not capped and has no victim: it is at most what the functions held or gave (G5; Q1).

`Arrived { stimulus: Stimulus, syn: Arc<SynIn> }`. `OutItem { seq: u64, data: Value }`.

### 3.4 `Queue` and `QueueState` (queue.rs)

`pub(crate) struct Queue { state: Mutex<QueueState>, wake: Condvar, right: Condvar }`: lock L4, in `Inner`. `wake` has one waiter, the node's own thread. `right` is waited on only by the node's end thread.

| `QueueState` field | type | what it is for |
|---|---|---|
| `mode` | `Mode` | `Unserved`, `Starting`, `Shared`, `Moving`, `Autonomous`, `Ending`, `Ended` (section 6) |
| `thread` | `Option<OwnThread>` | the node's own thread, once asked for |
| `in_holder` | `Option<Holder>` | who holds the in-right: `Server(u32)`, `Own` or `Ender` |
| `holder_thread` | `Option<ThreadId>` | that holder's thread |
| `in_call` | `Option<Instant>` | since when the holder has been inside one call of the node's signal code |
| `in_hand` | `u64` | entries the holder took from a line and has not yet put through their functions (0 or 1): counted by whoever takes the right from it |
| `taking` | `bool` | the node's own thread is waiting for the in-right: servers take it no more |
| `queued` | `bool` | the node is in `Pool.ready` |
| `retired` | `bool` | a replaced receiver chain still holds something for its successor (7.10) |
| `end_by` | `Option<Instant>` | set at N1: the end's bound. `take_by` is `end_by` less min(`server_stuck`, `end` / 2) |
| `steps` | `bool` | the end's steps N4 on have begun: nobody takes the in-right again, and no signal call begins |
| `abandoned` | `Option<ThreadId>` | the thread whose call was still running at the bound |
| `leave` | `bool` | the own thread must return (set at N7, or by a guard) |
| `detaching` | `usize` | closed synapses whose `detach(peer)` has neither returned nor been skipped |
| `sources` | `HashMap<Source, SourceLine>` | one arriving line per source; `Source = (String, String)`, the peer's name and scope |
| `ring` | `VecDeque<Source>` | the sources that can be taken from now, in turn order: one with something in `released`, or whose front part is attached and holds an entry or its closing marker |
| `synapses` | `HashMap<u64, Arc<SynIn>>` | the incoming synapses, by id, from their registration |
| `links_in` | `HashMap<(Source, String), LinkIn>` | per source and signal: the last number taken and the counts |
| `in_count`, `in_bytes` | `usize` | stream stimuli waiting in all arriving lines |
| `fired` | `VecDeque<Fired>` | the line of fired signals, before the functions, with the link ops in their places |
| `fired_bytes` | `usize` | its weights |
| `out_waiting` | `usize` | links that hold `carry` or `passed` (3.5): output work as real as a fired signal |
| `out_closed` | `bool` | set at N6: from here `fire` discards and counts |
| `pre` | `HashMap<String, Value>` | before `serve`: the latest fired value of each state signal |
| `helped`, `helped_seen` | `u64` | signals a firing thread sent because the line was half full; the value at the last step |
| `in_due`, `out_due`, `pulse_due` | `Option<Instant>` | the earliest time of the receiver chains, of the links' chains, of the pulse |
| `turn` | `u64` | counts turns and passes |
| `account` | `u32` | the lateness account, in steps |
| `late_since` | `Option<Instant>` | when the account last left zero |
| `note` | `Option<String>` | 'needs a thread: 16 of 16 taken by measure', 'could not start a thread: ...' |
| `waits`, `handling` | `[u64; 12]` | histograms of waits at the take and of handling times |
| `longest_wait` | `Duration` | the longest wait at a take since the previous report |
| `failed` | `u64` | calls of the node's code that panicked |
| `left` | `Left` | at the end: `not_integrated`, `not_told`, `not_sent`, `not_detached`, `chains_unread: u64`; `beside`, `off_thread: bool` |
| `fired_after` | `u64` | signals fired after the node's output closed, or by the abandoned thread |

`OwnThread { why: Why, asked: Priority, given: Priority, priority_note: Option<String>, claimed: bool, cancelled: bool, place: bool, gone: bool, since: Option<SystemTime>, busy: Duration }`. `Why` is `Declared`, `Measured` or `Asked`. `place`: it holds one of the measured places. `gone`: it ended without being told to leave.

**`QueueState::server()`** says who serves the node: `Own` when `thread` is claimed, else `Pool`. Every rule that wakes somebody asks this, never the mode, so the rules hold while the node is Ending.

**`QueueState::others_send(now)`** is the one function behind (Q6): first behaviour, true when `server()` is `Pool`, and for a node with its own thread while `in_call` is set and older than `wait`.

`SynIn { id: u64, peer: Peer, source: Source, origin: (u32, u64), local: bool, counted: AtomicBool, attached: AtomicBool, closed: AtomicBool, detach: AtomicU8, stop: tokio::sync::Notify, done: tokio::sync::Notify }`: one incoming synapse. `origin` is the peer's pid from its hello and, for a local synapse, the sending node's instance number (0 over a pipe). `detach` is the state of its `detach(peer)` call: not owed, pending, running, skipped or returned, changed by compare-and-swap (7.13). `stop` and `done` are told with `notify_one`, which keeps a permit for a waiter that comes later (inferred: tokio's Notify; E5, E1), and each is paired with a flag read first (`closed`, and the part's removal).

`LinkIn { origin: (u32, u64), last_seq: u64, owed: u64, told_turn: u64, taken: u64, coalesced: u64, told: u64, dropped_here: u64, repeated: u64 }`. `last_seq` starts at 0 and is kept per source.

`Fired` is `Signal { signal: String, data: Value, at: Instant, size: Size }`, or `Op(LinkOp)`: a `link`, `unlink` or `tune` with its chain already built, the synapse it came on, that synapse's origin, and where to answer (7.10). `Size { likely: usize, upper: usize, depth: usize }` is taken in `fire`, off every lock (9.1).

### 3.5 The output side (node.rs, axon.rs)

`Inner.axon: Mutex<Axon>` is lock L2, the output right. It replaces `links` and `last_state` (node.rs:94-95).

`Axon { links: Vec<LinkOut>, last_state: HashMap<String, Value>, branches: HashMap<Target, Arc<Branch>>, seqs: HashMap<(String, Target), Numbered>, closed: bool }`. `Numbered { last: u64, holder: (u32, u64) }` keeps, for a signal and target, the last number given and the origin of the synapse that last held the link; it is read only with Q8 answered "go on". `closed` is set at N6's bound.

`LinkOut` keeps `signal`, `to`, `chain`, `made_on`, `seq` (node.rs:73-80) and gains:
- `carry: VecDeque<Stimulus>`: what a replaced chain held, still to pass through the new one;
- `passed: VecDeque<Stimulus>`: what the chain gave back beyond one piece, still to be numbered and handed over;
- `counts: Arc<LinkCounts>` (`sent`, `dropped_here`, `too_large`: atomics, so no lock is needed to count).

`Branch { target: Target, line: Mutex<BranchLine>, ended: AtomicU64 }`: one per target, made with the first link to it; `ended` counts what was dropped when a link or the branch ended. `BranchLine { way: Way, line: Line<OutItem>, closing: bool }` is lock L3. `Way` is `Unreached`, `Reaching`, `Open(Remote)` or `Closed` (the node's end: no reacher starts).

`Inner` also gains: `queue: Queue`; `instance: u64`; `kinds: Mutex<HashMap<(String, String), SignalKind>>`; `runtime: OnceLock<tokio::runtime::Handle>` (captured by the first of `serve`, `reach`, `call`, `release`); `reaching: Mutex<HashMap<Target, Arc<tokio::sync::Mutex<()>>>>`; `total_count`, `total_bytes: AtomicUsize` (what waits in all its lines, for the node's ceiling) and a gauge per line; `autonomy: OnceLock<Autonomy>`; `returned` (a flag and a notify: the own thread's function has returned). It loses `firing` and `firing_inbox` (node.rs:105-106).

`Inner.receivers` becomes `Mutex<Receivers>`: `Receivers { now: HashMap<(String, String), Arc<Slot>>, retired: Vec<((String, String), Arc<Slot>)> }`. `Slot { chain: Mutex<Held>, fed_by: Mutex<Option<Arc<SynIn>>> }`, with `Held { chain: Chain, seen: Option<Instant> }`: the chain and the latest time it was given, so it never sees time go back. `Slot.chain` is lock L1.

A thread-local `running` lists the nodes whose code the thread is inside, and how many direct calls deep it is (7.11).

### 3.6 `Remote` (synapse.rs)

`Out` gains `kind: Kind`, `in_flight: AtomicUsize` and `carries: Mutex<Option<Arc<Branch>>>`. `Kind` is `Pipe { tx: mpsc::UnboundedSender<Vec<u8>>, signals: Notify }` or `Local { node: Weak<Inner>, syn: Arc<SynIn> }`. The control channel carries encoded frames. `held()` (synapse.rs:72-76) also holds while `in_flight` is above zero.

### 3.7 `Autonomy` and `Priority` (autonomy.rs)

`pub struct Autonomy { pub from_start: bool, pub priority: Priority }`, default `false` and `Normal`. `pub enum Priority { Low, Normal, High }`.

## 4. The threads and tasks

Threads of liminal-net's own are plain operating-system threads. None is a tokio thread.

| thread | name | started by | ends | may | may not |
|---|---|---|---|---|---|
| server | `liminal-net-server-<n>` | a push or the watch that finds a node ready, no idle server to claim and fewer than K counting. The decision is taken under L5 (the slot is counted then); the thread is spawned after every lock of the deciding thread is let go (in direct delivery after the sender's L3 and L2 too); a refused spawn takes the slot back | when `ready` is empty for `server_idle` (decided under L5 after a last look); a stuck one when its call returns and K already count | run a node's turn: its output, and with the in-right its receiver's functions, `integrate`, `lost`, `pulse`; enter that node's runtime handle for the turn | serve the output of a node whose own thread has claimed, unless that thread has been inside one call for longer than `wait` (Q6); wait for a line, a pipe or a reach; hold L2 to L5 across a node's code |
| watch | `liminal-net-watch` | the first thing to arm it | when nothing is armed (decided under L5, in the section that ends its pass) | read the clock; make nodes ready at their due times; mark a stuck server and start another; take the measure's step; start a move | call a node's code; run a chain; hold L5 while it takes L4 |
| own | `liminal-node-<name>` | `serve` for a declared node; the watch (measured); `take_thread` (asked) | when `leave` is set, or its claim was cancelled | everything a server may, for its one node, at the node's priority; sleep on `Queue.wake` without tokio; the end's steps and `shutdown` | serve another node; sleep while it holds the in-right and something waits |
| end | `liminal-node-<name>-end` | `serve`, at N1, for every node | when the end's steps are done, or as soon as another thread has them | wait on `Queue.right`; take the in-right as `Ender`; run N4 to N6 for a node the pool serves, or for any node at the bound | be a thread of a pool that node code can fill; call the node's signal code |
| worker | tokio's blocking pool | liminal-net, as today (dendrites.rs:44, 111, 206) | by tokio | requests, lent callables, `admits`, `attach`, `detach`, `autonomy`, a direct request in its async form or after a hop | take a node's in-right; run the end |
| caller | any thread that calls `fire`, a blocking form, or a bridge method | the node's code or the app | — | push; help with this node's output (G7); run another node's `request` in a direct blocking call | run another node's signal code |

If the system refuses the end thread, the end's steps run on a worker, as today (node.rs:252), and the report's note says so.

**Guards.** A failure of node code is caught by `node_code` (5.3). For a failure of the runtime itself, or a thread function that returns early, each thread of liminal-net's own holds a drop guard:
- a server: its slot leaves `servers`, `counting` goes down if it counted, an in-right it held is given back, and that node is made ready;
- the watch: `watching` becomes false, so the next arm starts one;
- an own thread that ends without `leave` before it claimed (its `own_thread` returned without calling `run`, or failed): its move is undone as a refused start is (T3 step 2: the node is Shared again, `taking` is cleared, a measured place is given back, the note says `its thread ended before it claimed`), and the node is made ready; for a declared node `serve` returns with those words (T2);
- an own thread that ends without `leave` after it claimed: `gone` is set, its in-right is given up, and the node is stopped with the reason `its own thread ended`; the end thread then takes the steps at once (N3) and counts what was queued;
- the end thread: its steps pass to a worker, as when the system refuses it.

Tasks on tokio's async threads:

| task | started by | ends | does | never |
|---|---|---|---|---|
| `serve` | the node's owner | the node's end | accepts connections, decides the end, drives its last steps | calls a node's code |
| reading task, one per incoming pipe synapse | `serve`, per connection (node.rs:242) | the peer's detach, a broken pipe, `SynIn.stop` | handshake and admission; pushes each `fire` into the queue; starts each request; builds each link op's chain and pushes the op into the line of fired signals | waits on a line or on a node's code; takes L2 |
| answers' writer, one per incoming pipe synapse | the reading task | its channel closes (the last answer of a request in flight was written); ended at N1 | writes encoded bytes | encodes |
| synapse-end task | the reading task at its end; a local synapse's close | after `detach(peer)` | marks the part closing; closes a local Remote at its reaching end; does the synapse's bookkeeping at its bound; calls `detach` on a worker after the marker | drops or counts a stimulus; runs on the thread that called `Remote::detach` |
| a pipe `Remote`'s writer, reader, quiet watcher | `Remote::start` (synapse.rs:98, 108, 122) | the synapse closes; the writer also when its way is no longer its own (7.5) | the writer writes control frames first, then encodes and writes its branch's signals | pops a signal after its way was taken from it |
| a local way's writer, in encoded delivery only | the way opening | the way closes | pops its branch's line, passes each `fire` through the codec, calls the receiver's `Queue::arrive` | holds L2 or L3 while it encodes |
| reacher, one per branch | whoever sets the way to `Unreached` with something in its line and a link left, or pushes into an `Unreached` way, in that same L3 section | decided under L3: the way opens, or nothing waits, or no link remains, or the branch is closed | makes one attempt at a time to reach the target without a launcher, backing off from 50 ms to 1 s between attempts | starts a process: a signal never starts one; waits inside an attempt for a retry |

The per-synapse integrate task (dendrites.rs:86, 210-233) and `fire_loop` (node.rs:543-559) are gone.

## 5. The locks and the rights

### 5.1 The locks, in the one order they may be taken

| | lock | where | held for | never held across |
|---|---|---|---|---|
| L1 | a receiver chain | `Slot.chain` | one use of the chain; it is the only lock held across a node's code (the chain's own function), and only the holder of the in-right takes it | — |
| L2 | the output right | `Inner.axon` | one drain step: one fired signal's work (its links' built-in functions, one copy and one weighing per link, at most `help_batch` moves per link into the targets' lines), or one piece of `carry` or `passed`, or one link op | a node's code, encoding, a write, a reach, an `.await` |
| L3 | a target's line | `Branch.line` | one push, one pop, or one piece of the move of what waited when a direct way opens | the same |
| L4 | a node's queue | `Queue.state` | one push, one pop, one change of state. Its length does not depend on how many stimuli wait nor on their size: no value is dropped under it, a run is put in whole, and the lines are swapped out whole at the end | the same, and L1, L2, L3 |
| L5 | the pool | `Shared.pool` | one change of the ready list, of a server's slot or of the alarm: constant time; no thread is spawned under it | the same, and L1 to L4 |

A thread may take a later lock while it holds an earlier one, never the reverse. L3 and L4 of another node are taken under L2 when a signal is handed to a target of the same process in direct delivery; a node's own L4 is taken under its L2 to pop a fired signal. The places where the order is easy to get wrong, each fixed:
- `fire` on an unserved node touches L4 only: the latest of a state signal is kept in `QueueState.pre`. T1's last step and a declared thread's claim (T2) take L2, then L4, and move `pre` into `last_state` in the section that makes the node served, so no value is stored after the first link took "the latest".
- `Node::report()` takes L2, each L3, L4 and L5 one at a time, in that order, copies what it needs and lets go before the next; the map is built from the copies.
- Per-link output counts are atomics (`LinkCounts`), so the writer and the reacher count holding no lock; the reacher reads "does a link remain" from a count kept on the branch.
- In encoded delivery nothing is encoded under L2 or L3: the way is treated as a pipe whose writer is the in-process task of section 4.
- The node's ceiling is kept by a trim after the push (7.6), with one line's lock at a time.
- What a push moved out, and a server it decided to start, are dealt with after the pushing thread's last lock: in direct delivery that is after the sender's L3 and L2.
- The watch copies what it needs under L5, lets go, then takes each node's L4. A move, and giving a measured place back, take L4, then L5 inside it.

Leaves, taken alone and held for a few instructions: the `receivers` map, `kinds`, `latest`, `remotes`, the handles' table, `stage`, `stop`, the process's `table`, a `Remote`'s `pending` and `last_used`. No std lock is held across an `.await`. `reaching[target]` is a tokio mutex, held across one attempt to reach.

A lock taken around a node's code is never left poisoned: the call is caught inside the guard's scope.

### 5.2 The rights

- **The in-right** is `QueueState.in_holder`, a flag under L4. Its holder is the only thread that calls the node's signal code. A server takes it for one turn; the own thread takes it once and keeps it; the `Ender` takes it for the end and never gives it back. A holder gives back only its own hold: it clears `in_holder` only if it still names it.
- **`taking`**, under L4: the own thread is waiting for the in-right. A server that sees it gives the right back before its next take, and no server takes it again.
- **The out-right** is L2. Whoever holds it sends: a server in its turn (for a node the pool serves, or one whose thread has been inside one call for longer than `wait`), the own thread, a thread that fires and helps, the runner of the end's steps. It is held per drain step. A reading task never takes it.

### 5.3 The waits, and the rule

| wait | who | bounded by |
|---|---|---|
| `block`: one `Handle::block_on` in a blocking form | the calling thread | the callee, as on the wire (decision 47) |
| `Queue.wake` | the own thread, idle or waiting for the in-right | the next due time, and `take_by` once the node is Ending; it holds nothing while it waits |
| `Queue.wake` or `Queue.right`, for `detaching` to reach zero | the runner of the end's steps (N4) | `end_by` |
| `Queue.right` | the end thread | `take_by`, then `end_by`; it returns when `steps` is another thread's |
| `Shared.work` | an idle server | `server_idle` |
| `Shared.alarm` | the watch | `wake_at` |
| a synapse-end task's wait for `done` | that task | `end` for its bookkeeping; then without a bound for `detach(peer)`, holding nothing |
| `serve`'s wait for a declared thread's claim (T2) | `serve` | `stop`, and `start_wait` |
| `serve`'s waits during the end | `serve` | none for `shutdown` (the node's own affair, as today: node.rs:253-261); `end` after it returned; 1 s for the thread |
| a reach's wait for `reaching[target]` | the reach | the reach's own deadline, read before it asks |
| local admission's wait for `admits` | the reach | `hello_wait` |
| a local request's wait for `attach` to return | the caller | the synapse closing (N1, 7.13); else the callee, as a frame waits unread on the wire |
| a link op's wait for its answer | the caller | whoever serves the reached node's output, as on the wire |
| `Node::join_thread` | the bridge's exit hook | none: it holds nothing, and it skips an abandoned thread |

Every wait with a deadline reads the process's clock. An async wait enables its `Notified` future on `ticked` before it reads the clock; a condvar wait re-reads the clock under its lock; `Process::advance` takes each node's L4 and then L5 around its notifies. So an `advance` between a waiter's look at the clock and its wait is never lost (inferred for `Notified::enable`; E3).

**No node code is ever called inside a runtime wait.** Every call of a node's code goes through one wrapper, `node_code`, which:
1. in a build with the checks on (debug, or `rule-checks`) asserts that the thread is in none of the waits above (a thread-local counter raised around each), and that it is not one of tokio's async threads: `rules::on_async_thread` makes one empty `block` on the node's handle inside `catch_unwind`, which panics there (inferred; D2, B10);
2. for signal code (the receiver's functions with their ticks and flushes, `integrate`, `lost`, `pulse`), in one L4 section: checks that this thread holds the in-right and is not the abandoned one, that `steps` is false and, for an Ending node, that `take_by` has not come. If the check fails it calls nothing and tells its caller to leave the turn. Else, in that same section, it takes what the call is for (a released stimulus, an entry, a count that is owed, a chain that is due) and sets `in_call` and the server's `call_since`;
3. notes the node on the thread's `running` list and runs the call inside `catch_unwind`;
4. afterwards, in one L4 section, clears `in_call` and stores what the call gave (a run for `released`). A thread that lost the right meanwhile stores nothing: what it had in hand was counted by the thread that took the right (7.12).
A blocking form never runs a callee on the calling thread from inside its `block_on`: `call_blocking` is `block(self.call(..))` today (node.rs:343-345), and in 0.3 it reaches first, lets `block_on` return, and only then calls a local owner's function. `block` takes the node's captured handle instead of `Handle::current()` (synapse.rs:306-308).

## 6. The states of a node

`Stage` (lifecycle.rs:51-57) stays what peers see: `Running`, `Securing`, `Ended`. `Mode` says how the queue is served.

| mode | stage | who serves the queue | a stimulus that arrives |
|---|---|---|---|
| Unserved | Running | nobody; the node is not listening and not in the table | cannot arrive. `fire` keeps the latest of a state signal in `pre` and sends nothing |
| Starting | Running | nobody yet; the node is in the table, not accepting. A declared node's thread is being started and claims from here | cannot arrive: a local reach gets retry. `fire` as for Unserved |
| Shared | Running | the shared servers | enters its source's line; the node is made ready |
| Moving | Running | the servers, until the new thread sets `taking`; then nobody until it claims | enters its line and stays there; it is taken in order after the claim. Output goes on throughout |
| Autonomous | Running | the node's own thread | enters its line; the thread is notified |
| Ending | Securing | whoever served it until the end's steps begin, then nobody: their runner calls no signal code | cannot arrive: `Queue::arrive` answers `closed` |
| Ended | Ended | nobody | cannot arrive. `fire` has been discarded and counted since N6 |

A node is served once at a time: `serve` on a node that is not Unserved returns at once with `<name> was already served`. Today a second `serve` refuses every synapse, because the stage is never set back (node.rs:263, dendrites.rs:26). A start that failed (T1 step 2, T2 step 5) leaves the node Unserved.

**T1. Unserved to Shared** (`serve`, a node that did not declare itself).
1. Capture the runtime handle. Under L4: `mode = Starting`. Read `hillock.autonomy()` once, on a worker thread, and keep it; `stop` and `start_wait` bound the wait.
2. Unless the delivery is wire: enter the process's table, under its lock. If an entry is there whose node is alive and not Ended, another node of this process answers at the address: the mode goes back to Unserved and `serve` returns `another node answers for <name> (another node answers at <address>)`, the words the bind gives (node.rs:211; transport.rs:111), and that entry is left as it is. Else the entry becomes this node's, with its instance. Then bind the listener (node.rs:209-212); on failure remove the entry if it is this node's, set the mode back to Unserved and return the reason. A local reach that finds an entry whose node is not Shared, Moving or Autonomous gets retry, as from a node securing its data (axon.rs:133-137).
3. Take L2, then L4: move `pre` into `last_state`; `mode = Shared`. Enter the accept loop.

**T2. Unserved to Autonomous** (`serve`, a node that declared `from_start`).
1. Steps 1 and 2 of T1.
2. Under L4: `thread = Some(OwnThread { why: Declared, .. })`. Start the thread (7.8). If the system refuses it: step 5, with the system's words.
3. The thread's claim, its first act inside `run`: it takes L2, then L4, and in that one section: if `cancelled` is set or the mode is not Starting, it returns, having changed nothing. Else it moves `pre` into `last_state`, sets `in_holder = Own`, `mode = Autonomous` and `claimed`, and wakes `serve`. So `fire` never sees a served mode with `pre` unmoved, and no shared server ever serves this node.
4. `serve` waits for the claim, a `stop`, or `start_wait`, and decides under L4:
   - claimed: it enters the accept loop. A `stop` asked meanwhile is found by the loop and leads to N1, the thread being the node's own;
   - not claimed, and `stop` was asked: `cancelled` is set, so the claim fails; `thread = None`; `serve` goes on as step 3 of T1 and straight to N1: the node ends as a shared one, and its data is secured as for any stop;
   - not claimed, and `start_wait` has passed: step 5, with `no claim within <start_wait>`. A thread whose guard reported it ended: step 5, with `its thread ended before it claimed`.
5. Failure. Under L4: `cancelled` is set, `thread = None`, `mode = Unserved`, `leave` is cleared. `serve` removes its entry from the table, drops the listener and returns `<name> could not start its thread (<error>)`: nothing ran, so nothing is secured.

**T3. Shared to Moving** (measured or asked), one L4 section:
1. Only from Shared. For a measured move, under L5 inside it: if `measured` is at `autonomous_max`, set the note and stop; else `measured += 1` and `place` is set. Then `mode = Moving`, `thread = Some(..)` with its reason.
2. After L4 is let go: start the thread. If the system refuses: under L4, only if the mode is still Moving and the thread is this one, `mode = Shared`, `thread = None`, the place given back (7.8), the note set.
`take_thread` on a node that is Moving by measure makes the move its own: `why = Asked`, and the measured place is given back (Q2), so the limit cannot undo it. It answers true only for a move that stands.

**T4. Moving to Autonomous** (the new thread, inside `run`).
1. Under L4: if the mode is not Moving, or `cancelled` is set, return: nothing was changed. Else `taking = true`.
2. While a server holds the in-right, the new thread serves the node's output and waits on `wake`. The server gives the right back before its next take and notifies `wake`.
3. Under L4, when `in_holder` is none and the mode is still Moving: `in_holder = Own`, `mode = Autonomous`, `taking = false`, `thread.claimed = true`. If the mode is no longer Moving: `taking = false`, return.
A call that never returns leaves the node Moving: its report says `moving to its own thread`, its output is served, and its end handles the call (G14).

**T5. Any served mode to Ending**: 7.12, step N1. A node that is Moving has its move cancelled there, in the same L4 section: `cancelled = true`, `taking = false`, its place given back if it holds one, `wake` notified. It ends as a shared node. A claim never changes Ending. **T6. Ending to Ended**: step N7.

## 7. The algorithms

### 7.1 A signal arriving over a pipe

The reading task of synapse `syn` at node B:
1. `frame::read_sized` gives the op and its frame's length `w`.
2. For `fire { signal, from, seq, data }`: the kind is `kinds[(from, signal)]`, else stream.
3. `Queue::arrive(syn, stimulus, kind, w)`, under B's L4, answers `taken` or `closed`, and hands back what it moved out of the line and whom to wake:
   a. Read `now`. If the synapse is closed or the mode is Ending or Ended: `closed`.
   b. Take the synapse's part of the source's line. A state stimulus goes to its lane: into `oldest` if that is empty, else into `latest`; a value it replaces there is moved out (`coalesced += 1`). A stream stimulus: while the source's line is not empty and holds `line_stimuli` entries, or its bytes plus `w` pass `line_bytes`, a victim is moved out (7.6); then it is pushed. An empty line always takes it. Every stimulus is moved out at most once in its life, so the work a push does for victims is paid for by their own pushes.
   c. Give it the next ticket. Put the source in the ring if it is not there, this part is its front part, and the part is attached.
   d. Wake: if `server()` is `Own`, or the node is Moving with `taking`: notify `wake`. Else, if not `queued` and no server holds the in-right: `queued = true`, and under L5 push the node to `ready`, claim an idle server (`idle -= 1`, `claims += 1`, notify `work`) or decide to start one, and arm the watch for `now + wait`. While a server holds the in-right nothing is queued for input: that holder's step 6 requeues the node.
4. After L4, and in direct delivery after the sender's L3 and L2 too (7.2): what was moved out is dropped; the server is started if one was decided; the node's ceiling is trimmed if it was passed (7.6). The reading task reads on at once. A `closed` answer ends nothing: the frames are read and dropped until the task is told to stop.

### 7.2 A signal delivered in one process

It is the last part of a drain step (7.5). In direct delivery the thread that holds the sender's L2 and the target line's L3 calls the receiving node's `Queue::arrive` itself; the stimulus is moved and its weight is the estimate. What `arrive` hands back (values moved out, a server to start, a trim to run) is dealt with after that thread has let go of L3 and L2. In encoded delivery the stimulus goes into the target's line like a pipe's, and the way's writer task passes the `fire` op through the codec (encode, the size rule, `serde_json::from_slice`, `Op::from_value`) and calls `arrive` with the frame's length. The receiver's server is woken; the stimulus is never integrated on the sender's thread. If `arrive` answers `closed`, the stimulus is put at the front of the sender's line, and the close rule of 7.5 applies in that same L3 section: the way becomes `Unreached` and, since the line holds something, a reacher is started. The reacher then finds no serving node and, with no launcher, tries again while a link remains: the signal waits for the node's successor, bounded by the line's caps.

### 7.3 A turn of a shared server

**The idle servers.** A server that finds `ready` empty adds itself to `idle` and waits on `work` for `server_idle`. A notifier that queues a node takes one: `idle -= 1`, `claims += 1`, notify. A server that wakes, under L5: if `claims` is above zero it takes one and goes to work, whichever server the notify meant; else, if its time has run out, it takes itself off (`idle -= 1`), looks at `ready` once more and ends if it is empty; else the wake was spurious and it waits on, counted as before. So `idle` and `claims` together are always the servers inside the wait, `idle` never goes below zero, and a claim is never left to a server that is not there (Q20).

A server pops node N from `ready` under L5, lets L5 go, and then:
1. Under L4: `queued = false`. Unserved, Starting or Ended: return. `in_ok`: `server()` is `Pool`, `in_holder` is none, `taking` is false and `steps` is false; then take the in-right. `out_ok` is `others_send(now)` (Q6).
2. Enter N's runtime handle. Note the start time.
3. Output, if `out_ok`: drain steps (7.5) until nothing waits (`fired` empty and `out_waiting` zero) or `turn_time` is used. Only if the drain reached that end, and `out_due` has come: tick the links' chains under L2 with the time now and hand over what they release. A drain that was cut ticks nothing: every waiting signal first brings the chains up to its own time (7.5).
4. Input, with the in-right. Every call below is `node_code`; when it refuses, nothing more of this turn's input is called.
   a. If `retired` is set: for each retired slot, flush the old chain, pass what it held through its successor, and put what passes in `released` of the source that last fed it. `retired` is cleared under L4 when no retired slot is left.
   b. For each receiver chain that is due: tick it with the earlier of now and the arrival time of the oldest entry of that source name that can be taken now. An entry in a part that is not attached, or behind an older synapse's part, cannot be taken and does not count: such a chain is ticked with the time now, so a due time that has come is never left standing without an entry in the ring to wait for. What the tick releases goes to `released` of the source that last fed the chain. A chain whose due time is later than a takeable entry's arrival is left for that entry's take (5b).
   c. If the pulse is due, call it (7.9).
   d. Takes. Repeat, each time in one L4 section, which is `node_code`'s: stop if this thread no longer holds the in-right, `taking` is set, the ring is empty, `turn_stimuli` takes were made, `turn_time` is used, or the node is Ending and `take_by` has come. Else pop the front source and take from it, in this order: a stimulus of its `released`; else the next entry of its front part; else that part's closing marker. Work out an entry's gap (7.6) and note it in hand (`in_hand`). Mark the call begun. Put the source at the back if it can still be taken from. Let L4 go and handle what was taken (step 5).
   e. Tell what is owed (7.6). Under L4: store `in_due`; if `in_holder` is still this server, clear it; notify `wake` if `taking`; notify `right`.
5. Handling what one take gave, with no runtime lock but the chain's:
   a. A released stimulus: keep it as the latest (node.rs:462-466), then `integrate`.
   b. An entry. If a loss is to be told now: `lost`. Find the slot for its source name and signal. With no slot it is integrated as in (a). Else, if a retired predecessor of that slot still waits, what it held goes through this slot first (4a); if the take found a new start (7.6), the chain is flushed first. Then, under L1: tell the chain of a gap (`Chain::gap`), bring it up to the entry's arrival time (`Chain::catch_up`), apply it with that time; the later of that time and the chain's last one is used, so a chain never sees time go back. What these gave, in that order, is the run. In one L4 section the run is put at the back of the source's `released`, `in_hand` is cleared, and the run's first stimulus is taken for (a). So a take gives at most one `integrate`, and the rest of a release waits under the queue's lock for the next takes.
   c. A closing marker, the first time it is taken: tell what is owed for that source; if this was the last synapse of that source name, flush its chains into `released`; mark the part `flushed`. It is taken again when the source's `released` is empty: the part leaves, and `SynIn.done` is notified. So `done` follows the last `integrate` of what arrived on that synapse.
   d. Record the wait and the handling time.
6. Under L4: work that a server may do remains when (input) `in_ok` would hold and the ring is not empty, or `retired` is set, or `in_due` or `pulse_due` has come; or (output) `others_send(now)` holds and `fired` is not empty, or `out_waiting` is above zero, or `out_due` has come. If so and the node is not `queued`, queue it at the back.

A stuck server: when the watch finds a counting server inside one call for longer than `server_stuck` while another node has waited for a server longer than `wait`, or a node is ready with no server, it marks the slot stuck and `counting` goes down, so another is started. When the call returns, the server finishes its turn. Then, under L5, it counts again if fewer than K count, else it ends.

### 7.4 The loop of a node's own thread

1. The thread function: arm the guard (section 4); name the thread; set its priority (7.8); call `hillock.own_thread(run)`; when that has returned, set and notify `returned`. Everything below is `run`.
2. The claim: T2 step 3 for a declared node; T4 for a move. Then loop:
   a. Under L4: if `leave` is set, or the claim was cancelled: return. Not yet claimed (Moving): claim when the in-right is free (T4 step 3).
   b. The idle test, in the same section. Work for this thread is: output (`fired` not empty, `out_waiting` above zero, or `out_due` come); once claimed, input (the ring not empty, `retired` set, or `in_due` or `pulse_due` come); and, Ending with `steps` false, the end (c). With none of these it waits on `wake` until the earliest of `out_due`, `in_due`, `pulse_due` and, when the node is Ending, `take_by`. A real clock waits with that timeout; a manual clock waits until notified, and `advance` notifies under L4.
   c. The end. Claimed, Ending, `steps` false, and no part remains or `take_by` has come: set `steps`, notify `right`, and run N4 to N6 (7.12) on this thread; tell `serve`; wait on `wake` for `leave`. If `steps` was set by the end thread (G14 a or c): wait on `wake` for `leave`.
   d. Else let L4 go and make one pass. Output: as step 3 of 7.3, at most `help_batch` drain steps or `turn_time`. Then, once claimed, input: steps 4a to 4e of 7.3, at most `turn_stimuli` takes.
   e. Before each take: if output work waits and this thread has not drained since its last take, it takes nothing; it lets L4 go, makes one drain of at most `help_batch` steps, and then takes, whatever `fired` holds by then. So output never keeps the thread from its input for longer than one batch, and what was fired before a take is sent before it, up to that batch. The call is marked begun in the section that takes the stimulus, so no fire can fall between the thread's last look at `fired` and its entering the node's code unseen: it either is seen by that section or finds `in_call` set.
3. While this thread is inside the node's code, what other threads fire waits in `fired`: a firing thread helps at half (7.5); and when the call is older than `wait`, the watch makes the node ready and the shared servers serve its output (7.7; Q6). When a call returns, the thread treats a due time that has come as work.

### 7.5 Fire, and the hand-over to each target

`Node::fire(signal, data)`, on any thread:
1. Off every lock: weigh the data once (`frame::estimate`: its likely size, an upper bound and its depth; when the bound with the op around it passes 16 MB, `frame::measure` counts it exactly).
2. Under L4 only: read `at = now`. Unserved or Starting: if it is a state signal of this node keep it in `pre`; return. `out_closed`, or the caller is the abandoned thread: `fired_after += 1`; return. Else push it, and decide, in this order:
   - (a) the caller is the thread that holds the in-right: it is the node's server, so it sends now;
   - (b) `fired` holds half its cap in count, or half of `line_bytes`: the caller helps, and `helped` counts what it sends. This holds in every mode;
   - (c) `others_send(at)` and the node is not `queued`: queue it for a server;
   - (d) `server()` is `Own`: with `in_call` none, notify `wake`; with `in_call` set, arm the watch for that call's start plus `wait`.
3. Sending now, or helping, is at most `help_batch` drain steps, after L4 is let go. A thread that helps while `fired` is at its cap goes on until it has popped one fired signal (G7). A helper that ends its batch with output work left looks once more under L4 and applies (c) and (d), so what it left is never without a server.

One drain step, by whoever holds L2. Output work is of three kinds, served in this order:
- (a) **`passed`.** If a link holds `passed`: number and hand over one piece of it, at most `help_batch` stimuli. Stop.
- (b) **`carry`.** Else, if a link holds `carry`: pass one piece of it through the link's chain and hand over what passes. Stop. So nothing fired later passes what a replaced chain held.
- (c) **A fired signal or a link op.** Else, under L4, pop the front of `fired`. None: store `out_due`; stop. A link op is applied (7.10). A signal:
  - if it is a state signal of this node and its size fits a frame, keep it as `last_state` (node.rs:497-503); a value that does not fit is not kept, so a link made later gets the last value that fitted;
  - for each link for that signal, in the order the links were made: bring the chain up to the signal's `at`; apply the chain to a copy of the data (node.rs:505) with time `at`; hand over what passes.
- **Handing over** what a chain gave: the first `help_batch` stimuli are numbered and moved at once; the rest goes to the back of the link's `passed`. One stimulus: `seq += 1` (decision 91; numbers start at 1). Its size is the fired size when the link has no function, else weighed again; the size and the depth are those of the message as framed. Over 16 MB or nested deeper than `MAX_DEPTH`: `too_large += 1`, and it goes no further; its number stays used, so the receiver sees the gap when a later one arrives. Then, under the target line's L3: if the way is open to a node of this process in direct delivery and the line is empty, push straight into the receiver's queue (7.2). Else push into the line by the rules of 7.1 step 3b. If the way is an open pipe or an encoded local way, notify its writer. If it is `Unreached`, make it `Reaching` and start the reacher, in this L3 section.
- At the step's end, under L4: `out_waiting` is set to the number of links that hold `carry` or `passed`. After L3 and L2: what was moved out is dropped, a server the push decided is started, the node's ceiling is trimmed if it was passed.

A fired signal meets its links' functions with the time it was fired, and a chain is ticked up to that time first; a tick with the time now happens only when no output work waits. So what the functions do does not depend on when a server came: ten state values fired at once are "the first, then the latest" however the drain is cut (tests/network.rs:444-459; Q15).

**The writer** of a pipe `Remote` writes every control frame that waits. Then, in one L3 section, it looks at its way: if the way is no longer `Open` with this Remote it pops nothing and ends; else it pops one signal. It encodes and writes it, and looks for control frames again. A `detach` is written after the signals that waited when it was asked. If a write fails, the entry goes back to its line (`Line::unpop`: a stream entry to the front with its ticket; a state value to its place unless a newer value of that signal waits), and the close rule applies. A frame that reached its reader although the write reported failure is then written twice: the receiver takes its number once (7.6).

**The close rule**, one for every way an outgoing Remote that carries a branch ends: end-of-file at its reader (synapse.rs:109-118), its own detach, a write that failed, a direct `arrive` answered `closed`. In one L3 section: if the way is `Open` with that Remote it becomes `Unreached`; and if the line holds something, a link remains and the branch is not closed, it becomes `Reaching` and a reacher is started. Since a writer looks at its way in the section in which it pops, nothing is pushed for a writer that has gone.

**The reacher** makes attempts while something waits in its line and a link remains. One attempt is one reach without a launcher (7.11), as `fire_loop` does today (node.rs:554), that never waits for a retry. On success, in L3 sections: a pipe or encoded way is set `Open` and its writer notified; for a direct way the reacher moves what waits into the receiver's queue, in order, one piece of `help_batch` per section, and sets the way `Open` only in a section in which it saw the line empty, so a push between two pieces is never left behind. On failure it backs off, from 50 ms doubling to 1 s, on the process's clock. It decides to end under L3: when nothing waits, or no link remains, or the branch is closed, it leaves the way `Unreached` (or `Closed`) and goes, so a later push starts a new one.

### 7.6 A full line

- **Nobody waits** (decision 97). A stream stimulus that finds its line full makes room by moving a victim out; it is never refused itself, and it always fits an empty line.
- **The victim** is `Line::victim()`: the line's oldest waiting stream item, whatever its signal (decision 97's words). (Q4) asks whether it should be the oldest of the signal that holds most of the line; that needs a deque of tickets per lane and is not built first.
- **The node's ceiling** (decision 104; Q3). After a push that left `total_count` above `node_stimuli` or `total_bytes` above `node_bytes`, and after the pusher has let go of its locks, `trim()` runs: it picks, from the per-line gauges, the line that holds most in the measure that is over, arriving or outgoing, takes that one line's lock, moves its victim out, and repeats while the node is over. `ceiling()` is the one function behind (Q3). State values and `released` take no part (G5).
- **Counting.** The drop is counted on its link at the end where it happened: `LinkIn.dropped_here` at the receiver, `LinkCounts.dropped_here` at the sender.
- **Telling.** Losses are found at the take, from the numbers. A `seq` is any u64 a peer writes (message.rs:87-92), and this arithmetic runs under L4, where a panic would poison the queue: it is saturating throughout. For an entry with number `s` of link `l` taken from synapse `syn`:
  - `s` is 0: the stimulus is unnumbered. It is taken; no gap is worked out and nothing is owed.
  - `syn.origin` differs from `l.origin`: the source's node was made again. It is a new start: the receiver chain is flushed first and starts again; `l.origin = syn.origin`, `l.last_seq = 0`.
  - else `s` equals `l.last_seq`: a repeat. It is dropped and counted (`repeated`); nothing is told and no chain is flushed.
  - else `s` is below `l.last_seq`: the link's numbers started again (a source made again in a process that kept its pid; a link made again, Q8). A new start, as above.
  - Then `g = s - l.last_seq - 1` numbers never came, and `l.last_seq = s`. For a stream link `owed += g`; for a state link nothing is owed. Either way the receiver chain is told the gap, so `in_order` does not wait for it.
  - If `owed > 0` and this link has not been told in this turn, `Hillock::lost(call, signal, owed)` is called before the stimulus, and `owed` is cleared. A gap found later in the same turn stays in `owed` and is told at the end of that turn's input (7.3 step 4e), and at the source's closing marker. So an overloaded node gets at most two `lost` calls per link per turn, and nothing is left owed at rest. What is still owed when the end's steps begin is counted in `left.not_told`.
- **A state stimulus** at a full line takes one of its two places. It is never dropped.

### 7.7 The watch and the measure

The watch sleeps on `alarm` until `wake_at`. **Arming** only ever lowers `wake_at`: under L5, a time earlier than `wake_at` (or any time when none is set) is stored and `alarm` notified; if no watch lives, one is decided there and started after L5. The watch is armed when:
- a node gets waiting work it did not have (for `now + wait`);
- a node's `in_due`, `out_due` or `pulse_due` is set to a time still to come;
- a ready node finds no server while one is inside a call (for that call's start plus `server_stuck`);
- an autonomous node's thread is inside a call while output work waits or arrives (for that call's start plus `wait`).

One pass, at time t:

0. Under L5: take `wake_at`, leaving none. From here an arm made by any thread is kept apart from what this pass computes.
1. **Due times that have come.** For every node of the table, under its L4, each acted on once and only when somebody may serve it now:
   - output (`out_due` has come; or, for a node with its own thread, any output work while its call is older than `wait`): if `others_send(t)`, make the node ready; else if `in_call` is none, notify `wake`;
   - input (`in_due` or `pulse_due` has come, or `retired` is set): only when the in-right is free, `taking` is false and `steps` is false: make the node ready (`Pool`) or notify `wake` (`Own`).
   In every other case nothing is done: the holder of the in-right treats a due time that has come as work when its call returns (7.3 steps 4b and 6; 7.4).
2. **Stuck servers.** As 7.3's last paragraph. Servers are started while a node is ready, none is idle and fewer than K count.
3. **The step**, at most once per `check_every`. For every node in mode Shared, under its L4, with W = `wait` and B = `backlog`. Only what can be taken now counts: what waits in `released`, and the entries of front parts that are attached (the ring's sources). Entries of a synapse whose `attach` has not returned, or behind an older synapse's part, wait for the node's own code or for their turn in order, and stay in the caps only.
   - *waiting for input*: (the in-right is free and `taking` is false) and (the oldest such entry arrived before t − W, or there are more than B of them, or a chain's due time that a tick would act on is before t − W);
   - *waiting for output*: the oldest fired signal was fired before t − W, or `out_due` < t − W, or `helped` differs from `helped_seen`;
   - *costing the others*: `in_call` is set, its holder is a server that still counts, and the pool is starving: no server is idle, K count, and some other node has been in `ready`, or has had such an entry waiting, since before t − W. The node's own entries and its own place in `ready` never count.
   The node is late if any of the three holds. Late: `account = min(account + 1, S)`. Not late: `account` goes down by one, not below zero. Then `helped_seen = helped`.
4. **One move.** Among the nodes whose account is S: first one that is costing the others, then the one late the longest (`late_since`), then by name. It moves by T3 with the reason 'measured'. If it moved, every other shared node's account is set to zero. If the limit refused it, it stays shared with its note, keeps its account, and is tried again at the next step.
5. **Next.** `next` is the earliest of: every due time still to come (later than t); the next moment a counting server would be stuck, if a node waits for a server; `t + check_every` while any node is late or has an account above zero; the oldest takeable entry's arrival plus W, for nodes that have waiting work and are not late; and, for a node whose own thread is inside a call with output work waiting, that call's start plus W if still to come. A due time that has come never arms the watch. Then, in one L5 section: `wake_at` becomes the earlier of `next` and what arms stored since step 0; if that is none, `watching` becomes false and the thread ends; else it sleeps until it. So an arm made while a pass was reading the nodes is never lost, and a watch never ends with an arm standing (Q19).

So nothing passes while nothing waits, and a due time that cannot be served makes no pass. A stalled process cannot add time: a step is one step, however long ago the last one was. The watch is outside tokio, so a starved runtime does not stop it.

In a manual process no watch thread exists: `Process::check()` is one pass at the manual clock's time.

### 7.8 Taking a thread

- **By declaration.** `Hillock::autonomy()` is read once when `serve` starts. `from_start` gives the thread before the first synapse (T2). A declared node always gets its thread, and does not count towards `autonomous_max` (decision 99).
- **By measure.** 7.7 step 4, then T3 and T4.
- **By the node's own call.** `Node::take_thread()` from any of its code: T3 with the reason 'asked'. It returns at once: true if the node has its thread, or a move stands; false if it is not served, is ending, or the thread could not be started. The thread takes over after the call in progress. An asked thread is given like a declared one and does not count towards `autonomous_max`: `counts_as_measured(why)` is the one function behind (Q2).
- **A measured place** is given back by one function: under L4, then L5, and only if `OwnThread.place` is still set, it clears it and lowers `Pool.measured`. N1's cancel, `take_thread` on a measured move, a refused spawn, N7 and the guards all call it; calling it twice changes nothing.
- **The priority** is what `autonomy()` said, whenever and however the thread comes (decision 96). The thread sets it on itself as its first act:
  - Windows: `SetThreadPriority(GetCurrentThread(), ..)` with below normal, nothing for normal, above normal. `Win32_System_Threading` is already enabled (liminal-net/Cargo.toml:21). No right is needed (inferred; A4).
  - Linux: the thread's nice value, relative to what the thread starts with. It reads its own value first (`libc::getpriority` on its thread id: base), then asks `libc::setpriority` on that id, which there applies to that thread alone (inferred; A4 reads it back on the thread and on another), for base + 5 (Low, at most 19), nothing (Normal) or base − 5 (High). So a process started nicer than usual keeps its distance. Any refusal leaves the thread at base, and the report says `priority <asked> asked, normal given: <why>`: `no right to raise a thread` for a refused High (decision 106), the system's words otherwise.
  - Other systems: nothing is set, and the report says so.
  Never a real-time level.
- **`Hillock::own_thread(run)`** wraps the thread's whole life. The default calls `run()`. The bridge attaches the thread to the interpreter once and lets go of the lock around `run` (9.2). Until `run` begins the node is served as before, and a move that is cancelled meanwhile changes nothing. An `own_thread` that returns without calling `run` is the guard's case (section 4).

### 7.9 A node's own periodic work (decision 98; built when hw-stats is ported, step S6)

`Hillock::pulse(&self, node: &Node) -> Option<Duration>`, default `None`.
1. Whatever holds the in-right calls it, through `node_code`: a shared server in its turn, the own thread in its pass. It never runs beside `integrate`.
2. It is called first in the node's first turn after `serve`. `Some(d)` means again `d` after this call began; `None` means no more, until `Node::pulse_soon()` or until a link is made for one of the node's signals. `Node::linked(signal)` lets it rest when nobody is linked.
3. An interval under `pulse_floor` (10 ms) is raised to it, so a pulse cannot spin.
4. What it fires is sent at once: the caller is the node's server (7.5, case a).
5. A pulse that holds a shared server longer than `server_stuck` is treated like any stuck call: the server is replaced, and the report says so. Such a node should declare itself (Q5).
It is not called once the node is Ending.

### 7.10 Link, unlink and tune

A link op takes its place in the line of fired signals and is applied by whoever serves the node's output, like a signal. So it takes effect exactly after what was fired before it, a reading task never does a node's output work (G9), and decision 102's words hold for it. For an op that arrives on synapse `made_on` of node S (a reading task), or is sent by a node of the same process through a local Remote:
1. Build the chain from the declared functions (node.rs:375-383); a bad function is answered `BadFunction` at once, as today.
2. Push the op into `fired`, under L4, with the synapse's origin and where to answer, and apply `fire`'s rule for whom to wake (7.5 step 2, cases c and d). A reading task never helps; a calling thread of the same process may (cases a and b). The caller waits for the answer as it does on the wire.
3. When a drain step pops it, under L2:
   - **link**: if a link for that signal and target exists, flush its chain into the link's `carry` (behind what `carry` already holds), put the new chain in its place, and take the op's synapse as `made_on`: the link then belongs to the synapse it was last made on, and the closing of an older one no longer takes it (today the whole link is replaced: node.rs:394-401). Else put a new link and make the target's branch. Its number is `Axon::numbers_from()`, the one function behind (Q8): first behaviour, it starts again from 0, as today, and the receiving end reads the lower number as a new start (7.6); with the yes, it goes on from `seqs` when the op's origin (the pid, and in one process the instance) is the one that last held the link, and starts again otherwise. If the node has a latest value of that state signal, apply the chain to it and hand it over (decision 90; node.rs:510-517).
   - **tune**: no such link is `BadFunction`, 'no link for ... to tune' (node.rs:418-421). Else flush the old chain into `carry` and put the new chain in.
   - **unlink**: remove the link; what its chain, its `carry` and its `passed` held is dropped, since the receiver asked for no more. When no link to that target remains, its branch closes: an open way writes what waits, any other drops it and counts it in `ended`.
4. Under L4: `out_waiting` and `out_due` are stored, so the `carry` it made is output work at once: the drainer goes on, or step 6 of its turn queues the node. After L2: answer null, or the error (decision 92). The answer leaves when the op is in effect.

What `carry` holds passes through the new chain in pieces of `help_batch`, each piece one drain step, ahead of any later signal (7.5 b); whoever serves the node's output moves the pieces, also when nothing more is fired. So a tune of a gate that held 100,000 items never holds L2 for more than one piece, and the held items never wait for a later fire.

At the linking node, `Remote::link` records the signal's kind, read from the interface it was admitted with (synapse.rs:35), before it sends the op.

A receiver's own chain (`Node::receive`, `Remote::link_with`) is put in by swapping the slot in the `receivers` map. The old slot is retired. Then, after the map is let go, under L4: `retired` is set and the node is woken (`wake` notified, or the node queued; a server that holds the in-right finds the flag at its step 6). So what the old chain held does not wait for the next stimulus, also when the swap is made while a call of the node's code runs. The holder of the in-right flushes it, passes what it held through the new chain and integrates what passes: at the start of a turn's input, and before a slot is first used for an entry (7.3 step 5b), so nothing of the new chain is integrated ahead of what its predecessor held. Nothing waits for a chain's lock, so a function that replaces its own chain does not lock itself.

### 7.11 A request over a pipe, a direct request, reaching and admission

**Over a pipe.** As today (synapse.rs:163-198; dendrites.rs:97-128), with these changes:
- `ask` encodes the request on the caller's thread. A message over 16 MB, or nested deeper than `MAX_DEPTH`, is `NetError::Frame` with its words, and nothing is sent.
- The worker that ran the request encodes the answer. An answer that does not fit, or is nested too deep, becomes an error record of kind `NetError` with the message `frame: a message of N bytes is larger than a frame holds (16 MB)` (errors.rs:108-109, 128-134; frame.rs:15-18), or `frame: a message nested deeper than 120 levels`. An error record that itself does not fit (its `accept` or `denied`) is sent as that same plain record, without them. The writer only writes bytes, so it lives.
- **The size rule is of the message as framed**, on all three deliveries: the op's own map is the first level, so a signal's data and an answer's value may be nested `MAX_DEPTH` − 1 levels, and a request's argument or an error's `accept` `MAX_DEPTH` − 2.
- The request runs inside `catch_unwind`. A panic is the record `NodeFailed`, `the node's code failed: <the panic's words>`, the same on all three deliveries.
- A request in flight when its caller detaches is answered, as today (the answers' writer lives while a request holds a sender: dendrites.rs:78-84, 105-127). `answers_after_detach()` is the one function behind (Q12). A request in flight when the reached node begins to end fails `Closed` at its caller (NETWORK.md §5.4): N1 ends the answers' writer and the connection; the callee's code runs on.

**Direct, the blocking form** (`Remote::request_blocking`, `Node::call_blocking`), on the caller's thread:
1. Pass the runtime's own guard: one empty `block`. A tokio async thread is refused with the same panic as the wire form gives today (inferred; D2).
2. If the synapse is closed: `NetError::Closed`. Take the next id from the `Remote`'s counter; `in_flight += 1`. If `attach` has not returned for this synapse, wait in one `block` until it has, or until the synapse is closed (N1 and 7.13 wake the waiters): then `NetError::Closed`, and nothing ran.
3. The size rule on the request as it would be framed, id included. Encoded delivery passes the op through the codec.
4. Where it runs. On this thread, unless this thread's `running` list is already `direct_depth` (16) direct calls deep, or already holds the callee node: its code is running on this thread, or this thread called out of it. Then steps 5 and 6 run on a worker thread of the callee's runtime and this thread waits for it in one `block`. No error is given: it is what the wire does, its hang on a lock the outer call holds included (G8). A direct call notes the calling node and the callee on `running` around the call, as `node_code` notes the node around every call of its code.
5. The begin, in one L4 section of the callee, as the first act on whichever thread runs it: if the synapse is closed, or the node is Ending or Ended, answer `NetError::Closed`: nothing runs. Else `busy += 1` (today at dendrites.rs:106). Then `node_code(answer)` (node.rs:354-373). `busy -= 1`, and notify `changed`.
6. The size rule on the answer, as over the pipe. Encoded delivery passes the answer or the error record through the codec.
7. `in_flight -= 1`, and the synapse-end task is woken if it waits for that. If the callee began to end meanwhile, the caller gets `NetError::Closed` once the callee's code has returned (G15). If only the caller's own detach closed it, the answer is given, as on the wire (Q12).
Arguments and the answer are owned values that move; nothing is shared.

**Direct, the async form** (`Remote::request`, `Node::call`): steps 2 and 3 on the caller; steps 5 and 6 with `spawn_blocking` on the callee's runtime, the begin of step 5 being the closure's first act; a node that begins to end answers `Closed` at once while the callee finishes.

**Reaching** (`Node::reach`, `reach_blocking`, and the reacher):
1. A synapse already open to the target is shared (axon.rs:96-106).
2. The deadline is now plus `reach_wait`, read on the process's clock before anything is waited for.
3. Attempts, until the deadline. One attempt holds `reaching[target]`; the wait for that mutex is timed by the deadline, and a reach that gets it looks for an open synapse again first. So two reaches of one target never connect at once (today: axon.rs:96-131), and one that waits behind another still fails after its own `reach_wait`, with the words of axon.rs:150-154. Inside the attempt:
   - unless the delivery is wire, look in the table. An entry whose node is Shared, Moving or Autonomous: local admission (below). Any other entry: retry;
   - else `transport::connect` (axon.rs:112). When it succeeds and the delivery is not wire, look in the table once more: a node of this process enters the table before it binds (T1), so one that bound between the two looks is there now. If it is, the connection is dropped unused (the node sees a silent connection, never a synapse) and the attempt goes on as the first case. Else the handshake, as today;
   - nobody answers: the launcher, once, as today; without one, `Unreachable`.
   Between attempts the mutex is let go and the reach sleeps 50 ms (axon.rs:159).
4. The reacher's attempt never waits for a retry: a refusal with retry, or nobody answering, ends it, and the reacher's own back-off follows (7.5). So it holds the mutex for one connect and handshake.

**Admission**, one function for the reading task and for a local reach:
1. Local only: both keys are checked with the handshake's own `proof` and `verify` (handshake.rs:50-60). A wrong key answers `NetError::Closed`, as the reaching side reads it on the wire (handshake.rs:102-104, 62-67; frame.rs:37-39): nothing about the key is said (NETWORK.md §5.3), and it is never counted.
2. A node that is not Shared, Moving or Autonomous answers retry, and the axon waits as today (axon.rs:133-137). Then the protocol and the name, as today (dendrites.rs:31-40).
3. `admits` runs on a worker thread and refuses with its words. A local reach waits for it at most `hello_wait` and then fails with the wire's `Timeout`, `timed out: no answer within the handshake's wait` (handshake.rs:65), still holding nothing but `reaching[target]`.
4. `Queue::admit`, one L4 section: if the mode is not Shared, Moving or Autonomous, answer retry (the node began to end while `admits` ran). Else make the `SynIn`, put it in `synapses` and add its part to the source's line: it is registered, not yet counted.
5. Answer `admitted`. If that cannot be written (the peer went away), the registration is undone in one L4 section: the `SynIn` and its empty part are removed; nothing was counted, `ever` is not set, and neither `attach` nor `detach` is called (today the synapse is counted only after that write: dendrites.rs:63-67). Else, in one L4 section: it is counted (`synapses += 1`, `ever`), and `changed` is notified. The reach returns. `attach` runs on a worker; it is not awaited by the reach. Until it returns the synapse is not `attached`: a pipe's frames wait unread, as today (dendrites.rs:70-74); a local synapse's signals wait in its part, which is not in the ring, and a local request waits (step 2 of the blocking form). When it returns: `attached`, the source enters the ring if it holds something, the node is made ready.
N1 marks every registered synapse closed in the L4 section that sets Ending, so a synapse is either registered before the end began, and closed by it, or refused with retry. A synapse closed before its `attach` began gets neither `attach` nor `detach`.

### 7.12 The end of a node

`E` is `Limits.end`. `serve` decides the end and drives its last steps; the steps N4 to N6 are run by one thread, the runner. `end_by` is the moment the end began plus E; `take_by` is `end_by` less min(`server_stuck`, E / 2).

- **N1.** The reason is decided as today (node.rs:217-249); a natural end still waits for requests in progress. Then one L4 section. For a natural end it first looks again: a synapse registered or a request begun since the decision keeps the node running, and `serve`'s loop goes on. Else: the mode becomes Ending and the stage Securing; `end_by` is set; a move not yet claimed is cancelled (T5); every registered synapse is marked closed and its part `closing`, and `detaching` is raised for each whose `attach` had begun; `wake` is notified; a node the pool serves is made ready. So nothing can arrive after this section, and every closing marker is already in its line. After it: each pipe's reading task is told to stop and its answers' writer and connection end; the callers waiting on a local synapse are woken (7.11); each synapse goes on through 7.13 from its step 3; the end thread is started. New synapses are refused with retry, as today (dendrites.rs:26-30).
- **N2. Finishing.** Whoever serves the node goes on taking, as in any turn: what was released, the entries in order, then each closing marker with its telling, its flush and the integrates of what the flush gave. Every call is `node_code`, which begins no signal call once `take_by` has come. The pulse is not called again.
- **N3. Who runs the steps**, decided under L4 by setting `steps`. "No part remains" means: every part's marker was taken and nothing waits in `released`.
  - A node with its own thread: that thread, when no part remains or `take_by` has come (7.4 c). Its wait on `wake` has `take_by` as a deadline, and N1 and `advance` notify it. It notifies `right`.
  - A node the pool serves, or one whose own thread is gone: the end thread. It waits on `right` until the in-right is free and (no part remains or `take_by` has come; at once for a thread that is gone); then `in_holder = Ender` and `steps`.
  - At `end_by`, if `steps` is still false, the end thread looks under L4 and takes the steps whatever it finds: `in_holder = Ender`, `steps`. If `in_call` is set, that call began before `take_by`: its thread is `abandoned`, `left.beside` is set, and `in_hand` is added to `left.not_integrated` (G14 a). If `in_call` is none and the holder was the node's own thread: `left.off_thread` (G14 c); that thread finds `steps` set and waits for `leave`.
  - The end thread returns as soon as it sees `steps` set by another thread, or `leave`.
  From `steps` on nobody takes the in-right (7.3 step 1, 7.4 a) and `node_code` refuses every signal call (5.3): the abandoned thread finishes the one call it is in and starts no other, an old holder leaves its turn at its next look, and the runner itself calls no signal code.
- **N4. Counting, and the first flush.** By the runner, with no call of the node's signal code:
  - Under L4 it swaps the arriving lines out and reads what is owed. Outside the lock it counts into `left.not_integrated` the entries still in the lines (stream and state), the stimuli in `released`, and what each receiver chain holds (`Chain::held()`, each chain under its own lock if that is free; a chain whose lock the abandoned call holds is counted in `left.chains_unread`). What was owed and not told goes to `left.not_told`. The lines are dropped there, outside the lock. It notifies `done` for every part whose marker was not taken.
  - Output: drain steps until nothing waits (7.5); then each link's chain is flushed into its `passed` and the pieces are handed over. What a throttle, a debounce, a batch or `in_order` held is numbered and sent, so the last state fired before the end is not lost. The links' functions are built-ins (decision 50), so this calls no node code.
  - It waits on its own condvar (`wake` for the node's own thread, `right` for the end thread) until `detaching` is zero or `end_by` has come (NETWORK.md §4.3: `detach` before `shutdown`). At `end_by` each `detach(peer)` still pending is marked skipped by compare-and-swap, counted in `left.not_detached`, and never called; one that is running goes on.
- **N5.** `Hillock::shutdown()`, by the runner, caught like any call of node code. What it fires is sent at once: the caller holds the in-right (7.5, case a).
- **N6. The last hand-over.** By the runner, after `shutdown` has returned:
  1. drain steps until nothing waits;
  2. the closing hold: L2, then L4 inside it. If `fired` is not empty, back to 1. Else `out_closed` is set: from here `fire` discards and counts (`fired_after`). Still in that hold each link's chain is flushed into its `passed` once more, so what `shutdown` fired into a throttle, a debounce, a batch or `in_order` goes out; what a closed `gate` still holds is counted in `left.not_sent`;
  3. the pieces of `passed` are handed over.
  The runner then tells `serve`. `serve` waits until every target's line is empty, at most E from `shutdown`'s return. Then, empty or not, one L2 section: the axon is closed; each branch's way becomes `Closed` under its L3, so no reacher starts and its writer stops; what still waits in a line, in `carry` or in `passed` is moved out and counted in `left.not_sent`; the reachers end. What was moved out is dropped after the locks. The links were kept until here (7.13), so what the end fired had its targets.
- **N7.** In this order: the axon's remotes are detached (node.rs:267-270); under L4 `leave` is set and `wake` and `right` are notified; a measured thread's place is given back (7.8); `serve` waits for the own thread, at most 1 s of real time, through `returned`, which the thread function gives after `own_thread` has returned (7.4), accepting and refusing with retry meanwhile; the listener is dropped; the node leaves the table, if the entry is its own; last, the mode and the stage become Ended. So from N1 until the listener is dropped a connection is always answered with retry (axon.rs:133-137; decision 83), a local reach that finds the entry gets retry until it is gone, and a watcher of `Node::stage()` (node.rs:194) that reads Ended can serve a successor at once, as today (node.rs:262-263).

The reason is today's words, with a tail only when something was left. Its counts, each only when above zero: `asked to; 12 signals not integrated, 4 losses not told, 3 not sent, 1 peer not detached`. Then, after the last resort, `; its data was secured beside a call still running`, or `; its data was secured off its own thread`. So `shutdown` has started by E after the end began, and the end takes at most E more after `shutdown` returns, and 1 s for the thread.

If `serve` is dropped before its end (its runtime shut down), a guard sets `out_closed`, the mode to Ended, `abandoned` to the holder's thread if it is in a call, and `leave`; notifies `wake` and `right`; gives a measured place back; and removes the node's own entry from the table. The own thread ends without calling anything. Nothing is secured, as today.

### 7.13 The end of one synapse

When a reading task ends, or a local synapse is detached or closed:
1. `Remote::detach` only marks and returns: the end runs in a task of the reached node, never on the calling thread. Requests in flight on the synapse are answered (7.11; Q12).
2. One L4 section: the synapse is closed; its part is marked `closing`, so its marker is taken after everything that arrived on it, and after the last `integrate` of it has returned; `detaching` is raised if its `attach` had begun; the node is made ready. At a node's end N1 has done this for every synapse.
3. The reaching end of a local synapse. On the wire the reaching node's reader meets end-of-file once the reached node has written its last answer, and runs `Out::close` (synapse.rs:53-64, 118). A local Remote has no reader, so the synapse-end task runs that close for it, once no request is in flight on it (at once when the reached node is ending): the Remote is not open; a call still waiting fails `Closed`; the handles the reaching node lent over it with `lend_to` are released (node.rs:323; synapse.rs:59-62) and the Remote is forgotten; `closed()` wakes. dendrites.rs:185 releases nothing today: handles are registered against the lender's own axon-side id, and a dendrite-side id from the same counter (node.rs:241; axon.rs:124) never equals one. 0.3 drops that line.
4. The task waits for `SynIn.done`, at most E (today the wait has no bound: dendrites.rs:181-182).
5. Then, whether `done` came or the bound passed, the bookkeeping: the synapse leaves the count (`synapses -= 1`) and `changed` is notified. If the node is not Ending, the links made on it go (dendrites.rs:184), in one L2 section: what their chains, `carry` and `passed` held, and what waits in a branch left without a link and without an open way, is moved out and counted in `ended`. **While the node is Ending the links stay**: N4 to N6 still send through them, and N6's bound closes them. Nothing of the arriving side is dropped here: while the node runs, the stimuli still in the part are within the line's caps and are taken in order (decisions 97 and 107 drop only at a full line and at a node's end); at the end N4 alone counts what was left.
6. `detach(peer)` runs on a worker thread once `done` has come, however long that takes; the task holds nothing meanwhile. The closure first changes the synapse's `detach` state from pending to running by compare-and-swap; if it finds it skipped (N4, at `end_by`) it calls nothing. When the call has returned, or was skipped, `detaching` is lowered under L4 and `wake` and `right` are notified. So `detach(peer)` always follows that peer's last `integrate`, as today (dendrites.rs:180-190), and is never called after it was counted as not detached.

## 8. The limits and settings

All are fields of `Limits`, held by the `Process`. `Limits::from_env()` reads the variables once, by the rule of 3.1. A test gives its own values. Every value is clamped to its floor and its ceiling, and the report shows the value in use. A time's ceiling is a year unless the table says otherwise.

| field | first value | floor | ceiling | environment variable | what it is |
|---|---|---|---|---|---|
| `wait` | 0.25 s | 1 ms | | LIMINAL_QUEUE_WAIT_SECONDS | W: how long a stimulus may wait before its node is late (decision 103) |
| `backlog` | 256 | 1 | | LIMINAL_QUEUE_BACKLOG | B: stream stimuli waiting for a server before the node is late |
| `sure` | 10 s | `check_every` (so S is at least 1) | | LIMINAL_QUEUE_SURE_SECONDS | how much more late than on time a node must be before it moves |
| `check_every` | 0.1 s | 1 ms | | — | the measure's step |
| `line_stimuli` | 1,024 | 1 | | LIMINAL_QUEUE_STIMULI | stream stimuli in one line (decision 104) |
| `line_bytes` | 16 MB (16 × 1,024 × 1,024, as frame.rs:9) | 1; one stimulus always fits an empty line | | LIMINAL_QUEUE_BYTES | their bytes |
| `node_stimuli` | 4,096 | `line_stimuli` | | LIMINAL_NODE_STIMULI | the node's ceiling over all its lines (Q3) |
| `node_bytes` | 64 MB | `line_bytes` | | LIMINAL_NODE_BYTES | the same, in bytes |
| `fired` | 256 | 2 | | — | the line of fired signals; a firing thread helps at half, or at half of `line_bytes` |
| `help_batch` | 64 | 1 | | — | the most one call of `fire` sends; the size of a piece |
| `servers` | min(4, cores) | 1 | min(4, cores): decision 100 fixes the number, the variable can only lower it | LIMINAL_SHARED_THREADS | K |
| `server_stuck` | 0.25 s | 1 ms | | — | one call longer than this: the server is replaced |
| `server_idle` | 10 s | 0 | | — | an idle server ends |
| `turn_stimuli` | 16 | 1 | | — | a turn's takes |
| `turn_time` | 5 ms | 0 (the call in hand always runs) | | — | a turn's time, read from the process's clock |
| `autonomous_max` | 16 | 0: none moves by measure | 16: decision 99 fixes the number, the variable can only lower it | LIMINAL_AUTONOMOUS_MAX | threads taken by measure |
| `end` | 5 s | 10 ms | | LIMINAL_END_SECONDS | E (decision 107) |
| `direct_depth` | 16 | 1 | | — | nested direct calls on one thread before the next hops to a worker |
| `pulse_floor` | 10 ms | 1 ms | | — | the shortest interval of a pulse (step S6) |

The delivery is LIMINAL_NET_DELIVERY (`direct`, the default; `encoded`; `wire`), read by `Process::global()`. Fixed in the code: the reacher's back-off (50 ms doubling to 1 s), `frame::MAX_FRAME` (16 MB), `frame::MAX_DEPTH` (120; Q13). `Timing` keeps its fields and its variable; its reading takes the rule of 3.1.

Which waits follow the process's clock, so a manual process can drive them: every wait of 5.3's table; `serve`'s start wait and grace (node.rs:215-248); the quiet watcher (synapse.rs:121-139); a reach's deadline and sleep (axon.rs:109, 159); the reacher's back-off. Which do not, and stay on real time in every process: the handshake's `hello_wait` over a pipe (handshake.rs:62-67) and the pipe-busy retry (transport.rs:73-87), since `handshake.rs` and `transport.rs` are unchanged; N7's 1 s for the thread.

## 9. The public API

### 9.1 liminal-net

```rust
// hillock.rs: four methods with defaults; a node written for 0.2 compiles unchanged
pub trait Hillock: Send + Sync + 'static {
    fn autonomy(&self) -> Autonomy { Autonomy::default() }
    fn lost(&self, _call: &Call<'_>, _signal: &str, _n: u64) {}
    fn own_thread(&self, run: &mut (dyn FnMut() + Send)) { run() }
    fn pulse(&self, _node: &Node) -> Option<Duration> { None }          // step S6
    // interface, request, integrate, admits, attach, detach, shutdown: as they are
}

// autonomy.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Autonomy { pub from_start: bool, pub priority: Priority }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority { Low, #[default] Normal, High }

// lifecycle.rs
#[derive(Debug, Clone)] pub struct Limits { /* the fields of section 8, all pub */ }
impl Default for Limits;
impl Limits {
    pub fn from_env() -> Limits;
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Limits;   // the rule of 3.1
    pub fn clamped(self) -> Limits;                                          // floors and ceilings
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Delivery { Direct, Encoded, Wire }
impl Delivery { pub fn from_env() -> Delivery; pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Delivery }

// process.rs
#[derive(Clone)] pub struct Process { /* private */ }      // Debug prints its delivery
impl Process {
    pub fn global() -> Process;                                   // Delivery::from_env(), Limits::from_env()
    pub fn new(delivery: Delivery, limits: Limits) -> Process;    // a private one
    pub fn manual(delivery: Delivery, limits: Limits) -> Process; // no server, no watch, a manual clock
    pub fn delivery(&self) -> Delivery;
    pub fn limits(&self) -> &Limits;                              // the values in use
    pub fn now(&self) -> tokio::time::Instant;
    pub fn servers(&self) -> usize;                               // live server threads
    pub fn watching(&self) -> bool;
    // on a manual process only; they panic on any other
    pub fn advance(&self, by: Duration);      // moves the clock and wakes every wait that has a deadline
    pub fn check(&self) -> Check;             // one pass of the watch
    pub fn run_one(&self) -> bool;            // one turn of the front ready node, on this thread; false if none, or K count
    pub fn run_until_idle(&self) -> usize;    // turns until nothing is ready
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Check { pub made_ready: usize, pub replaced: usize, pub stepped: bool,
                   pub moved: Option<String>, pub refused: Option<String> }

// node.rs
pub struct NodeConfig { /* name, kind, scope, ipc_dir, key, timing */ pub process: Process }
impl Node {
    pub fn take_thread(&self) -> bool;
    pub fn join_thread(&self);     // waits until the node's own thread, if it had one, has returned from own_thread;
                                   // returns at once for a thread that was abandoned inside the node's code
    pub fn report(&self) -> Map;
    pub fn state_line(&self) -> String;
    pub fn linked(&self, signal: &str) -> bool;
    pub fn pulse_soon(&self);                                     // step S6
}

// synapse.rs
impl Remote { pub fn delivery(&self) -> Delivery }              // Wire for a pipe

// frame.rs
pub const MAX_DEPTH: usize = 120;                                // levels of the message as framed (Q13)
pub async fn read_sized<R: AsyncRead + Unpin + ?Sized>(r: &mut R) -> Result<Option<(Value, usize)>, NetError>;
pub async fn write_bytes<W: AsyncWrite + Unpin + ?Sized>(w: &mut W, bytes: &[u8]) -> Result<(), NetError>;
#[derive(Debug, Clone, Copy)] pub struct Size { pub likely: usize, pub upper: usize, pub depth: usize }
pub fn estimate(value: &Value) -> Size;      // walks without recursion; counts no deeper than MAX_DEPTH + 1
pub fn measure(value: &Value) -> usize;      // exactly serde_json::to_vec(value).len(); only for a depth within MAX_DEPTH

// functions.rs: three methods with defaults on SynapticFunction, and their Chain forms
fn flush(&mut self, _now: Instant) -> Vec<Stimulus> { Vec::new() }  // give up what is held, in order
fn gap(&mut self, _from: u64, _to: u64) {}                           // the numbers from..to will never come
fn held(&self) -> usize { 0 }                                        // how many stimuli it holds now
impl Chain {
    pub fn flush(&mut self, now: Instant) -> Vec<Stimulus>;
    pub fn gap(&mut self, from: u64, to: u64);
    pub fn held(&self) -> usize;
    pub fn catch_up(&mut self, at: Instant) -> Vec<Stimulus>;        // tick, if something was due by `at`
}

// rules.rs: compiled in debug builds and with the `rule-checks` feature
pub const ENABLED: bool;                                             // false in a build without the checks
pub fn on_async_thread(handle: &tokio::runtime::Handle) -> bool;     // the empty-block probe of 5.3
pub fn set_attached(check: fn() -> bool);                            // the bridge's: is this thread attached to the interpreter?
```

`lib.rs` also exports `Autonomy`, `Priority`, `Limits`, `Delivery`, `Process`, `Check`. `NodeConfig::from_env` sets `process: Process::global()`.

`run_one` and `run_until_idle` run a node's code on the calling thread, so they take `node_code`'s guard in every build: called on a tokio async thread they panic before any node code runs (a `#[tokio::test]` body must call them through `spawn_blocking` or from a plain thread).

With the checks on, every public method of `Node`, `Remote` and `Process`, their constructors included, first asks the check set with `set_attached` and panics when it says yes (9.2). `address::digest`, `address::address`, `key::ipc_dir`, `key::load_or_create`, `Timing::from_env`, `Limits::from_lookup` and `Delivery::from_lookup` take no lock of the network and carry no check.

`flush` for the built-ins: `throttle` and `debounce` give their held latest; `batch` gives its part-filled batch; `in_order` gives everything it holds, in order, and starts again from the next stimulus; `gate` gives nothing. `held` says how many stimuli a function holds at that moment: after a flush, only a closed gate's is above zero. `gap` is only `in_order`'s: it moves its next number past the gap. No built-in's behaviour on `apply` or `tick` changes, but for one hole: `in_order` adds one to a number a peer wrote (functions.rs:517, 528), and that becomes saturating (fault 23). `functions::hold_cap()` is the one function behind (Q1): first behaviour `None`, no cap. With `Some((stimuli, bytes))`, the line's caps: `gate` drops its oldest held and `in_order` gives up its oldest gap early and releases in order, each drop counted on its link as `dropped_here`, and for a stream told by the number it leaves out.

Signatures that stay, with a changed meaning:

| item | 0.2 | 0.3 |
|---|---|---|
| `Node::fire` | pushes to an unbounded channel (node.rs:285-290) | 7.5: a push, and at most `help_batch` drain steps of this node's own output work (G7's exception); before `serve` it keeps a state's latest; after the node's output closed it is counted |
| `Node::serve` | node.rs:205-272 | T1 or T2, then the end of 7.12; a second call while it is served returns at once |
| `Node::reach`, `reach_blocking` | always the pipe (axon.rs:95-161) | the table first, and again after a connect, unless wire; one attempt in flight per target; every reach keeps its own deadline |
| `Remote::request`, `request_blocking`, `Node::call`, `call_blocking` | frames | 7.11 |
| `Remote::link`, `link_with`, `unlink`, `tune` | applied at once, racing with what was fired (node.rs:385-425) | 7.10: applied at their place in the line of fired signals and answered when in effect; a link made again keeps to the synapse it was last made on; its numbers by Q8 |
| `Remote::detach` | a frame, in line (synapse.rs:278-282) | written after what waited; a local synapse ends in a task of the reached node; requests in flight are answered, as today (Q12) |
| `Node::receive` | discards what the old chain held (node.rs:127-132) | what it held passes through the new chain, ahead of it, without waiting for the next stimulus |
| `Node::stop` | leaves open synapses served (node.rs:250-271) | 7.12: requests in flight on them fail `Closed` at their callers |

**`Node::report()`** gives a map, its keys in this order. Each part lands with the structure it counts (section 12):
- `thread`: `shared`, `moving` or `own`; `why`: null, `declared`, `measured` or `asked`; `since`: seconds since the Unix epoch, or null;
- `priority`: `{asked, given, note}`; `note`: null or the queue's note;
- `waiting`: `{stimuli, bytes, state_bytes, released, fired, output}` (`output`: links with `carry` or `passed`); `oldest_wait` and `longest_wait`, in seconds (`longest_wait` is since the previous report and starts again when read); `in_call_for`: null, or the seconds the holder has been inside one call;
- `late` and `sure`, in seconds (the account times `check_every`, against `sure`);
- `waits` and `handling`: twelve counts each (under 1 ms, then doubling, the last from 1,024 ms); `busy_share`: null, or the share of time the node's thread was not asleep; `failed`; `fired_after`;
- `incoming`: per source and signal `{from, scope, signal, kind, waiting, taken, coalesced, lost, owed, dropped_here, repeated, high: {stimuli, bytes}}`;
- `outgoing`: per link `{to, scope, signal, sent, waiting, dropped_here, too_large, way}`; `ended`: per target `{to, scope, dropped}`, kept after its links are gone;
- `kept`: `{latest, links_in, kinds, sources, seqs, branches, reaching}`, the sizes of what is never pruned;
- `process`: `{servers, replaced, measured, of, delivery, limits}`;
- `left`: null, or `{not_integrated, not_told, not_sent, not_detached, chains_unread, beside, off_thread}`.

**`Node::state_line()`** gives one line of words, built from these parts, joined by ` · `:
- `shared`, or `moving to its own thread`, or `its own thread for <n> s (<why>)`;
- `priority <given>` when it is not normal, or `priority <asked> asked, <given> given: <note>`;
- the note, when there is one;
- `<n> waiting, oldest <x> s`, when something waits; `inside one call for <x> s`, when above `server_stuck`; `longest wait <x> s`, when above zero; `<n> lost`, when above zero.
Seconds are written by `lifecycle::seconds` (lifecycle.rs:60-67).

### 9.2 The bridge (`liminal_net`)

- `Node(name, kind, scope, hillock, interface, ipc_dir=None, timing=None, process=None)`. `process=None` is the global one. `timing` takes the rule of 3.1 (today a huge value panics: bridge:434-438).
- `Process(delivery=None, limits=None, manual=False)`: `delivery` is `'direct'`, `'encoded'`, `'wire'` or None (the variable); `limits` maps the field names of section 8 to numbers, durations in seconds. Getters `delivery`, `limits`, `servers`, `watching`. On a manual one: `advance(seconds)`, `check()` (a dict), `run_one()`, `run_until_idle()`. The class lands with step S0; its manual calls work from S1.
- New on `Node`: `take_thread()` (a bool), `report` (a dict), `state_line` (text). New on `Remote`: `delivery` (text; step S5).
- On the hillock object, all optional:
  - `autonomy`: an attribute, or a method called once when the node is made, under the interpreter's lock (it is Python). `True` or `False` is `from_start`; a dict may hold `from_start` and `priority` (`'low'`, `'normal'`, `'high'`).
  - `lost(peer, signal, n)`; `pulse()`, which gives seconds or None (step S6).
- `PyHillock::own_thread(run)` is `Python::attach(|py| py.detach(run))`: the node's thread keeps one Python thread state for its life and holds the interpreter's lock for none of its loop (inferred for PyO3 0.30; B3 reads `threading.get_ident()` and a thread-local across calls). Each call of Python code attaches for that call only, as today (bridge:285, 303, 345, 383).
- **Rule 2, by construction.** The bridge keeps every `net::Node`, `net::Remote` and `net::Process` inside a wrapper, `Net<T>`, whose field is private to its module and whose only accessor is `with(py, |value| ..)`, which is `py.detach` around the closure. So a method that reaches the network while it holds the interpreter's lock does not compile. Every method goes through it, the getters included:
  - `Node`: `serve`, `start`, `join`, `stop`, `fire` (it converts under the lock first; today it keeps the lock: bridge:546-549), `reach`, `lend`, `call`, `release`, `latest`, `receive`, `take_thread`, `report`, `state_line`, and the getters `lent`, `name`, `address`, `target`, `synapses`, `stage` (today `start`, `stop`, `lend` and these getters keep the lock: bridge:510-514, 541-543, 571-574, 612-649);
  - `Remote`: `request` (the blocking form; today the async form: bridge:662-673; to a node of the same process it is a direct call on the calling Python thread, which attaches again for a Python callee), `link`, `unlink`, `tune`, `detach`, `closed`, and the getters `pid`, `interface`, `target`, `is_open`, `delivery` (today `detach` and the getters keep the lock: bridge:713-715, 723-742);
  - `Process`: every method and getter. `run_one` and `run_until_idle` run a node's turn on the calling Python thread, without the lock; Python node code attaches per call.
  The constructors and the module functions call into `net` too, and are not behind the wrapper: `Node(...)` (bridge:431, 482, 498), `Process(...)`, `digest` and `address` (bridge:754-764). They go through one helper, `into_net(py, || ..)`, which is `py.detach` as well; a constructor converts its Python arguments first.
- **Rule 2, checked.** With the checks on, the bridge gives liminal-net its check with `rules::set_attached`: the calling thread is attached to the interpreter when `PyThreadState_GetDict()` is not null (pyo3-ffi-0.30.1 src/pystate.rs:60, in the limited API; `PyGILState_Check` is not: pyo3-ffi-0.30.1 src/lib.rs:601-606, and PyO3's own `thread_is_attached` is private: pyo3-0.30.1 src/internal/state.rs:38) (inferred that it is null inside `py.detach`; B9 proves both directions). There is no flag: the check asks only whether the thread holds the lock now, so it cannot be fooled inside a nested direct call, where Python node code runs on the thread that called `request`.
- **Rule 1, checked.** `liminal_net._on_async_thread()` gives `rules::on_async_thread` for the bridge's runtime; a Python callback calls it and records the answer (B10). Thread names are not used: tokio gives its blocking threads the name of its async ones (tokio-1.54.1 src/runtime/blocking/pool.rs:296, 514; src/runtime/scheduler/multi_thread/worker.rs:506-515), and Python names a thread it did not start `Dummy-N`.
- **The build with the checks.** `rule-checks` is a cargo feature of liminal-net, passed on by liminal-net-py. pip builds a maturin project in the release profile, so CI's bridge job (.github/workflows/ci.yml:54) and the local instructions build with it: `--config-settings=build-args="--features rule-checks"` to pip, `--features rule-checks` to `maturin develop` (inferred from maturin; B0). `liminal_net._rule_checks` says whether the build has them, and test_rules.py fails, not skips, without them. With the feature only: `liminal_net._on_async_thread()`, `_probe_on_async()` (the probe run from an async task: it must say true) and `Node._enter_held()` (a call into the network with the lock held, around the wrapper: it must panic).
- **The exit hook.** The module keeps a registry of every node made (weak), not only of those started. `liminal_net._end_all(registry=None)` stops each node that is served, waits for its end, and then waits with `Node::join_thread` until the node's own thread has returned from `own_thread`, all without the lock and without a bound of its own: each end is bounded but for the node's own `shutdown` (E before it, E and 1 s after it), and cutting `shutdown` short would leave its data unsecured. `atexit` calls it on the module's registry, so no thread of liminal-net attaches to an interpreter that is finalizing (inferred). An abandoned thread is still inside the node's Python code and is not waited for; like a request or a lent callable still running at exit, it is as today. A test calls `_end_all` on a private registry.
- A thread's priority is the operating system's. It does not decide who gets the interpreter's lock, and the report's `priority.note` says so for a Python node.

## 10. The faults in 0.1 and 0.2 as built, and what a peer sees change

### 10.1 The faults, each with its fix

| | fault | where | fix in 0.3 | step |
|---|---|---|---|---|
| 1 | A frame over 16 MB ends the writer task. That request is never answered, nor any later one on that synapse, and nothing is refused with words (against decision 45). On Windows the caller then waits for ever (inferred; X8) | dendrites.rs:78-84; synapse.rs:98-106, 171; frame.rs:12-19 | requests are encoded by the caller, answers by the worker; too large is `NetError::Frame`, or an error record with the same words; writers only write bytes (7.11) | S2 |
| 2 | The receiver's functions run under one node-wide lock that async threads also take. While one function runs, every other synapse's task of that node waits for the lock on an async thread; with as many such synapses as the runtime has async threads nothing of that runtime runs: no request is answered, no synapse admitted. A panic in one poisons the lock for every later stimulus | node.rs:96, 431-448; dendrites.rs:212; synapse.rs:231-233 | a slot and a lock per chain, taken only by the holder of the in-right; every call caught; no async thread touches a chain. A slow function still holds the node's other signals, since the node takes one at a time (decision 95; Q11) | S1 |
| 3 | A signal whose target cannot be reached is dropped with no count, after its number was given | node.rs:485, 554-556 | it waits in the target's line, bounded; what is dropped is counted and told | S2 |
| 4 | One target that is busy, silent or securing its data holds up every other target, again for each signal: up to 2 s, 5 s per handshake step, or 30 s | node.rs:553-557; transport.rs:73-87; handshake.rs:62-67; axon.rs:109, 133-137 | a line and a reacher per target; nothing awaits a reach on the way of a signal | S2 |
| 5 | What is fired before `serve` piles up without limit; at the end the fire loop is aborted, and what is queued, and what the functions hold, is dropped silently | node.rs:139, 213-214, 264-266, 285-290 | before `serve` only a state's latest is kept; at the end the chains are flushed before and after `shutdown` and the lines get their time; the rest is counted in the reason | S2, S3 |
| 6 | After `stop`, synapses already open go on answering and integrating while and after `shutdown` runs; the stage is checked at admission only, and before `admits` only | node.rs:250-271; dendrites.rs:26, 44, 66 | the end closes them first, under the lock that registers them; nothing arrives after; `shutdown` runs with the in-right held | S3 |
| 7 | No line has a cap: each synapse's signals, the firing channel, the writers' lines | dendrites.rs:77, 85; node.rs:139; synapse.rs:82 | sections 7.1, 7.5, 8. The control frames' lines stay uncapped with the requests (decision 109) | S1, S2 |
| 8 | A closing synapse waits for its line without a bound, and with it the node's count of synapses and its `detach(peer)`. Its release of handles releases nothing (7.13 step 3) | dendrites.rs:181-191 | 7.13: the bookkeeping at most E after the close | S1 |
| 9 | Integrates of different synapses run at the same time, against decision 95 | dendrites.rs:86, 210-233 | one in-right per node | S1 |
| 10 | `link`, `unlink` and `tune` change the links at once while fired signals still wait; the tune test passes by being quicker | node.rs:385-425; tests/network.rs:513-528 | every link op takes its place in the line of fired signals | S2 |
| 11 | `tune`, a re-made link and a replaced receiver chain discard what the old functions held: a throttle's held latest is lost | node.rs:127-132, 394, 415-425 | what was held passes through the new chain, ahead of it | S2 |
| 12 | Two reaches of one target at the same time both connect; the second replaces the first in the table | axon.rs:96-131 | one attempt in flight per target | S2 |
| 13 | A function that holds a value whose time has come can be passed by a newer stimulus first, when a late timer and a new message race | functions.rs:393-403; node.rs:545-553 | a chain is ticked up to a stimulus's time before the stimulus is applied; a stimulus carries the time it was fired; a tick never runs ahead of waiting items | S2 |
| 14 | A receiver's function that still holds something when its synapse closes never releases it; its timer runs only while a synapse from that source is open | dendrites.rs:212-217 | the closing marker flushes the source's chains; `in_due` is the node's | S1 |
| 15 | A link made again restarts its numbers at 1; a receiver's `in_order` from the earlier link drops them as already passed | node.rs:394-401; functions.rs:520-522 | the receiving end reads a number below its last, or a changed origin, as a new start: its chain is flushed and starts again, and the first arrival tells what it can (7.6). Whether a link made again by the same node should go on from its number instead is Q8 | S1, S2 |
| 16 | The blocking forms need a tokio context on the calling thread | synapse.rs:306-308 | `block` uses the node's captured handle | S1 |
| 17 | The bridge's `fire`, `start`, `stop`, `lend`, `Remote.detach` and several getters call into the network while they hold the interpreter's lock | bridge:510-514, 541-549, 571-574, 612-649, 713-715, 723-742 | the wrapper and the helper of 9.2 | S2 |
| 18 | A message nested deeper than serde_json reads back (more than 127 levels: serde_json-1.0.154 src/de.rs:63, 1375-1376) closes the synapse at the reading end | frame.rs:48-50; dendrites.rs:89-95; synapse.rs:109 | it is refused at the sender's end, with words, on all three deliveries. The limit itself, `MAX_DEPTH` (120), is new and stricter than the fault needs (Q13) | S2 |
| 19 | A panic's words carry a tokio task id, so they differ from run to run (inferred) | dendrites.rs:119-122 | the panic's own words | S2 |
| 20 | Two hops to the blocking pool per stimulus, one more per stimulus a function passes and per tick | dendrites.rs:223-231; node.rs:468-475 | functions and `integrate` back to back on the thread that serves the node: at most one wake per burst | S1 |
| 21 | `shutdown`, `admits`, `attach` and `detach` share tokio's blocking pool with requests that have no cap and no timeout: a pool filled by hung requests keeps the data from being secured | node.rs:252; dendrites.rs:44, 111, 206 | the end's steps run on the node's own thread or on a thread started for the end; a `detach` that cannot begin is skipped and counted; local admission's wait for `admits` is bounded | S3, S5 |
| 22 | LIMINAL_QUIET_SECONDS with a huge value (`1e300`) panics in `Duration::from_secs_f64`; the bridge's `timing` has the same hole | lifecycle.rs:42-44; bridge:434-438 | the rule of 3.1 | S0 |
| 23 | `in_order` adds one to a number a peer wrote: `u64::MAX` overflows, a panic in a debug build, inside the receivers' lock | functions.rs:517, 528 | saturating; 7.6's arithmetic too | S0 |

Left as they are, and to be rows of NETWORK.md §10: requests in progress and the frames they wait in (decision 109); what `gate` and `in_order` hold until row 110 is confirmed; state values outside the node's ceiling, and the maps of G5 that are never pruned.

### 10.2 What a peer sees change, with the wire as it is

A node in another language that follows NETWORK.md must know these. Each names the row or sentence it touches. Those marked with a question are not written into NETWORK.md before the user's yes (section 14); the others follow rows already decided.
- §4.8, row 49 (Q9): a receiver's functions run when the node takes a stimulus. State values are merged in the line and stream items dropped from a full line before them (decisions 104 and 97), so "every stimulus" means every stimulus that is taken.
- §4.8, rows 52 and 107: a tune, a link made again and a node's end pass on what the sender's functions held.
- §4.11, row 95 (Q10): a node's `integrate` that waits, through a request, for the same node to take another signal, hangs.
- §4.11's list of faults, NETWORK.md:547 (Q11): what 0.3 mends about a slow receiver function is that it no longer holds async threads, requests and admissions; it still holds that node's other signals.
- §5.2 (Q13): a message nested deeper than 120 levels, as framed, is refused like one over 16 MB. This is a new limit: 0.2 reads 127 levels.
- §5.4, row 92: on one connection, control frames are written before signal frames that waited; `link`, `unlink` and `tune` are answered when in effect, which is after every signal fired before them.
- §5.4, row 91 (Q8): whether a link made again goes on from its number. Until the yes it starts at 1, as today. A dropped stream item keeps its number (decided with 91 and 97). A `fire` with `seq` 0 is taken as unnumbered, and a number that arrives twice is taken once.
- §5.4, NETWORK.md:621 (Q12): a request in flight when its caller detaches is answered, as today; the sentence says it fails. Requests in flight when the reached node begins to end fail `Closed`, where today they are answered during and after `shutdown` (fault 6; decision 107).
- §4.3, rows 107 and 27: the end's order and bound (7.12).

## 11. The tests

No test starts a process. No new test asserts that something was quick enough, or that something did not happen within a pause. Three tools make that possible:
- **Pure tests** call `Line`, `QueueState`, `Alarm`, `Idle` and `Chain` with the time passed in, as tests/functions.rs already does.
- **A manual `Process`** (`Process::manual`): no server, no watch, a manual clock. The test runs turns (`run_one`), passes (`check`) and time (`advance`) itself.
- **Gates.** A test hillock whose `integrate` (or `own_thread`, `admits`, `attach`) waits on a channel the test holds; the gate opens when it is dropped. A positive outcome on real threads is awaited with `eventually` (tests/common/mod.rs:167-176) under a generous limit; an absence is asserted as a count read from `report()`.
Each test has its own `Process`. Tests that call `run_one` do so from a plain thread or `spawn_blocking`.

### 11.1 The existing tests

The 48 Rust tests (26 in tests/network.rs, 12 in tests/functions.rs, 9 per platform in tests/vectors.rs, 1 in key.rs) and the 12 Python tests keep every test function as it is.
- tests/common/mod.rs: `TempDir` carries a `Process::new(Delivery::Wire, Limits::default())`, and `config()` (53-64) hands it to every node it builds. The suite therefore keeps crossing the pipe, which it exists to prove (tests/network.rs:1-2).
- A new `liminal-net-py/tests/conftest.py` sets LIMINAL_NET_DELIVERY to `wire` before the module's global process is first used. test_bridge.py is not touched.
- The two coalescing tests (tests/network.rs:444-459; test_bridge.py:208-217) keep their fixed sleep, so a stall longer than about 300 ms can still fail them, as today. What 0.3 removes is the wrong content: whatever was received by then is the first and the latest, never a middle value or the latest alone.
- The tune test (tests/network.rs:513-528) stops being a race: the tune takes its place behind what was fired before it.
- `a_wrong_key_is_refused_and_never_counted` (tests/network.rs:120-137) already accepts `Closed`. `one_node_per_name` (tests/network.rs:187-202) keeps its words: the twin's bind fails, as today.

### 11.2 New Rust tests

**tests/queue.rs** (pure):
- Q1 a source keeps its order and sources take turns: a1 a2 a3 and b1 are taken as a1 b1 a2 a3.
- Q2 a source's order holds across its signals; a closing marker is taken after everything that arrived on its synapse.
- Q3 a state signal keeps its oldest and its latest: of 100 values, value 0 and value 99 are taken, 98 are coalesced.
- Q4 the latest never overtakes: v1, 200 stream items, v2, v3 are taken as v1, the 200, v3.
- Q5 a state signal enters a full line, and nothing is dropped.
- Q6 a full line drops its oldest and the gap is told: cap 3, numbers 1 to 5; 3 4 5 are taken, 'lost 2' before the 3, and 5 = 3 taken + 2 told.
- Q7 the byte cap; one stimulus larger than the cap fits an empty line and is the next victim; every floor and ceiling of section 8 (`clamped`: K never above min(4, cores), `autonomous_max` never above 16); `from_lookup` for `Limits`, `Timing` and `Delivery` on good and missing values and on each bad kind: empty, not a number, negative, not finite, `1e300`, above a year, a fraction for a count, a count above `usize`: each is the default, and none panics.
- Q8 in a full line of two signals (4 `events`, then 1,020 `lines`), one more `lines` drops the oldest event (decision 97).
- Q9 the node's ceiling: lines under their caps and together over it, arriving and outgoing; the line that holds most in the measure that is over gives up its oldest: by count, and by bytes with one 16 MB stimulus against 1,000 small ones.
- Q10 telling: a turn that takes seven stimuli, each after a gap, with no later stimulus: the first gap is told before its stimulus, the other six at the end of that turn; nothing is owed after it.
- Q11 the numbers: a changed origin, or a number below the last, with numbers 1 to 8,976 never arrived and the first arrival 8,977: exactly 8,976 are told when the origin changed, whatever the last number was, and the chain is flushed. `seq` 0 is taken, nothing is owed and nothing panics; a repeat of the last number is dropped and counted, nothing is told and no chain is flushed; `u64::MAX` after 1 saturates.
- Q12 the account, as lists of steps numbered from 1: 99 late steps then on time never reaches S; 100 reach it; 3 late and 3 on time for 10,000 steps never; 3 late and 1 on time first reaches it at step 198; never below zero; with S floored to 1 a node moves at its first late step and not before.
- Q13 fired signals and link ops leave first in, first out; the four decisions of `fire` in each of Shared (step S2), Ending (S3), and Moving and Autonomous with the thread in a call and not (S4); a helper that leaves something applies the wake rule; a helper at the full cap goes on until it has popped one.
- Q14 the move in the state machine: a server holds the right, `taking` is set, the server gives it back before its next take, the own thread claims, and the takes go on in order with none twice.
- Q15 a drain cut after five of ten state values, then a tick at a late time: the link still gives the first and the latest. A receiver `debounce(0.5 s)` holding r1 with r2 and r3 waiting from 0.3 s and 0.4 s: one value is integrated, r3.
- Q16 two synapses of one source: 99 pushed on the newer before 97 and 98 on the older: taken as 97, 98, the older's marker, 99; nothing told lost.
- Q17 the identity given = taken + told + owed + waiting + above, checked after every step of a scripted run with drops at both ends and a tail drop.
- Q18 a move crossed by the end: T3, then N1, then the new thread's first step: it returns, `taking` was never set, the measured place is back once (given back twice it is still one), the mode is Ending; `take_thread` during a measured move makes it asked.
- Q19 the watch's alarm: an arm stored between a pass's step 0 and its step 5 is kept, and the pass does not end the watch; a pass that computes nothing with no arm ends it; an arm never raises `wake_at`.
- Q20 the idle servers: two idle, one notify, the other's time running out at that instant: one works, one waits on, `idle` is 1; a spurious wake changes nothing; `idle` never goes below zero and no claim is left without a server.
- Q21 the measure counts only what can be taken: entries older than W in a part that is not attached, and in a part behind an older synapse's part, leave the account at 0; the same entries in an attached front part raise it.
- Q22 a take on the own thread: with `fired` not empty and no drain since the last take it takes nothing; after one drain it takes whatever `fired` holds; `in_call` is set in the taking section, and a fire after that section finds it set.
- Q23 the end in the state machine: `node_code` refuses every signal call once `take_by` has come, and after `steps`; the own thread sets `steps` at `take_by`; at `end_by` with a call running the holder is abandoned and what it had in hand is counted, with none running the right is taken and `off_thread` set for an own thread; the counts of N4 (lines, `released`, a chain's hold, owed as `not_told`); `detaching` raised at the close and lowered at the return; a pending `detach` skipped at the bound is never run.
- Q24 `Line::unpop`: a stream entry returns to the front with its ticket; a state value returns unless a newer one waits.
- Q25 released runs: a take gives at most one `integrate`; a run of 40 is taken in order across turns and before any later entry of its source; `taking` between two of them hands the rest to the new holder; a marker completes only after its flush's run is integrated.

**tests/functions.rs**, added: F1 `flush` for each built-in; F2 `in_order` moves on at a `gap`; F3 `held` for each built-in before and after a flush (a closed `gate` keeps its count); F4 `catch_up` releases what was due before a stimulus's time, and the stimulus then meets the chain after it; F7 `in_order` with the numbers `u64::MAX` − 1 and `u64::MAX` does not overflow. **tests/frame.rs**, new: F5 over generated values (escapes, control characters, non-ASCII, floats with 17 digits and exponents, integers of 19 digits, depths to `MAX_DEPTH`): `measure(v)` equals `serde_json::to_vec(v).len()` and `estimate(v).upper >= measure(v)`; F6 the depth is of the message as framed: for a signal's data, an answer's value, a request's argument and an error's `accept`, the deepest value that passes and the next one, which is refused (data nested 119 passes and 120 does not; an argument 118 and 119; all at 121); `estimate` walks a value nested 100,000 deep without recursion.

**tests/process.rs** (manual, but P14):
- P1 turns: two nodes with 40 and 1 waiting; the order of integrates is a1 to a16, b1, a17 to a32, a33 to a40.
- P2 with R nodes ready and one server, the last is taken in the R-th turn.
- P3 a stuck server is replaced: a thread of the test holds a turn of A in a gate; `advance(server_stuck)`; `check()` reports one replaced; `run_one()` serves B.
- P4 a receiver's timer: a receiver `throttle` holds a value; `check()` at its due time makes the node ready and `run_one()` integrates it. With A held in `integrate` instead: two `check()` calls report `made_ready` 0, and opening the gate integrates the release.
- P5 by measure: a node with one stimulus and no turn run is late from W; 99 steps do not move it; the 100th reports it moved, and its thread is named `liminal-node-<name>`.
- P6 late for 99 steps, then served: it never moves.
- P7 a stall is forgiven: `advance(30 min)` and one `check()` add one step.
- P8 time inside its own code does not count: A is held in `integrate`, a second stimulus for A arrives during the hold, nothing else is ready, `server_stuck` is an hour: 200 steps leave A's account at 0 (also after its server was replaced, as in P3). With B waiting longer than W behind it, both accounts rise; at step 100 A, the holder, moves first, and B's account is 0.
- P9 two nodes late together: one moves at step 100, the other needs 100 more.
- P10 the limit: with `autonomous_max` 1 the second node stays shared, its note says `needs a thread: 1 of 1 taken by measure`, and it takes the place when the first ends, with no advance of N7's second. With `autonomous_max` 0 a declared node still gets its thread.
- P11 output can earn the thread: a node whose fired signals wait is late; a firing thread that had to help marks the step late.
- P12 a busy pool under W: six nodes with short integrates and one server, every wait under W: no account rises in 1,000 steps.
- P13 a node that is being served with `in_count` above B is not late.
- P14 a real `Process` with `server_idle` 0 and two nodes: both are served; `process.servers()` returns to 0 and `process.watching()` to false, awaited; a server held in a gate is replaced and the replacement ends.
- P15 a release beside a waiting node: A's receiver `gate` opens on a take and releases 40, B has 1 waiting: the order of integrates is a1 to a16, b1, a17 to a32, a33 to a40.
- P16 the table: a second node of the same address is refused with the bind's words and the first entry stays; an Ended node's entry is replaced by a successor; a node removes only its own entry, so an old node leaving after its successor entered removes nothing; a wire process keeps the table empty.
- P17 a turn does not spin: two raw connections of one source name (as tests/network.rs:157-185 drives a handshake), the first open with a receiver chain holding a value, the second with entries that cannot be taken yet; at the chain's due time `check()` makes the node ready and `run_until_idle()` returns, with the held value integrated.

**tests/axon.rs** (served nodes, wire, a private process; counts read from `report()`):
- X1 `fire` never waits: one target not reached, one that accepts and stays silent, one whose `integrate` is held; 10,000 fires return; each link's counts add up.
- X2 one unreachable target delays no other, with `hello_wait` set to 60 s for the silent target: the good receiver gets 20 signals in order, and the silent target's line shows 20 waiting.
- X3 `lines` 1 to 99 and then `done` reach one target in that order.
- X4 on a manual process: fire, then send a tune, with no turn run: the tune is not answered yet; `run_until_idle()` sends the signal through the old functions, applies the tune and answers it. The same for link and unlink.
- X5 what a tuned link's throttle held passes through the new chain with no later fire: the op's own drain moves it. A tune of a gate holding 1,000 moves them in pieces, in order, and a line fired right after the tune arrives after all 1,000.
- X6 a full target line: the sender fires more than the cap while the target's way is still `Reaching` (the receiver is not served yet): the oldest are dropped and counted at the sender, exactly the number over the cap. Then the receiver is served on a manual process and its turns are run: it takes what was kept, is told the gap before the first, and the identity holds at rest.
- X7 a signal over 16 MB is counted `too_large`; the writer lives; later signals arrive and the gap is told.
- X8 an answer over 16 MB, an answer nested 121 levels, one nested 128 levels (the least that 0.2 could not read back: fault 18), and an error record whose `accept` is over 16 MB are each an error record with the words; the synapse lives. X9 a request over 16 MB is refused at the caller.
- X10 before `serve`, 100,000 fires keep one value per state signal; a fire at the instant `serve` starts is not lost; the first link gets the latest.
- X11 a request is answered while that peer's line is full and `integrate` is held.
- X12 a cycle: caps of 4, both integrates fire back, 1,000 fired; both nodes answer a request; per link the numbers add up.
- X13 two reaches at once make one synapse; a reach that waits behind another fails after its own `reach_wait` (manual clock).
- X14 while a receiver function of one source is held on a gate, with as many other sources sending as the runtime has async threads: a request to that node is answered, a new synapse to it is admitted, `Node::receive` for another signal returns, and a second node's stimuli are integrated (0.2 fails the first two: fault 2). A function that panics is counted and the next stimulus is integrated. A slot replaced in the middle of a turn: the old gate's 500 are integrated before line 501, with no later stimulus needed.
- X15 one at a time: two sources fire at a node whose `integrate` raises a flag on entry and lowers it on return and records the thread that holds the in-right; a counter in `node_code` (checks on) fails the test if two signal calls of one node overlap.
- X16 a `detach` asked while 40 signals wait is written after them; a request in flight when its caller detaches is answered (Q12's first behaviour).
- X17 a sender's timer on a manual process: a state's held latest is released when `check()` at its due time makes the node ready and `run_one()` ticks.
- X18 a link made again while a throttle holds a value, with no later fire: the value arrives. Its numbers start again and the receiver reads a new start (Q8's first behaviour; with the yes they go on). A link made again over a second synapse stays when the first synapse closes.
- X19 `unlink` with 30 signals waiting for a target not reached: `ended` shows 30 for that target.
- X20 a raw peer that stops reading after its hello (as tests/network.rs:157-185 drives a handshake), 50 signals waiting for it, then a request to it: when it reads again, the request frame comes before the waiting signals.
- X22 a state value over 16 MB: counted `too_large`; the receiver keeps the value before it; a link made afterwards gets the value before it.
- X23 a state signal linked by a third node (a raw peer's `link` naming another node): 100 values are taken as a stream, none coalesced.
- X24 `admitted` that cannot be written: `admits` is held on a gate, the raw peer drops its connection, the gate opens: `synapses` stays 0, neither `attach` nor `detach` is called, and the node ends with `nobody came within ...`.

**tests/ending.rs** (manual clock unless said):
- E1 the order at `stop`, with two peers linked for a state: five queued; the record is five integrates, then each `detach`, then `shutdown`, and nothing after, on every run. A state fired just before `stop` and another fired from `shutdown`, inside the coalesce time: both peers' last value is the one from `shutdown`, and the reason has no tail. A fire from a request still running after the last hand-over closed is in `fired_after` and reaches nobody. It ends with no `advance`, for a shared node and for a declared one.
- E2 a state held by the 250 ms throttle before `stop` goes out at the end.
- E3 the bound: `integrate` held, three more queued, `stop`, `advance(end)`: `shutdown` has run, and the reason ends `; 3 signals not integrated; its data was secured beside a call still running`.
- E4 a request in flight at `stop`, and one sent after it on a synapse that was open, fail with `Closed`.
- E5 a closing synapse is bounded (step S1): `integrate` held, the peer detaches, `advance(end)`: `synapses` is 0; nothing is dropped; when the gate opens the rest is integrated in order and `detach(peer)` runs after it. The peer lent a callable with `lend_to` over its Remote: the peer's `lent()` falls when that Remote closes.
- E6 a natural end's reason is exactly `no synapse for 0.2 s`.
- E7 E3 with a receiver chain that releases three (the first is held in `integrate`, two wait in `released`) and three more queued: the reason says `5 signals not integrated`; with a second chain still holding one value at the bound, 6. The record shows no `integrate` and no `lost` after `shutdown` started, though the abandoned call returned; what it fired is in `fired_after`.
- E8 a backlog that outlasts E with calls that return, on a shared server and on an own thread: `shutdown` runs by E on the node's thread, the reason says `N signals not integrated` and has no `beside`. A chain that releases 100 just before `take_by`, with integrates that return: no `beside`, and the rest counted. An own thread that is between two calls at `take_by` takes the end itself.
- E9 `admits` held on a gate, `stop`, the gate opened: the reach gets retry and `synapses` is 0 at the end.
- E10 a small runtime (two blocking threads) filled with gated requests, then `stop`: `shutdown` still runs, and the reason says `1 peer not detached`; and a target never reached: after `advance(end)` twice the reason says `N not sent`.

**tests/autonomy.rs**:
- A1 declared: every `integrate` and `shutdown` runs on `liminal-node-x`, from the first stimulus; a request runs on another thread; the report says `declared`; its end needs no `advance`.
- A2 by measure: numbers 0 to n are integrated in order, none twice, across the move; later ones on the node's thread; after 1,000 idle steps it is still `own`.
- A3 `take_thread` from a request returns true, and the thread takes over after the call in progress.
- A4 the priority, read back inside `integrate`: Windows gives above normal for High and below normal for Low. Linux, compared with the value read on another thread of the test: 5 more for Low (or 19); for High either 5 less, or the same with its note.
- A5 an own thread held in `integrate`, on a manual process: 127 signals fired from another thread wait (`report().waiting.fired` is 127). The 128th makes the caller help: one batch of 64 is sent, 64 wait.
- A6 during the move: the shared holder is held, `take_thread` is called, a fired signal is received before the gate opens.
- A7 `serve` dropped: the node's thread ends, and a measured place is back. An `own_thread` that returns without calling `run`: the node is shared again with its note, and the place is back.
- A8 an own thread at Low, gated outside the node's code: 10,000 fires from two threads; `waiting.fired` is never above 256 + 2, and input arriving meanwhile is taken.
- A9 others send after `wait` (Q6's first behaviour), continuing A5: `advance(wait)`, `check()` reports the node made ready, and `run_until_idle()` sends the 64 while the gate is still shut; a state's held latest goes out the same way at its time; a `tune` sent meanwhile is answered by that turn. With Q6 answered yes: the first signal is sent at once.
- A10 the declared start, with a test hillock whose `own_thread` waits on a gate before it calls `run`: (1) a fire while Starting, the claim, a fire after it, then a link: the link gets the later value; (2) a local reach admitted right after the claim finds the latest; (3) `stop`, and `start_wait` running out, each against a claim: the node either was claimed and goes on, or was cancelled and the claim changed nothing, never both. A refused thread start (a test hook on the spawn): a measured node stays shared, its note says why, nothing was lost; a declared node's `serve` returns `could not start its thread`, and a second `serve` is allowed.

**tests/delivery.rs** (step S5):
- D1 runs each scenario as one function of a `Delivery`, on wire, encoded and direct, and requires the three results to be equal, and equal to the expected one. The scenarios: an echoed value with its key order, an integer of 19 digits and a float with 17 digits; a refusal's kind, words, accept and status; a method outside the interface; a panic and its words; `admits` refusing; a wrong key (`Closed` on all three) and never counted; a twin served and refused with the same words, the first node still answering; 20 stream signals; a new link's latest; sender functions; receiver functions with numbers from 1; tune and unlink; a lent callable called and released; handles released with their synapse; the ending reason; a successor served by a watcher of `stage()` as soon as it reads Ended; requests, answers and signals of 16 MB − 1, 16 MB and 16 MB + 1 bytes as framed; the nesting rule at F6's boundaries for a signal, a request argument and an error's `accept`; 20 nested requests between two nodes; a request that comes back into its caller: the inner call runs on another thread than the outer on all three; a request whose callee begins to end while it runs. On manual processes, with the clock advanced by the test: a state coalesced to first and latest; a callable lent over a synapse still held by a request that outlasts the quiet time; a quiet synapse closing; a node securing its data waited for and started again. In the direct run, with `serve` spawned and `reach` called at once (tests/network.rs:43-45), the remote says `Direct` and a blocking request ran on the calling thread, every time; after the twin was refused a request to the first node is still `Direct`.
- D2 each blocking form (`request_blocking`, `call_blocking`, `reach_blocking`, `run_one`) called from an async task panics alike on the three deliveries, and the callee never ran.
- D3 a receiver that begins to end while its sender still fires 40, on each delivery with its own stated result. Wire: the sender's way becomes `Unreached` when its Remote reads end-of-file, the 40 wait in its line, and are in `ended` when the link goes: none is in no count. Direct and encoded: a push that finds the receiver ending stays at the front of the line, a reacher starts, and the value is delivered to the successor; a reacher that gave up starts again on the next push.
- D4 a local synapse whose `attach` is held on a gate: its entries older than W leave the account at 0; a request that waits for it gets `Closed` when the node is stopped, and the node's code never ran; a request begun after the node began to end gets `Closed`.
- D5 a callable lent with `lend_to` over a local Remote: after `detach`, `closed()` returns and the lender's `lent()` falls, as tests/network.rs:565-580 shows on the wire.

### 11.3 New Python tests

Each uses a private `liminal_net.Process`.
- test_delivery.py: B1 a request, an exception and ten stream signals give the same results on `direct`, `encoded` and `wire`; B2 a Python-to-Python request in one process runs on the calling thread in direct delivery; B12 a request that comes back into its caller while the caller holds a `threading.RLock`: the inner handler's `acquire(blocking=False)` gives False on the three deliveries.
- test_autonomy.py: B3 an `autonomy` attribute gives one thread name, one `threading.get_ident()` and one thread-local for every `integrate` and for `shutdown`; B4 a Python `integrate` held on an event does not hold a second node's signals, nor a request to the first; B5 `lost(peer, signal, n)` is called with the count; B6 `fire` from two Python threads keeps each thread's order; B7 `take_thread` and the keys of `report`; B8 `_end_all` on a private registry stops a started node, waits for a `shutdown` that takes longer than `end`, and skips a node never started; B11 when `_end_all` has returned, the node's own thread has returned from `own_thread`.
- test_rules.py: B0 `liminal_net._rule_checks` is true, else the file fails. B9 `Node._enter_held()` panics, from a plain Python thread and from inside a Python node's handler reached by a direct request; every public method and getter of `Node`, `Remote` and `Process`, both constructors, `digest` and `address` are called once with the check on and none panics. B10 `_probe_on_async()` says true; every Python `request`, `integrate`, `admits`, `attach`, `detach`, `shutdown`, `lost`, lent callable and receiver function, on the three deliveries, calls `_on_async_thread()` and records false.

## 12. The landing steps

Each step is a commit series that leaves every test passing: the 48 and the 12 as they are, and the new ones of the steps before it. A test is named in the step that first has everything it needs. `Node::report()` lands in parts, each with the structure it counts.

- **S0. Groundwork, no behaviour change but faults 22 and 23.** serde_json's `float_roundtrip` (decision 108). `frame::read_sized`, `write_bytes`, `estimate`, `measure`, `MAX_DEPTH`. `flush`, `gap`, `held`, `catch_up` in functions.rs, and the saturating number. `Limits` with its rule, floors and ceilings; `Timing::from_env`'s rule; `Delivery`; `Process` with its table's rules, clock and limits only. rules.rs and the `rule-checks` feature. `NodeConfig.process`; the test helper and the bridge's constructor gain it; the bridge's `Process` class; conftest.py. Tests: F1 to F7, Q7, P16. Beside it, in a commit of its own, the key file's permissions (Q7).
- **S1. The arriving side.** queue.rs: `Line`, `SourceLine` with its parts and `released`, the arriving half of `QueueState`, `Alarm`, `Idle`. process.rs: the servers, the watch's due times and stuck servers, the manual process. dendrites.rs: admission registered and then counted under L4; a `fire` op is pushed into the queue; the per-synapse channel and integrate task go; the closing marker and its bound (7.13). A slot per receiver chain; `lost`; kinds learned at `link`; `node_code`; `block` on the captured handle. `report()`: `waiting`, `incoming`, `waits`, `handling`, `failed`, `kept`, `process`. `fire_loop` stays as it is for this step. Faults 2, 8, 9, 14, 15 (receiver's half), 16, 20. Tests: Q1 to Q11, Q16, Q17 (receiver's half), Q19, Q20, Q24, Q25, P1 to P4, P14, P15, P17, X11, X14, X15, X24, E5.
- **S2. The output side.** The line of fired signals with the link ops in it; the `axon` lock; drain steps with `carry` and `passed`, and the helping rule; a branch, a line and a reacher per target, and the close rule; the writers write bytes, control first; one attempt per target and every reach's own deadline; the 16 MB and nesting rules with words; `fire` before `serve`; the node's ceiling; the bridge's wrapper, helper and check, and CI's bridge job built with `rule-checks`. `report()`: `outgoing`, `ended`. `fire_loop` goes. Faults 1, 3, 4, 5 (first half), 7, 10 to 13, 15, 17 to 19. Tests: Q13 (Shared), Q15, Q17, X1 to X10, X12, X13, X16 to X20, X22, X23, B0, B9.
- **S3. The end.** Steps N1 to N7, `detaching`, the end thread and the reason's tail. `report()`: `left`, `fired_after`. Faults 5, 6, 21. Tests: Q13 (Ending), Q23, E1 to E4, E6 to E10 (E8's own-thread half and E1's declared half with S4).
- **S4. The measure and the node's own thread.** The step and the move in the watch; autonomy.rs; `autonomy`, `own_thread`, `take_thread`, `join_thread`; the guards; the priority; `report()`: `thread`, `why`, `since`, `priority`, `note`, `late`, `sure`, `busy_share`, `in_call_for`; `state_line`; `libc` for Unix targets; the bridge's `autonomy`, `lost`, `take_thread`, `report`, `_end_all`. Tests: Q12, Q13 (Moving and Autonomous), Q14, Q18, Q21, Q22, P5 to P13, A1 to A10, B3 to B8, B11, B10 (wire).
- **S5. In-process delivery.** local.rs: the table's two looks in `reach`, admission without a pipe, the codec and the local way's writer. The local kind of `Remote` for request, call, link, unlink, tune, release, detach, its close and the quiet close; `in_flight`; the hop at depth 16 and on coming back into a node. `Remote::delivery`, and the bridge's `Remote.delivery`. The bridge's `request` uses the blocking form. Tests: D1 to D5, B1, B2, B12, B10 (all three). The workspace becomes 0.3.0; CHANGELOG, README and NETWORK.md follow (section 13).
- **S6. With hw-stats (decision 98).** `Hillock::pulse`, `Node::pulse_soon`, `pulse_due`, after Q5 is answered. Tests: a pulse on a shared server and on the node's thread, at manual times; it rests on `None` and starts again on a link.

Before S5, nodes of one process still meet over the pipe, as today. `Process::global()` reads the delivery from S0 on; a node enters the table from S1 on (T1), and nothing looks in it until S5.

## 13. What changes in each source file

**New**
- `queue.rs`: `Line`, `Entry`, `Lane`, `SourceLine`, `Part`, `Run`, `Arrived`, `OutItem`, `Queue`, `QueueState`, `Mode`, `Holder`, `OwnThread`, `SynIn`, `LinkIn`, `Fired`, `LinkOp`, `Left`, `Alarm`, `Idle`; the pushes and pops, `unpop`, `victim`, `ceiling`, the gap and `owed` arithmetic, `server`, `others_send`, `admit`, the decisions of `fire`, the take and `node_code`'s section, the end's hand-over and counts, the measure's step. Pure: every function takes `now`.
- `process.rs`: `Process`, `Shared`, `Pool`, `Server`, `Clock`, `Check`; the table's enter and leave; the server's loop and the turn; the watch's pass; `make_ready`; the manual calls; the servers' and the watch's guards.
- `autonomy.rs`: `Autonomy`, `Priority`; starting and cancelling a move; the declared claim; the own thread's function, loop and guard; the end thread; giving a place back; the priority calls for Windows and Linux.
- `local.rs`: finding a node of this process; admission shared with the dendrites; the codec of the encoded mode and its writer; the size rule for a direct request; the `running` list and the hop.
- `rules.rs`: `ENABLED`, `on_async_thread`, `set_attached`, the check at the public entries.

**Changed**
- `node.rs`: `NodeConfig.process`. `Inner` loses `links`, `last_state`, `firing`, `firing_inbox` (94-95, 105-106, 139) and gains the fields of 3.5. `fire` (285-290) becomes 7.5. `route`, `links_due` and `fire_loop` (479-559) become the drain step and the tick under `axon`. `integrate`, `receive_through`, `receive_tick`, `receiver_due` (431-476) move into the turn. `add_link`, `remove_link`, `tune_link` (385-425) become the application of a link op (7.10). `set_receiver` (127-132) swaps a slot, sets `retired` and wakes the node. `serve` (205-272) becomes T1 or T2, today's loop, and 7.12, with a guard for its drop; its last steps keep today's order of listener and stage (262-263). `call_blocking` (343-345) reaches first. New: `take_thread`, `join_thread`, `report`, `state_line`, `linked`, `pulse_soon`.
- `dendrites.rs`: admission (26-74) becomes one function shared with local.rs, registering under L4 after `admits` and counting after `admitted` was written. The signals channel and the integrate task (85-86, 210-233) go; a `fire` op calls `Queue::arrive` with the frame's length. Answers are encoded on the worker, inside `catch_unwind` (105-127). Link ops (142-173) are built and pushed (7.10). The reading task also ends on `SynIn.stop`. The end (180-191) becomes the synapse-end task of 7.13; the release of handles at 185 goes.
- `synapse.rs`: `Out` gains `kind`, `in_flight`, `carries`; `tx` carries bytes (36, 82). The writer (98-106) writes control frames first, then its branch's signals while the way is its own, and a detach after what waited; a failed write gives the entry back. `Out::close` (53-64) applies the close rule to the branch it carries, and is run for a local Remote by the synapse-end task. `ask` (163-178) encodes on the caller. `request`, `request_blocking`, `call_handle`, `link`, `link_with`, `unlink`, `tune`, `release`, `detach` each get their local form. `send` (273-275) goes. `held` (72-76) counts `in_flight`. `block` (306-308) uses the node's handle. The quiet watcher (121-139) reads the process's clock. New: `delivery`.
- `axon.rs`: `reach` (95-161) reads its deadline first, takes `reaching[target]` per attempt, reads the process's clock, and looks in the table before and after a connect unless the delivery is wire. `Branch`, `BranchLine`, `Way` and the reacher.
- `functions.rs`: `flush`, `gap`, `held` on the trait with defaults; their forms and `catch_up` on `Chain`; `flush` for `Throttle`, `Debounce`, `InOrder`, `Batch`; `gap` for `InOrder`; `held` for the five that hold; the saturating number in `InOrder` (517, 528); `hold_cap` (Q1). `Stimulus` and what the eight built-ins do on `apply` and `tick` stay.
- `hillock.rs`: `autonomy`, `lost`, `own_thread`, and with S6 `pulse`, each with a default.
- `lifecycle.rs`: `Limits`, `Delivery`; `Timing::from_env` (37-47) takes the rule of 3.1. `Timing`'s fields, `Stage` and `seconds` stay.
- `frame.rs`: `read_sized`, `write_bytes`, `Size`, `estimate`, `measure`, `MAX_DEPTH`. `encode`, `write` and `read` stay.
- `lib.rs`: the five modules and the new exports.
- `liminal-net/Cargo.toml`: `float_roundtrip` on serde_json; `libc` for Unix targets (it is already in Cargo.lock, at 0.2.191); the feature `rule-checks`. The workspace's Cargo.toml and pyproject.toml: 0.3.0 at S5.
- `liminal-net-py/src/lib.rs`: the `Net<T>` wrapper and `into_net`; the `NodeConfig` literal (485-492) gains `process`; `Process`; `PyHillock` keeps the node's `autonomy` and gains `lost`, `own_thread`, `pulse`; every method through the wrapper (9.2's list); `timing_from` (430-450) takes the rule of 3.1; `Remote.request` (662-673) on the blocking form; `take_thread`, `report`, `state_line`, `Remote.delivery`; the registry and `_end_all`; with `rule-checks`: `_rule_checks`, `_on_async_thread`, `_probe_on_async`, `Node._enter_held`. `liminal-net-py/Cargo.toml`: `float_roundtrip`, and the feature passed on.
- `.github/workflows/ci.yml`: the bridge job's install line (54) builds with `rule-checks`.
- Tests: tests/common/mod.rs (`TempDir` and `config()`); new tests/queue.rs, frame.rs, process.rs, axon.rs, ending.rs, autonomy.rs, delivery.rs; additions to tests/functions.rs; new conftest.py, test_delivery.py, test_autonomy.py, test_rules.py.
- Documents: NETWORK.md: §3.4's module table; §4.3 (the end); §4.5; §4.11's pointer to this document; new rows for what stays uncapped, and for Q1 to Q7 as they are answered. The points of 10.2 that carry a question (rows 49, 91 and 95; NETWORK.md:547 and 621; the nesting limit in §5.2) are written only after the user's yes to each (Q8 to Q13). CHANGELOG.md, with G8's new hang and section 10.2 named as changes. README.md, with how the bridge is built for its tests.

**Unchanged**: `message.rs`, `handshake.rs` (its `proof` and `verify` are used as they are), `address.rs`, `transport.rs`, `handles.rs`, `errors.rs`, `idl.rs`; `key.rs` as far as this document goes (section 1); `PROTO`; tests/vectors.rs; every existing test function.

## 14. What the user must still decide

Q1 to Q7 stand from the earlier drafts. Where a decided row speaks, the first behaviour is the row's words; an answer changes one function and its tests only.
- **Q1** (decision 110): whether what `gate` and `in_order` hold gets a cap. `functions::hold_cap()`. The brief of this revision keeps the row open, so no cap is built first; the log on disk records "Capped, oldest dropped" (commit e4193c5). If that row is the user's word, `hold_cap` returns the line's caps from S0 on, the holds leave G5's list, what a function releases at once is then at most a line (so `released`, `carry` and `passed` are bounded with it), and F3 gains the cap's cases.
- **Q2**: whether a thread taken by the node's own call counts towards the 16 of decision 99. `counts_as_measured(why)`: first behaviour, it is given like a declared one. No decided row speaks of it.
- **Q3**: decision 104 says one ceiling "over all its lines". `ceiling()`: first behaviour, one total over arriving and outgoing lines together. The question is whether each side should have its own, so that a slow target can never cost a node its input.
- **Q4**: decision 97 says "the oldest waiting stream item". `Line::victim()`: first behaviour, the line's oldest stream item whatever its signal. The question is whether the signal that holds most of the line should give up its own oldest.
- **Q5**: for the pulse of step S6: what follows from a pulse that blocks on a shared server, and its shortest interval. Section 7.9 leaves it to the stuck-server rule and a floor of 10 ms.
- **Q6**: decision 102 and NETWORK.md:492 say an autonomous node's own thread sends what it fires, a firing thread helping at half. `others_send(now)`: first behaviour, exactly that, with one bound this revision adds: once the node's thread has been inside one call for longer than `wait` (0.25 s), the shared servers send what waits and apply the link ops, so a fired signal, and the answer to a `link`, `unlink` or `tune`, wait for that call for at most `wait`. The question is whether they should wait at all: with the yes, `others_send` holds for an autonomous node whenever `in_call` is set, and A9 takes its second form.
- **Q7**: where the Unix key file's change lands (NETWORK.md §4.11's last paragraph; key.rs:83-84, 139-143): this document assumes a commit of its own beside S0.

Q8 to Q13 are new in this revision. Each touches a sentence of NETWORK.md; each is one sentence and one yes.
- **Q8** (row 91, "counted per link"): may a link made again by the node that holds it go on from its number? `Axon::numbers_from()`: until the yes it starts again at 1, as today (node.rs:394-401), and the receiving end reads the lower number as a new start (7.6), so fault 15 is mended either way. With the yes, a loss across the re-link is told exactly, and the number goes on only when the op comes from the origin that last held the link.
- **Q9** (row 49, "every stimulus"): may it read "every stimulus that is taken"? The lines of decisions 97, 101 and 104 stand before the receiver's functions, so a state value merged in the line and a stream item dropped from a full line never reach them. No function is behind it: the other answer needs the functions to run at arrival, which is another design.
- **Q10** (row 95 and §4.11): may they say the new way node code can hang (G8's second case)? It follows from "one at a time".
- **Q11** (NETWORK.md:547): may the sentence about a slow receiver function be reworded to what 0.3 really mends (fault 2's row)?
- **Q12** (NETWORK.md:621, "a synapse closing fails every request in flight"): for the caller's own detach, is today's behaviour the rule (the request in flight is still answered)? `answers_after_detach()`: until the answer, today's behaviour, and X16 tests it. When the reached node begins to end, 0.3 follows the sentence.
- **Q13** (§5.2): the nesting limit is a new sentence of the protocol. `frame::MAX_DEPTH`: 120 levels of the message as framed, as specified. The other number is 127, exactly what today's reader reads back, so that nothing that crosses today is refused. Tests are written against the constant.
