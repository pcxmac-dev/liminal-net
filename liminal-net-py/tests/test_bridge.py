"""liminal-net's Python bridge: Python objects as nodes on the network.

Nodes are served on the bridge's own runtime in this process, over a scratch key folder, under names of their own.
No process is started, and nothing leaves this machine.
"""
from __future__ import annotations

import datetime
import os
import sys
import threading
import time
from pathlib import Path

import pytest

import liminal_net

LIBS = Path(__file__).resolve().parents[3] / "common" / "src"
QUICK = {"grace": 0.2, "start_wait": 5, "hello_wait": 1, "reach_wait": 5, "coalesce": 0.1}

ECHO = {
    "trait": "test.Echo",
    "methods": {
        "echo": {"args": [{"name": "value", "type": "any"}], "returns": "any"},
        "add": {"args": [{"name": "a", "type": "int"}, {"name": "b", "type": "int"}], "returns": "int"},
        "fail": {"args": [], "returns": "null"},
        "refuse": {"args": [], "returns": "null"},
        "callback": {"args": [{"name": "log", "type": "node:callable"}], "returns": "any"},
    },
    "signals": {"status": {"kind": "state", "data": "map"}, "lines": {"kind": "stream", "data": "any"}},
}


class Refused(Exception):
    """A refusal as Liminal's plugins raise them: words, what to accept, a status."""

    def __init__(self, words, accept=None, status=400):
        super().__init__(words)
        self.accept = accept
        self.status = status


class Echo:
    def __init__(self):
        self.node = None
        self.secured = 0
        self.attached = []

    def echo(self, value=None):
        return value

    def add(self, a, b=0):
        return a + b

    def fail(self):
        raise ValueError("bad value")

    def refuse(self):
        raise Refused("needs a second confirmation", accept="--force")

    def callback(self, log):
        return self.node.call(log, "from echo", level="info")

    def attach(self, peer):
        self.attached.append(peer["name"])

    def shutdown(self):
        self.secured += 1


class Recorder:
    def __init__(self):
        self.got = []

    def integrate(self, peer, signal, data):
        self.got.append((signal, data))

    def wait(self, n, within=3.0):
        deadline = time.monotonic() + within
        while len(self.got) < n and time.monotonic() < deadline:
            time.sleep(0.01)
        return list(self.got)


@pytest.fixture
def place(tmp_path):
    return {"scope": tmp_path / "scope.db", "ipc_dir": tmp_path / "ipc", "timing": QUICK}


def echo_node(place, **kw):
    hillock = Echo()
    node = liminal_net.Node("echo", "plugin", place["scope"], hillock, ECHO, ipc_dir=place["ipc_dir"],
                            timing=place["timing"], **kw)
    hillock.node = node
    node.start()
    return node, hillock


def app_node(place, recorder=None):
    recorder = recorder or Recorder()
    node = liminal_net.Node("app", "app", place["scope"], recorder, {"trait": "test.App"}, ipc_dir=place["ipc_dir"],
                            timing=place["timing"])
    return node, recorder


def waiting():
    """A launcher that starts nothing: reach then waits for a node still binding."""


def test_digests_and_addresses_match_host_py(tmp_path):
    if not (LIBS / "liminal_libs" / "host.py").is_file():
        pytest.skip("liminal-libs is not checked out beside this repository")
    sys.path.insert(0, str(LIBS))
    try:
        from liminal_libs import host
    finally:
        sys.path.remove(str(LIBS))
    existing = tmp_path / "Real Folder" / "liminal.db"
    existing.parent.mkdir()
    existing.write_text("")
    for path in (existing, tmp_path / "missing" / ".." / "Other.DB", r"C:\no-such-liminal-dir\Scope\liminal.db"):
        assert liminal_net.digest(path) == host._digest(path), path
        if sys.platform == "win32":
            assert liminal_net.address("fetcher", path) == host.address("fetcher", path).replace(
                r"\liminal-fetcher-", r"\liminal-net-fetcher-")


def test_a_python_node_answers_requests(place):
    node, _ = echo_node(place)
    caller, _ = app_node(place)
    remote = caller.reach("echo", launcher=waiting)
    value = {"z": 1, "a": [1, 2.5, "three", None, True], "m": {"y": {"deep": []}}}
    answer = remote.request("echo", value)
    assert answer == value and list(answer) == ["z", "a", "m"], "the keys keep their order"
    assert remote.request("add", 2, b=40) == 42, "positional and keyword arguments both cross"
    assert remote.pid == os.getpid(), "the node runs in this process"
    assert remote.interface["trait"] == "test.Echo"
    assert node.synapses == 1


def test_a_python_exception_crosses_as_a_remote_error(place):
    echo_node(place)
    caller, _ = app_node(place)
    remote = caller.reach("echo", launcher=waiting)
    with pytest.raises(liminal_net.RemoteError) as raised:
        remote.request("fail")
    assert (raised.value.kind, raised.value.message) == ("ValueError", "bad value")
    with pytest.raises(liminal_net.RemoteError) as raised:
        remote.request("refuse")
    error = raised.value
    assert (error.kind, error.message, error.accept, error.status) == (
        "Refused", "needs a second confirmation", "--force", 400)


