//! liminal-net's Python bridge (decision 62): a Python object becomes a node on Liminal's network, so the Python not
//! yet replaced keeps working beside the Rust.
//!
//! There is one implementation of the network, liminal-net's. A Python node's own code runs on liminal-net's worker
//! threads, holding the interpreter's lock only for each call (decision 61). Every call from Python into the network
//! lets go of the lock while it waits.
//!
//! ```python
//! import liminal_net
//!
//! class Greeter:
//!     def greet(self, name):
//!         return f"hello {name}"
//!
//! interface = {"trait": "example.Greeter", "methods": {"greet": {"args": [{"name": "name", "type": "text"}],
//!                                                               "returns": "text"}}}
//! node = liminal_net.Node("greeter", "example", r"C:\data\liminal.db", Greeter(), interface)
//! print(node.serve())
//! ```

// As in liminal-net: a refusal crosses with its words, accept and denied, so the error type is large.
#![allow(clippy::result_large_err)]

use net::functions::Stimulus;
use net::handles::HandleRef;
use net::lifecycle::Stage;
use net::{
    Call, Chain, Hillock, Interface, Launcher, Map, NetError, Node, NodeConfig, NodeError, Peer, Remote, Target,
};
use pyo3::create_exception;
use pyo3::exceptions::{PyConnectionError, PyException, PyOverflowError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PySet, PyString, PyTuple};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

create_exception!(
    liminal_net,
    RemoteError,
    PyException,
    "A node's refusal or failure, as it crossed: its kind (today's class name), its words, accept, denied and status."
);
create_exception!(
    liminal_net,
    NetworkError,
    PyConnectionError,
    "The network itself failed: nobody answered, a wrong key, a closed synapse."
);

/// The bridge's own runtime: liminal-net's tasks run on it.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("liminal-net")
            .enable_all()
            .build()
            .expect("liminal-net's runtime starts")
    })
}

/// Waits for a future, on this thread, without the interpreter's lock.
fn wait<T: Send>(py: Python<'_>, future: impl Future<Output = T> + Send) -> T {
    py.detach(|| runtime().handle().block_on(future))
}

// Values: Python's objects and the wire's types (NETWORK.md §5.5).

/// A Python object as it crosses: `None`, `bool`, `int` (64 bits), `float` (NaN as null), `str`, `list` and `tuple`,
/// `dict` with text keys (its order kept), `set` as a list, `bytes` as base64 text, and anything with `isoformat()`
/// (a `datetime`) as ISO-8601 text.
fn to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if obj.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.is_instance_of::<PyInt>() {
        if let Ok(i) = obj.extract::<i64>() {
            return Ok(Value::from(i));
        }
        if let Ok(u) = obj.extract::<u64>() {
            return Ok(Value::from(u));
        }
        return Err(PyOverflowError::new_err(
            "an integer beyond 64 bits does not cross the wire",
        ));
    }
    if let Ok(f) = obj.cast::<PyFloat>() {
        return Ok(Value::from(f.value()));
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(Value::String(s.to_str()?.to_owned()));
    }
    if let Ok(d) = obj.cast::<PyDict>() {
        let mut m = Map::new();
        for (k, v) in d.iter() {
            let key = k
                .cast::<PyString>()
                .map_err(|_| PyTypeError::new_err("a map's keys are text"))?;
            m.insert(key.to_str()?.to_owned(), to_value(&v)?);
        }
        return Ok(Value::Object(m));
    }
    if obj.is_instance_of::<PyList>()
        || obj.is_instance_of::<PyTuple>()
        || obj.is_instance_of::<PySet>()
        || obj.is_instance_of::<PyFrozenSet>()
    {
        let mut items = Vec::new();
        for item in obj.try_iter()? {
            items.push(to_value(&item?)?);
        }
        return Ok(Value::Array(items));
    }
    if let Ok(b) = obj.cast::<PyBytes>() {
        return Ok(Value::String(base64(b.as_bytes())));
    }
    if obj.hasattr("isoformat")? {
        return Ok(Value::String(obj.call_method0("isoformat")?.extract()?));
    }
    Err(PyTypeError::new_err(format!(
        "a {} does not cross the wire",
        obj.get_type().name()?
    )))
}

