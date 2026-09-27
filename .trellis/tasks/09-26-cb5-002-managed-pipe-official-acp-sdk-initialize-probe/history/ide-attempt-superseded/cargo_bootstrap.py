"""仅为本任务绕开宿主 Schannel 凭据失败；上游 HTTPS 保持证书校验。"""
import http.server
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import urllib.request

ROOT = Path(__file__).resolve().parent


class Registry(http.server.BaseHTTPRequestHandler):
    """仅代理官方 sparse index 与 crate 下载，不接受任意上游。"""

    def do_GET(self):
        """Cargo 使用本机 HTTP，上游由 Python TLS 校验证书。"""
        try:
            if self.path == "/config.json":
                data = json.dumps({"dl": f"http://127.0.0.1:{self.server.server_port}/crates/{{crate}}/{{version}}/download"}).encode()
            else:
                path = self.path
                if path.startswith("/crates/"):
                    _, _, name, version, _ = path.split("/")
                    url = f"https://static.crates.io/crates/{name}/{name}-{version}.crate"
                else:
                    url = "https://index.crates.io" + path
                with urllib.request.urlopen(url, timeout=30) as response:
                    data = response.read()
            self.send_response(200)
            self.end_headers()
            self.wfile.write(data)
        except Exception as error:
            self.send_error(502, type(error).__name__)

    def log_message(self, *_):
        """避免把下载细节刷入验证日志。"""


def cleanup_cargo(process):
    """树清理失败不跳过 direct kill/wait；错误列表明确记录未证明的清理。"""
    errors = []
    try:
        result = subprocess.run([str(Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"), "/PID", str(process.pid), "/T", "/F"], timeout=10, check=False)
        if result.returncode:
            errors.append(f"taskkill_exit={result.returncode}")
    except (OSError, subprocess.TimeoutExpired) as error:
        errors.append(f"tree_cleanup={type(error).__name__}")
    finally:
        try:
            if process.poll() is None:
                process.kill()
        except OSError as error:
            errors.append(f"direct_kill={type(error).__name__}")
        finally:
            try:
                process.wait(timeout=5)
            except (OSError, subprocess.TimeoutExpired) as error:
                errors.append(f"direct_wait={type(error).__name__}")
    return errors


def main():
    """在 finally 中关闭代理线程；Cargo 限定到本任务缓存和 crate。"""
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 18742), Registry)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    env = os.environ.copy()
    env["CARGO_HOME"] = str(ROOT / ".cargo-cache")
    env["CARGO_HTTP_PROXY"] = ""
    command = ["cargo", "--config", 'source.crates-io.replace-with="task-official"', "--config", 'source.task-official.registry="sparse+http://127.0.0.1:18742/"', *sys.argv[1:]]
    process = None
    try:
        process = subprocess.Popen(command, cwd=ROOT / "harness", env=env)
        return process.wait(timeout=600)
    except (subprocess.TimeoutExpired, KeyboardInterrupt):
        # Rustup shim 可能有 Cargo 后代；仅终止本次拥有的进程树并回收句柄。
        if process is not None and process.poll() is None:
            errors = cleanup_cargo(process)
            if errors:
                print(json.dumps({"cleanupErrors": errors, "descendantsReaped": "unverified"}), file=sys.stderr)
        raise
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    sys.exit(main())
