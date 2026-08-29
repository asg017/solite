# adapted from https://github.com/jupyter/jupyter_kernel_test/blob/main/jupyter_kernel_test/__init__.py


import json
import pytest
from jupyter_client.blocking.client import BlockingKernelClient
from jupyter_client.manager import KernelManager, start_new_kernel
from jupyter_client.utils import run_sync  # type:ignore[attr-defined]
from typing import Any, List
import inspect
from subprocess import Popen, PIPE
from tempfile import TemporaryFile
from pathlib import Path
import re
import shutil
import socket
import subprocess
import time
from dataclasses import dataclass
from types import SimpleNamespace
from fastmcp import Client
from fastmcp.client.transports import StdioTransport
import pytest_asyncio


def ensure_sync(func: Any) -> Any:
    if inspect.iscoroutinefunction(func):
        return run_sync(func)
    return func


class Kernel:
    def __init__(self, manager: KernelManager, client: BlockingKernelClient):
        self.manager = manager
        self.client = client
        self.timeout = 1

    def get_non_kernel_info_reply(self) -> dict[str, Any] | None:
        while True:
            reply = self.client.get_shell_msg(timeout=self.timeout)
            if reply["header"]["msg_type"] != "kernel_info_reply":
                return reply

    def execute(self, code: str):
        self.client.execute(
            code=code, silent=False, store_history=False, stop_on_error=False
        )
        reply = self.get_non_kernel_info_reply()
        assert reply is not None

        busy_msg = ensure_sync(self.client.iopub_channel.get_msg)(timeout=1)
        assert busy_msg["content"]["execution_state"] == "busy"

        output_msgs = []
        while True:
            msg = ensure_sync(self.client.iopub_channel.get_msg)(timeout=0.1)
            if msg["msg_type"] == "status":
                assert msg["content"]["execution_state"] == "idle"
                break
            elif msg["msg_type"] == "execute_input":
                assert msg["content"]["code"] == code
                continue
            output_msgs.append(msg)

        return reply, output_msgs


@pytest.fixture
def solite_kernel(tmp_path, monkeypatch):
    # Register a kernelspec pointing at *this* checkout's freshly built
    # binary. A globally installed `solite` kernelspec may point at another
    # checkout's binary, which would silently test the wrong build.
    kernel_dir = tmp_path / "jupyter" / "kernels" / "solite"
    kernel_dir.mkdir(parents=True)
    (kernel_dir / "kernel.json").write_text(
        json.dumps(
            {
                "argv": [
                    str(CLI_PATH),
                    "jupyter",
                    "up",
                    "--connection",
                    "{connection_file}",
                ],
                "display_name": "Solite",
                "language": "sql",
                "interrupt_mode": "signal",
            }
        )
    )
    # JUPYTER_PATH entries take precedence over the user-level kernelspecs
    monkeypatch.setenv("JUPYTER_PATH", str(tmp_path / "jupyter"))
    km, kc = start_new_kernel(kernel_name="solite")
    yield Kernel(km, kc)
    kc.stop_channels()
    km.shutdown_kernel()


@pytest_asyncio.fixture
async def mcp_client():
    client = Client(
        transport=StdioTransport(command=str(CLI_PATH), args=["mcp", "up"]),
    )
    async with client:
        await client.ping()
        yield client


CLI_PATH = Path(__file__).parent.parent / "target" / "debug" / "solite"


# https://stackoverflow.com/questions/14693701/how-can-i-remove-the-ansi-escape-sequences-from-a-string-in-python
def escape_ansi_codes(src):
    ansi_escape = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")

    return ansi_escape.sub("", src)


@dataclass
class CliResult:
    stdout: str
    stderr: str
    success: bool


@pytest.fixture
def solite_cli():
    def solite_cli(
        args: List[str],
        communicate=None,
        kill=False,
        escape_ansi=True,
        cwd=None,
        env=None,
    ):
        # `env` is merged over the inherited environment
        if env is not None:
            import os

            env = {**os.environ, **env}
        with TemporaryFile() as stdout:
            with TemporaryFile() as stderr:
                p = Popen(
                    [str(CLI_PATH), *args],
                    stdin=PIPE,
                    stdout=stdout,
                    stderr=stderr,
                    cwd=cwd,
                    env=env,
                )
                if communicate is not None:
                    for line in communicate:
                        p.communicate(line)

                if kill:
                    p.kill()
                else:
                    p.wait()

                stdout.seek(0)
                out = stdout.read()

                stderr.seek(0)
                err = stderr.read()

                if escape_ansi:
                    stdout = escape_ansi_codes(out.decode("utf8"))
                    stderr = escape_ansi_codes(err.decode("utf8"))
                else:
                    stdout = out.decode("utf8")
                    stderr = err.decode("utf8")

        return CliResult(stdout, stderr, success=p.returncode == 0)

    yield solite_cli


@pytest.fixture
def s3_gateway(tmp_path):
    """Start a local S3-compatible gateway (versitygw) backed by a temp dir.

    Skips the test cleanly when `versitygw` is not on PATH. Yields a
    namespace with `root` (the gateway's posix root dir; `root / "bucket"`
    is the bucket named "bucket") and `env` (AWS_* vars to merge into
    `solite_cli(..., env=...)`).
    """
    exe = shutil.which("versitygw")
    if exe is None:
        pytest.skip("versitygw not installed")

    root = tmp_path / "gw"
    (root / "bucket").mkdir(parents=True)

    # Grab a free port by binding to port 0 and reading it back.
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]

    # Global flags (-a/-s/-p) must precede the `posix` subcommand: versitygw
    # silently ignores -p and binds :7070 if it comes after `posix DIR`.
    proc = subprocess.Popen(
        [
            exe,
            "-a",
            "testkey",
            "-s",
            "testsecret",
            "-p",
            f"127.0.0.1:{port}",
            "posix",
            str(root),
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    try:
        deadline = time.monotonic() + 5
        connected = False
        while time.monotonic() < deadline:
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                    connected = True
                    break
            except OSError:
                time.sleep(0.1)
        if not connected:
            proc.terminate()
            proc.wait(timeout=5)
            pytest.fail(f"versitygw did not start listening on port {port} in time")

        yield SimpleNamespace(
            root=root,
            env={
                "AWS_ENDPOINT_URL_S3": f"http://127.0.0.1:{port}",
                "AWS_ACCESS_KEY_ID": "testkey",
                "AWS_SECRET_ACCESS_KEY": "testsecret",
                "AWS_REGION": "us-east-1",
            },
        )
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)