def test_only_the_interface_is_served(place):
    _, hillock = echo_node(place)
    caller, _ = app_node(place)
    remote = caller.reach("echo", launcher=waiting)
    with pytest.raises(liminal_net.RemoteError) as raised:
        remote.request("shutdown")
    assert raised.value.kind == "NotInInterface"
    assert hillock.secured == 0, "the node's own code never saw it"


def test_admits_refuses_with_its_words(place):
    class Picky(Echo):
        def admits(self, peer):
            return None if peer["kind"] == "app" else f"only apps, not a {peer['kind']}"

    node = liminal_net.Node("echo", "plugin", place["scope"], Picky(), ECHO, ipc_dir=place["ipc_dir"],
                            timing=place["timing"])
    node.start()
    stranger = liminal_net.Node("stranger", "plugin", place["scope"], Recorder(), {"trait": "t"},
                                ipc_dir=place["ipc_dir"], timing=place["timing"])
    with pytest.raises(liminal_net.NetworkError, match="only apps, not a plugin"):
        stranger.reach("echo", launcher=waiting)
    caller, _ = app_node(place)
    assert caller.reach("echo").is_open


def test_the_node_secures_its_data_and_ends_after_its_last_synapse(place):
    node, hillock = echo_node(place)
    caller, _ = app_node(place)
    remote = caller.reach("echo", launcher=waiting)
    assert remote.request("echo", 1) == 1
    assert hillock.attached == ["app"]
    remote.detach()
    remote.detach()
    assert node.join(timeout=5) == "no synapse for 0.2 s"
    assert hillock.secured == 1
    assert node.stage == "ended"


def test_signals_go_where_they_are_linked(place):
    node, _ = echo_node(place)
    receiver, recorder = app_node(place)
    receiver.start()
    remote = receiver.reach("echo", launcher=waiting)
    remote.link("lines")
    for i in range(10):
        node.fire("lines", {"n": i})
    got = recorder.wait(10)
    assert [d["n"] for _, d in got] == list(range(10)), "every one, in order"
    assert receiver.latest("echo", "lines") == {"n": 9}


def test_state_signals_are_coalesced_and_sender_functions_filter(place):
    node, _ = echo_node(place)
    receiver, recorder = app_node(place)
    receiver.start()
    remote = receiver.reach("echo", launcher=waiting)
    remote.link("status", [{"fn": "changed", "fields": ["state"]}])
    for state in ("a", "a", "b", "b", "c"):
        node.fire("status", {"state": state})
    time.sleep(0.4)
    assert [d["state"] for _, d in recorder.got] == ["a", "c"], "the first at once, then the latest that changed"


def test_a_python_function_runs_at_the_receivers_end(place):
    node, _ = echo_node(place)
    receiver, recorder = app_node(place)
    receiver.start()
    remote = receiver.reach("echo", launcher=waiting)

    def evens_doubled(stimulus):
        if stimulus["data"] % 2:
            return None
        return {"data": stimulus["data"] * 2}

    remote.link("lines", receiver=evens_doubled)
    for i in range(1, 7):
        node.fire("lines", i)
    recorder.wait(3)
    time.sleep(0.1)
    assert [d for _, d in recorder.got] == [4, 8, 12]


def test_a_lent_callable_is_answered_at_its_owner(place):
    echo_node(place)
    owner, _ = app_node(place)
    owner.start()
    calls = []

    def log(line, level=None):
        calls.append((line, level, threading.current_thread().name))
        return f"logged {line}"

    handle = owner.lend(log)
    assert handle["$node"] >= 1 and handle["owner"]["node"] == "app"
    remote = owner.reach("echo", launcher=waiting)
    assert remote.request("callback", handle) == "logged from echo"
    assert [(line, level) for line, level, _ in calls] == [("from echo", "info")]
    assert owner.lent == 1


def test_values_cross_as_the_wire_types_say(place):
    echo_node(place)
    caller, _ = app_node(place)
    remote = caller.reach("echo", launcher=waiting)
    assert remote.request("echo", (1, 2)) == [1, 2], "a tuple crosses as a list"
    assert sorted(remote.request("echo", {3, 1, 2})) == [1, 2, 3], "a set crosses as a list"
    assert remote.request("echo", b"\x00\xffhi") == "AP9oaQ==", "bytes cross as base64 text"
    when = datetime.datetime(2026, 11, 9, 10, 30, tzinfo=datetime.timezone.utc)
    assert remote.request("echo", when) == "2026-11-09T10:30:00+00:00", "a time crosses as ISO-8601 text"
    assert remote.request("echo", float("nan")) is None, "NaN crosses as null"
    assert remote.request("echo", 1_794_000_000_123_456_789) == 1_794_000_000_123_456_789
    assert remote.request("echo", 1.0) == 1.0 and isinstance(remote.request("echo", 1.0), float)
    with pytest.raises(OverflowError):
        remote.request("echo", 2**70)
    with pytest.raises(TypeError):
        remote.request("echo", object())


def test_nobody_answering_without_a_launcher_is_a_network_error(place):
    caller, _ = app_node(place)
    with pytest.raises(liminal_net.NetworkError, match="no node nowhere answers"):
        caller.reach("nowhere")
