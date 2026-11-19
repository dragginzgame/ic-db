"""Check schema diagnostics with the caller's rust-analyzer LSP executable."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
DB = HERE.parents[2] / "crates/ic-db"


def send(server, **message):
    data = json.dumps({"jsonrpc": "2.0", **message}).encode()
    server.stdin.write(f"Content-Length: {len(data)}\r\n\r\n".encode() + data)
    server.stdin.flush()


def receive(server):
    length = None
    while True:
        line = server.stdout.readline()
        if not line:
            raise EOFError("rust-analyzer stopped before reporting diagnostics")
        if line == b"\r\n":
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1])
    if length is None:
        raise ValueError("missing LSP Content-Length")
    return json.loads(server.stdout.read(length))


def check(server, root, file):
    uri = file.as_uri()
    document = {"uri": uri}
    source = file.read_text()
    control = source[:source.index("#[cfg(any())]")].count("\n")
    revision = source[:source.index("pub struct Revision")].count("\n")
    send(server, id=1, method="initialize", params={
        "rootUri": root.as_uri(),
        "capabilities": {"experimental": {"serverStatusNotification": True},
                         "textDocument": {"diagnostic": {}}},
        "workspaceFolders": [{"uri": root.as_uri(), "name": root.name}],
    })
    requested = False
    pending = {2, 3}
    while pending:
        msg = receive(server)
        if "method" in msg and "id" in msg:
            send(server, id=msg["id"], result=None)
        elif msg.get("id") == 1:
            send(server, method="initialized", params={})
            send(server, method="textDocument/didOpen", params={"textDocument": {
                **document, "languageId": "rust", "version": 1, "text": source}})
        elif (msg.get("method") == "experimental/serverStatus"
              and msg["params"]["quiescent"] and not requested):
            requested = True
            send(server, id=2, method="textDocument/diagnostic", params={"textDocument": document})
            send(server, id=3, method="textDocument/hover", params={
                "textDocument": document, "position": {"line": revision, "character": 12}})
        elif msg.get("id") in pending:
            pending.remove(msg["id"])
            if msg["id"] == 2:
                items = msg["result"]["items"]
                inactive = [d for d in items if d.get("code") == "inactive_code"]
                assert inactive and all(d["range"]["start"]["line"] >= control for d in inactive), items
                assert not any(d.get("severity") == 1 for d in items), items
            else:
                assert "pub struct Revision(pub u64)" in msg["result"]["contents"]["value"], msg


def run(binary):
    target = HERE.parents[1] / "target"
    target.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=target, prefix="schema-editor-") as directory:
        root = Path(directory)
        shutil.copytree(HERE / "producer/src", root / "src")
        shutil.copy(HERE / "producer/build.rs", root)
        (root / "Cargo.toml").write_text(f'''[package]
name = "schema-editor-check"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
ic-db = {{ path = {json.dumps(str(DB))} }}
[build-dependencies]
ic-db = {{ path = {json.dumps(str(DB))}, features = ["build"] }}
''')
        file = root / "src/model/mod.rs"
        with file.open("a") as output:
            output.write("\n#[cfg(any())]\npub struct InactiveControl;\n")
        with subprocess.Popen([binary], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              env={**os.environ, "CARGO_NET_OFFLINE": "true"}) as server:
            try:
                signal.alarm(120)
                check(server, root, file)
            finally:
                signal.alarm(0)
                server.kill()
        print("Active schema declarations, generated newtype hover and inactive control verified.")


if __name__ == "__main__":
    def timeout(*_):
        raise TimeoutError("rust-analyzer did not finish within 120 seconds")

    signal.signal(signal.SIGALRM, timeout)
    run(sys.argv[1])