fn to_py<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::Null => py.None().into_bound(py),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => i.into_pyobject(py)?.into_any(),
            (None, Some(u)) => u.into_pyobject(py)?.into_any(),
            _ => n.as_f64().unwrap_or(f64::NAN).into_pyobject(py)?.into_any(),
        },
        Value::String(s) => PyString::new(py, s).into_any(),
        Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(to_py(py, item)?)?;
            }
            list.into_any()
        }
        Value::Object(m) => {
            let dict = PyDict::new(py);
            for (k, v) in m {
                dict.set_item(k, to_py(py, v)?)?;
            }
            dict.into_any()
        }
    })
}

/// Standard base64, with padding: how bytes cross (decision 73).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn args_of(args: &Bound<'_, PyTuple>) -> PyResult<Vec<Value>> {
    args.iter().map(|a| to_value(&a)).collect()
}

fn kwargs_of(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Map> {
    match kwargs {
        None => Ok(Map::new()),
        Some(d) => match to_value(d.as_any())? {
            Value::Object(m) => Ok(m),
            _ => unreachable!("a dict is a map"),
        },
    }
}

fn peer_dict<'py>(py: Python<'py>, peer: &Peer) -> PyResult<Bound<'py, PyAny>> {
    let d = PyDict::new(py);
    d.set_item("name", &peer.name)?;
    d.set_item("kind", &peer.kind)?;
    d.set_item("scope", &peer.scope)?;
    d.set_item("pid", peer.pid)?;
    Ok(d.into_any())
}

// Errors: a node's refusals cross as records (NETWORK.md §4.1).

/// A Python exception as a record: its class's name is the kind, `str()` the words, and `accept`, `denied` and
/// `status` are taken from it when it has them. A `RemoteError` passed on keeps the kind it came with.
fn node_error(py: Python<'_>, e: PyErr) -> NodeError {
    let value = e.value(py);
    let attr = |name: &str| value.getattr(name).ok().filter(|v| !v.is_none());
    let kind = if value.is_instance_of::<RemoteError>() {
        attr("kind").and_then(|k| k.extract::<String>().ok())
    } else {
        None
    };
    let kind = kind.unwrap_or_else(|| {
        value
            .get_type()
            .name()
            .map(|n| n.to_string())
            .unwrap_or_else(|_| "Exception".into())
    });
    let message = match attr("message").filter(|_| value.is_instance_of::<RemoteError>()) {
        Some(m) => m.extract::<String>().unwrap_or_default(),
        None => value.str().map(|s| s.to_string()).unwrap_or_default(),
    };
    let mut error = NodeError::new(kind, message);
    error.accept = attr("accept").and_then(|v| to_value(&v).ok());
    error.denied = attr("denied").and_then(|v| to_value(&v).ok());
    error.status = attr("status").and_then(|v| v.extract::<u16>().ok());
    error
}

fn remote_error(py: Python<'_>, e: &NodeError) -> PyErr {
    let err = RemoteError::new_err(format!("{}: {}", e.kind, e.message));
    let value = err.value(py);
    let none = || py.None().into_bound(py);
    let set = |name: &str, v: Option<&Value>| {
        let v = v.map(|v| to_py(py, v)).transpose().ok().flatten().unwrap_or_else(none);
        let _ = value.setattr(name, v);
    };
    let _ = value.setattr("kind", &e.kind);
    let _ = value.setattr("message", &e.message);
    set("accept", e.accept.as_ref());
    set("denied", e.denied.as_ref());
    let _ = value.setattr("status", e.status);
    err
}

fn py_err(py: Python<'_>, e: NetError) -> PyErr {
    match e {
        NetError::Remote(e) => remote_error(py, &e),
        NetError::Io(e) => e.into(),
        other => NetworkError::new_err(other.to_string()),
    }
}

// A Python object as a node's own code.

struct PyHillock {
    obj: Py<PyAny>,
    interface: Interface,
}

impl PyHillock {
    /// Calls one of the object's methods, if it has it.
    fn call_if<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        args: impl pyo3::call::PyCallArgs<'py>,
    ) -> Option<PyResult<Bound<'py, PyAny>>> {
        let obj = self.obj.bind(py);
        match obj.hasattr(name) {
            Ok(true) => Some(obj.call_method1(name, args)),
            _ => None,
        }
    }
}

impl Hillock for PyHillock {
    fn interface(&self) -> Interface {
        self.interface.clone()
    }

