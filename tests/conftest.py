# adapted from https://github.com/jupyter/jupyter_kernel_test/blob/main/jupyter_kernel_test/__init__.py


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
from dataclasses import dataclass


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
        msg_id = self.client.execute(
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
def solite_kernel():
    km, kc = start_new_kernel(kernel_name="solite")
    yield Kernel(km, kc)
    kc.stop_channels()
    km.shutdown_kernel()


CLI_PATH = Path(__file__).parent.parent / "target" / "debug" / "solite-cli"


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
        args: List[str], communicate=None, kill=False, escape_ansi=True, cwd=None
    ):
        with TemporaryFile() as stdout:
            with TemporaryFile() as stderr:
                p = Popen(
                    [str(CLI_PATH), *args],
                    stdin=PIPE,
                    stdout=stdout,
                    stderr=stderr,
                    cwd=cwd,
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