    /// `request(peer, method, args, kwargs)` if the object has it; else the method itself, as `method(*args, **kwargs)`.
    /// Either way only the interface's methods reach it.
    fn request(&self, call: &Call<'_>, method: &str, args: Vec<Value>, kwargs: Map) -> Result<Value, NodeError> {
        Python::attach(|py| {
            let answer = (|| {
                let obj = self.obj.bind(py);
                let args_py = to_py(py, &Value::Array(args))?;
                let kwargs_py = to_py(py, &Value::Object(kwargs))?;
                let answer = if obj.hasattr("request")? {
                    obj.call_method1("request", (peer_dict(py, call.peer)?, method, args_py, kwargs_py))?
                } else {
                    let args: Bound<'_, PyTuple> = args_py.cast::<PyList>()?.to_tuple();
                    obj.call_method(method, args, Some(kwargs_py.cast::<PyDict>()?))?
                };
                to_value(&answer)
            })();
            answer.map_err(|e| node_error(py, e))
        })
    }

    fn integrate(&self, call: &Call<'_>, signal: &str, data: &Value) {
        Python::attach(|py| {
            let (Ok(peer), Ok(data)) = (peer_dict(py, call.peer), to_py(py, data)) else {
                return;
            };
            if let Some(Err(e)) = self.call_if(py, "integrate", (peer, signal, data)) {
                e.write_unraisable(py, Some(self.obj.bind(py)));
            }
        })
    }

    /// `admits(peer)`: `None` or `True` admits; text refuses with those words; anything else refuses.
    fn admits(&self, peer: &Peer) -> Result<(), String> {
        Python::attach(|py| {
            let Ok(peer) = peer_dict(py, peer) else {
                return Err("the peer could not be described".into());
            };
            match self.call_if(py, "admits", (peer,)) {
                None => Ok(()),
                Some(Ok(answer)) if answer.is_none() => Ok(()),
                Some(Ok(answer)) => match (answer.cast::<PyBool>(), answer.extract::<String>()) {
                    (Ok(b), _) if b.is_true() => Ok(()),
                    (_, Ok(why)) => Err(why),
                    _ => Err("refused".into()),
                },
                Some(Err(e)) => Err(e
                    .value(py)
                    .str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|_| "refused".into())),
            }
        })
    }

    fn attach(&self, peer: &Peer) {
        self.notify("attach", peer);
    }

    fn detach(&self, peer: &Peer) {
        self.notify("detach", peer);
    }

    fn shutdown(&self) {
        Python::attach(|py| {
            if let Some(Err(e)) = self.call_if(py, "shutdown", ()) {
                e.write_unraisable(py, Some(self.obj.bind(py)));
            }
        })
    }
}

impl PyHillock {
    fn notify(&self, name: &str, peer: &Peer) {
        Python::attach(|py| {
            let Ok(peer) = peer_dict(py, peer) else { return };
            if let Some(Err(e)) = self.call_if(py, name, (peer,)) {
                e.write_unraisable(py, Some(self.obj.bind(py)));
            }
        })
    }
}

/// A Python callable as a launcher: it starts the node when nobody answers.
struct PyLauncher(Py<PyAny>);

impl Launcher for PyLauncher {
    fn launch(&self) -> std::io::Result<()> {
        Python::attach(|py| {
            self.0
                .call0(py)
                .map(drop)
                .map_err(|e| std::io::Error::other(e.to_string()))
        })
    }
}

/// A Python callable as a synaptic function at the receiver's end. It is given the stimulus as a dict (`signal`,
/// `from`, `seq`, `data`) and gives back `None` (dropped), a dict (passed, perhaps reshaped) or a list of them.
/// A function that fails drops the stimulus.
fn py_function(f: Py<PyAny>) -> impl FnMut(Stimulus) -> Vec<Stimulus> + Send {
    move |s: Stimulus| {
        Python::attach(|py| {
            let run = || -> PyResult<Vec<Stimulus>> {
                let d = PyDict::new(py);
                d.set_item("signal", &s.signal)?;
                d.set_item("from", &s.from)?;
                d.set_item("seq", s.seq)?;
                d.set_item("data", to_py(py, &s.data)?)?;
                let out = f.bind(py).call1((d,))?;
                let one = |item: &Bound<'_, PyAny>| -> PyResult<Stimulus> {
                    let v = to_value(item)?;
                    Ok(Stimulus {
                        signal: v.get("signal").and_then(Value::as_str).unwrap_or(&s.signal).to_string(),
                        from: v.get("from").and_then(Value::as_str).unwrap_or(&s.from).to_string(),
                        seq: v.get("seq").and_then(Value::as_u64).unwrap_or(s.seq),
                        data: v.get("data").cloned().unwrap_or(Value::Null),
                    })
                };
                if out.is_none() {
                    Ok(Vec::new())
                } else if let Ok(list) = out.cast::<PyList>() {
                    list.iter().map(|item| one(&item)).collect()
                } else {
                    Ok(vec![one(&out)?])
                }
            };
            run().unwrap_or_else(|e| {
                e.write_unraisable(py, Some(f.bind(py)));
                Vec::new()
            })
        })
    }
}

/// A Python callable lent to other nodes.
fn lent(f: Py<PyAny>) -> impl Fn(Vec<Value>, Map) -> Result<Value, NodeError> + Send + Sync + 'static {
    move |args, kwargs| {
        Python::attach(|py| {
            let call = || -> PyResult<Value> {
                let args = PyTuple::new(py, args.iter().map(|a| to_py(py, a)).collect::<PyResult<Vec<_>>>()?)?;
                let kwargs = to_py(py, &Value::Object(kwargs))?;
                to_value(&f.bind(py).call(args, Some(kwargs.cast::<PyDict>()?))?)
            };
            call().map_err(|e| node_error(py, e))
        })
    }
}

fn timing_from(timing: Option<&Bound<'_, PyDict>>) -> PyResult<net::Timing> {
    let mut t = net::Timing::from_env();
    let Some(d) = timing else { return Ok(t) };
    for (k, v) in d.iter() {
        let seconds: f64 = v.extract()?;
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(PyValueError::new_err("times are seconds from 0"));
        }
        let d = Duration::from_secs_f64(seconds);
        match k.extract::<String>()?.as_str() {
            "start_wait" => t.start_wait = d,
            "grace" => t.grace = d,
            "hello_wait" => t.hello_wait = d,
            "quiet" => t.quiet = d,
            "reach_wait" => t.reach_wait = d,
            "coalesce" => t.coalesce = d,
            other => return Err(PyValueError::new_err(format!("there is no time {other:?}"))),
        }
    }
    Ok(t)
}

/// A node on Liminal's network, its code a Python object.
#[pyclass(module = "liminal_net", name = "Node", frozen)]
struct PyNode {
    node: Node,
    served: Mutex<Option<tokio::task::JoinHandle<String>>>,
}

#[pymethods]
impl PyNode {
    /// `Node(name, kind, scope, hillock, interface, ipc_dir=None, timing=None)`.
    /// - `hillock` is the node's own code: its interface's methods, and optionally `request`, `integrate`,
    ///   `admits`, `attach`, `detach` and `shutdown`.
    /// - `interface` is its description, as NETWORK.md §6 writes it.
    /// - `ipc_dir` is the user's folder (`LIMINAL_IPC_DIR`, else `~/.liminal/ipc`), and `timing` maps times to
    ///   seconds (`start_wait`, `grace`, `hello_wait`, `quiet`, `reach_wait`, `coalesce`).
    #[new]
    #[pyo3(signature = (name, kind, scope, hillock, interface, ipc_dir=None, timing=None))]
    #[allow(clippy::too_many_arguments)] // Python's constructor: its arguments are keywords there
    fn new(
        py: Python<'_>,
        name: &str,
        kind: &str,
        scope: PathBuf,
        hillock: Py<PyAny>,
        interface: &Bound<'_, PyAny>,
        ipc_dir: Option<PathBuf>,
        timing: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyNode> {
        let interface: Interface = serde_json::from_value(to_value(interface)?)
            .map_err(|e| PyValueError::new_err(format!("not an interface description: {e}")))?;
        let ipc_dir = ipc_dir.unwrap_or_else(net::key::ipc_dir);
        let timing = timing_from(timing)?;
        let key = py.detach(|| net::key::load_or_create(&ipc_dir))?;
        let config = NodeConfig {
            name: name.into(),
            kind: kind.into(),
            scope,
            ipc_dir,
            key,
            timing,
        };
        let hillock = Arc::new(PyHillock {
            obj: hillock,
            interface,
        });
        Ok(PyNode {
            node: Node::new(config, hillock),
            served: Mutex::new(None),
        })
    }

    /// Serves the node until it ends, and says why it ended.
    fn serve(&self, py: Python<'_>) -> String {
        let node = self.node.clone();
        wait(py, async move { node.serve().await })
    }

    /// Serves the node in the background. `join` waits for its end.
    fn start(&self) {
        let node = self.node.clone();
        let handle = runtime().spawn(async move { node.serve().await });
        *self.served.lock().unwrap() = Some(handle);
    }

    /// Waits for a node `start`ed to end, and says why; `None` if `timeout` seconds passed first.
    #[pyo3(signature = (timeout=None))]
    fn join(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Option<String>> {
        let Some(mut handle) = self.served.lock().unwrap().take() else {
            return Err(PyValueError::new_err("the node was not started"));
        };
        let (ended, handle) = wait(py, async move {
            let ended = match timeout {
                Some(t) => tokio::time::timeout(Duration::from_secs_f64(t.max(0.0)), &mut handle)
                    .await
                    .ok(),
                None => Some((&mut handle).await),
            };
            (ended, handle)
        });
        match ended {
            Some(reason) => Ok(Some(reason.map_err(|e| PyValueError::new_err(e.to_string()))?)),
            None => {
                *self.served.lock().unwrap() = Some(handle);
                Ok(None)
            }
        }
    }

    /// Ends the node now, for this reason.
    fn stop(&self, reason: &str) {
        self.node.stop(reason);
    }

    /// Fires a signal along the axon, to every node linked for it.
    fn fire(&self, signal: &str, data: &Bound<'_, PyAny>) -> PyResult<()> {
        self.node.fire(signal, to_value(data)?);
        Ok(())
    }

    /// Reaches another node's dendrites: by name, in this node's scope unless another is given. `launcher`, a
    /// callable, starts it when nobody answers.
    #[pyo3(signature = (node, scope=None, launcher=None))]
    fn reach(
        &self,
        py: Python<'_>,
        node: &str,
        scope: Option<String>,
        launcher: Option<Py<PyAny>>,
    ) -> PyResult<PyRemote> {
        let target = Target::new(node, scope.unwrap_or_else(|| self.node.target().scope));
        let launcher = launcher.map(PyLauncher);
        let me = self.node.clone();
        let reached = wait(py, async move {
            me.reach(&target, launcher.as_ref().map(|l| l as &dyn Launcher)).await
        });
        reached.map(|remote| PyRemote { remote }).map_err(|e| py_err(py, e))
    }

    /// Lends a callable to other nodes: its handle, to pass in a message.
    fn lend<'py>(&self, py: Python<'py>, function: Py<PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.node.lend(lent(function));
        to_py(py, &handle.to_value())
    }

    /// Calls a handle another node lent, at its owner's dendrites.
    #[pyo3(signature = (handle, *args, **kwargs))]
    fn call<'py>(
        &self,
        py: Python<'py>,
        handle: &Bound<'py, PyAny>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let handle = HandleRef::from_value(&to_value(handle)?).ok_or_else(|| PyValueError::new_err("not a handle"))?;
        let (args, kwargs) = (args_of(args)?, kwargs_of(kwargs)?);
        let me = self.node.clone();
        let answer = wait(py, async move { me.call(&handle, args, kwargs).await }).map_err(|e| py_err(py, e))?;
        to_py(py, &answer)
    }

    /// Tells a handle's owner it is no longer held.
    fn release(&self, py: Python<'_>, handle: &Bound<'_, PyAny>) -> PyResult<()> {
        let handle = HandleRef::from_value(&to_value(handle)?).ok_or_else(|| PyValueError::new_err("not a handle"))?;
        let me = self.node.clone();
        wait(py, async move { me.release(&handle).await }).map_err(|e| py_err(py, e))
    }

    /// The latest of a signal this node integrated, from that source.
    fn latest<'py>(&self, py: Python<'py>, source: &str, signal: &str) -> PyResult<Bound<'py, PyAny>> {
        let latest = py.detach(|| self.node.latest(source, signal));
        to_py(py, &latest.unwrap_or(Value::Null))
    }

    /// This node's own function at its end of the links it makes for `signal` from `source`.
    fn receive(&self, py: Python<'_>, source: &str, signal: &str, function: Py<PyAny>) {
        let chain = Chain::new().then(py_function(function));
        py.detach(|| self.node.receive(source, signal, chain));
    }

    /// How many handles this node holds lent.
    #[getter]
    fn lent(&self) -> usize {
        self.node.lent()
    }

    #[getter]
    fn name(&self) -> String {
        self.node.name().to_string()
    }

    /// Where its dendrites listen.
    #[getter]
    fn address(&self) -> String {
        self.node.config().address()
    }

    /// Its name and scope, as other nodes reach it.
    #[getter]
    fn target(&self) -> (String, String) {
        let t = self.node.target();
        (t.node, t.scope)
    }

    /// How many synapses its dendrites hold now.
    #[getter]
    fn synapses(&self) -> usize {
        self.node.synapses()
    }

    /// `running`, `securing` or `ended`.
    #[getter]
    fn stage(&self) -> &'static str {
        match self.node.stage() {
            Stage::Running => "running",
            Stage::Securing => "securing",
            Stage::Ended => "ended",
        }
    }
}

/// Another node, held through a synapse from this node's axon.
#[pyclass(module = "liminal_net", name = "Remote", frozen)]
struct PyRemote {
    remote: Remote,
}

#[pymethods]
impl PyRemote {
    /// Requests one of the node's interface methods: `remote.request("status")`, `remote.request("setup", log, steps=None)`.
    #[pyo3(signature = (method, *args, **kwargs))]
    fn request<'py>(
        &self,
        py: Python<'py>,
        method: &str,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (args, kwargs) = (args_of(args)?, kwargs_of(kwargs)?);
        let (remote, method) = (self.remote.clone(), method.to_string());
        let answer = wait(py, async move { remote.request(&method, args, kwargs).await }).map_err(|e| py_err(py, e))?;
        to_py(py, &answer)
    }

    /// Asks the node to extend its axon to this node's dendrites for `signal`.
    /// - `functions`: the built-ins it runs at its end, as data (`[{"fn": "changed", "fields": ["state"]}]`).
    /// - `receiver`: this node's own function at its end (see `Node.receive`).
    #[pyo3(signature = (signal, functions=None, receiver=None))]
    fn link(
        &self,
        py: Python<'_>,
        signal: &str,
        functions: Option<&Bound<'_, PyAny>>,
        receiver: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let functions = declared(functions)?;
        let (remote, signal) = (self.remote.clone(), signal.to_string());
        let chain = receiver.map(|f| Chain::new().then(py_function(f)));
        wait(py, async move {
            match chain {
                Some(chain) => remote.link_with(&signal, functions, chain).await,
                None => remote.link(&signal, functions).await,
            }
        })
        .map_err(|e| py_err(py, e))
    }

    /// The reverse of `link`.
    fn unlink(&self, py: Python<'_>, signal: &str) -> PyResult<()> {
        let (remote, signal) = (self.remote.clone(), signal.to_string());
        wait(py, async move { remote.unlink(&signal).await }).map_err(|e| py_err(py, e))
    }

    /// Replaces a live link's functions at the node's end.
    #[pyo3(signature = (signal, functions=None))]
    fn tune(&self, py: Python<'_>, signal: &str, functions: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let functions = declared(functions)?;
        let (remote, signal) = (self.remote.clone(), signal.to_string());
        wait(py, async move { remote.tune(&signal, functions).await }).map_err(|e| py_err(py, e))
    }

    /// Lets go of the synapse. Detaching twice does nothing more.
    fn detach(&self) {
        self.remote.detach();
    }

    /// Waits until the synapse has closed.
    fn closed(&self, py: Python<'_>) {
        let remote = self.remote.clone();
        wait(py, async move { remote.closed().await });
    }

    #[getter]
    fn pid(&self) -> u32 {
        self.remote.pid()
    }

    #[getter]
    fn interface<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, self.remote.interface())
    }

    #[getter]
    fn target(&self) -> (String, String) {
        let t = self.remote.target();
        (t.node.clone(), t.scope.clone())
    }

    #[getter]
    fn is_open(&self) -> bool {
        self.remote.is_open()
    }
}

fn declared(functions: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<Value>> {
    match functions.map(to_value).transpose()? {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(PyTypeError::new_err("functions are a list of declarations")),
    }
}

/// host.py's digest of a scope's path.
#[pyfunction]
fn digest(scope: PathBuf) -> String {
    net::address::digest(&scope)
}

/// Where the node `name` of a scope listens.
#[pyfunction]
#[pyo3(signature = (name, scope, ipc_dir=None))]
fn address(name: &str, scope: PathBuf, ipc_dir: Option<PathBuf>) -> String {
    net::address::address(name, &scope, &ipc_dir.unwrap_or_else(net::key::ipc_dir))
}

#[pymodule]
#[pyo3(name = "liminal_net")]
fn liminal_net_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyNode>()?;
    m.add_class::<PyRemote>()?;
    m.add("RemoteError", m.py().get_type::<RemoteError>())?;
    m.add("NetworkError", m.py().get_type::<NetworkError>())?;
    m.add_function(wrap_pyfunction!(digest, m)?)?;
    m.add_function(wrap_pyfunction!(address, m)?)?;
    m.add("PROTO", net::PROTO)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
