"""Gold Band 最小 JSONL 复现：一次 npx、initialize/new，绝不发送 prompt。"""
from collections import Counter, deque
from pathlib import Path
import hashlib
import json
import queue
import stat
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent
EVIDENCE = ROOT / "evidence/gold-band-wire-repro"
NPX = r"C:\Users\lifei\AppData\Roaming\npm\npx.cmd"
LAUNCHER = [NPX, "-y", "@tencent-ai/codebuddy-code@2.158.0", "--acp"]
EXECUTION = [r"C:\Windows\System32\cmd.exe", "/d", "/s", "/c", " ".join(LAUNCHER)]
INITIALIZE = {
    "protocolVersion": 1,
    "clientCapabilities": {
        "elicitation": {"form": {}},
        "_meta": {"subagent-transcript": True, "parameterizedModelPicker": True},
    },
    "clientInfo": {
        "name": "serena-desktop-gold-band-repro",
        "title": "SerenaDesktop Gold Band Repro",
        "version": "0.1",
    },
}
UPDATE_TYPES = {"agent_message_chunk", "agent_thought_chunk", "user_message_chunk", "tool_call", "tool_call_update", "plan", "available_commands_update", "current_mode_update", "config_option_update", "session_info_update", "usage_update"}


class ProbeError(Exception):
    """仅携带固定枚举；错误原文不能泄漏外部上下文。"""


def save(name, value):
    """持久化当前诊断专属目录，不覆盖旧轮次证据。"""
    (EVIDENCE / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def safe_error(error):
    """保留 JSON-RPC code 与静态分类，丢弃 message/data 的任意文本。"""
    text = json.dumps(error)
    category = "HTTP_500" if "status code 500" in text else "HTTP_AUTH_STATUS" if "status code 401" in text or "status code 403" in text else "UNCLASSIFIED"
    return {"code": error.get("code") if isinstance(error.get("code"), int) else None, "classification": category}


class JsonlClient:
    """顺序发请求但不假设帧顺序；route 建立前只缓存有界安全通知。"""

    def __init__(self, send, receive):
        """注入 bytes writer 和有界 reader，使 fake 与真实走同一解析路径。"""
        self.send = send
        self.receive = receive
        self.rows = []
        self.next_id = 1
        self.wrong_ids = 0
        self.notifications = Counter()
        self.preroute = deque(maxlen=64)
        self.preroute_total = 0
        self.route = None
        self.unsolicited_requests = 0

    def record(self, direction, message):
        """只记录调用方已白名单处理的内容。"""
        self.rows.append({"sequence": len(self.rows) + 1, "direction": direction, "message": message})

    def request(self, method, params, timeout):
        """只允许诊断的三种方法，按精确 JSON-RPC identity 等响应。"""
        if method not in {"initialize", "session/new", "session/close"}:
            raise ProbeError("METHOD_FORBIDDEN")
        rpc_id = self.next_id
        self.next_id += 1
        outgoing = {"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params}
        self.send((json.dumps(outgoing, separators=(",", ":")) + "\n").encode())
        self.record("request", outgoing)
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ProbeError("TIMEOUT")
            line = self.receive(remaining)
            if not line:
                raise ProbeError("EOF")
            try:
                message = json.loads(line)
            except (ValueError, UnicodeError):
                raise ProbeError("MALFORMED_JSON") from None
            if not isinstance(message, dict) or message.get("jsonrpc") != "2.0":
                raise ProbeError("MALFORMED_ENVELOPE")
            if "method" in message:
                if "id" in message:
                    self.unsolicited_requests += 1
                    raise ProbeError("UNEXPECTED_SERVER_REQUEST")
                notification = message["method"]
                notification = notification if notification == "session/update" else "OTHER_NOTIFICATION"
                params_in = message.get("params", {})
                if not isinstance(params_in, dict):
                    raise ProbeError("MALFORMED_NOTIFICATION")
                update = params_in.get("update", {})
                update_type = update.get("sessionUpdate") if isinstance(update, dict) else None
                update_type = update_type if update_type in UPDATE_TYPES else "OTHER_UPDATE"
                self.notifications[notification + ":" + update_type] += 1
                safe = {"method": notification, "updateType": update_type}
                # 所有notification共用64条wire上限，不能让OTHER绕过界限。
                if sum(self.notifications.values()) <= 64:
                    self.record("notification", safe)
                if self.route is None:
                    self.preroute_total += 1
                    self.preroute.append(safe)
                continue
            # bool/string ID 不得与数字 RPC id 相等后错误绑定。
            if type(message.get("id")) is not int or message["id"] != rpc_id:
                self.wrong_ids += 1
                # 错误identity仅保留固定数量样本，余下继续累计计数。
                if self.wrong_ids <= 16:
                    self.record("ignored_response", {"reason": "RPC_ID_MISMATCH"})
                continue
            if ("result" in message) == ("error" in message):
                raise ProbeError("MALFORMED_RESPONSE")
            if "error" in message:
                if not isinstance(message["error"], dict):
                    raise ProbeError("MALFORMED_ERROR")
                safe = safe_error(message["error"])
                self.record("response", {"jsonrpc": "2.0", "id": rpc_id, "error": safe})
                raise ProbeError(safe["classification"])
            result = message["result"]
            if not isinstance(result, dict):
                raise ProbeError("MALFORMED_RESULT")
            if method == "initialize":
                caps = result.get("agentCapabilities", {})
                if not isinstance(caps, dict):
                    raise ProbeError("MALFORMED_CAPABILITIES")
                session_caps = caps.get("sessionCapabilities", {})
                close_supported = isinstance(session_caps, dict) and isinstance(session_caps.get("close"), dict)
                safe = {"protocolVersion": result.get("protocolVersion"), "agentCapabilityKeys": sorted(caps), "closeSupported": close_supported}
            elif method == "session/new":
                session_id = result.get("sessionId")
                safe = {"sessionId": session_id if isinstance(session_id, str) else None}
            else:
                safe = {"acknowledged": True}
            self.record("response", {"jsonrpc": "2.0", "id": rpc_id, "result": safe})
            return safe


def handshake(client, cwd):
    """新会话失败/空身份立即停；仅显式广告 close capability 时尝试关闭。"""
    result = {"stage": "initialize", "status": "FAILED", "sessionId": None, "close": "NOT_RUN", "promptSent": False}
    try:
        initialized = client.request("initialize", INITIALIZE, 60)
        result["initialize"] = initialized
        if type(initialized["protocolVersion"]) is not int or initialized["protocolVersion"] != 1:
            raise ProbeError("PROTOCOL_MISMATCH")
        result["stage"] = "session/new"
        session = client.request("session/new", {"cwd": cwd, "mcpServers": []}, 60)
        if not isinstance(session["sessionId"], str) or not session["sessionId"].strip():
            raise ProbeError("EMPTY_OR_INVALID_SESSION_ID")
        client.route = session["sessionId"]
        result.update(stage="session_ready", status="PASS", sessionId=client.route)
        if initialized["closeSupported"]:
            result["close"] = "REQUESTED"
            try:
                client.request("session/close", {"sessionId": client.route}, 5)
                result["close"] = "ACKNOWLEDGED"
            except ProbeError as error:
                result["close"] = "FAILED: " + str(error)
        else:
            result["close"] = "NOT_ADVERTISED"
    except ProbeError as error:
        result["error"] = str(error)
    return result


def manifest(root):
    """全部隐藏文件/目录纳入，symlink/reparse fail closed；只存长度和实际hash。"""
    result = {}
    for path in root.rglob("*"):
        meta = path.lstat()
        if path.is_symlink() or getattr(meta, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
            raise ProbeError("WORKSPACE_LINK")
        key = path.relative_to(root).as_posix()
        if path.is_dir():
            result[key] = {"kind": "directory"}
        elif path.is_file():
            content = path.read_bytes()
            result[key] = {"kind": "file", "size": len(content), "sha256": hashlib.sha256(content).hexdigest()}
        else:
            raise ProbeError("WORKSPACE_SPECIAL_FILE")
    return result


def run(child_env=None, launcher_diagnostics=None):
    """唯一真实执行；durable sentinel 防止失败/unknown 后再次启动。"""
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    sentinel = EVIDENCE / "attempt-started.json"
    with sentinel.open("x", encoding="utf-8") as file:
        json.dump({"launcherArgv": LAUNCHER, "executionArgv": EXECUTION, "attempt": 1, "automaticRetry": False}, file)
        file.flush()
        import os
        os.fsync(file.fileno())
    workspace = tempfile.TemporaryDirectory(prefix="cb5-004-gold-band-")
    cwd = Path(workspace.name).resolve()
    before = manifest(cwd)
    process = None
    stop = threading.Event()
    incoming = queue.Queue(maxsize=64)
    threads = []
    stderr_count = [0]
    report = {"launcherArgv": LAUNCHER, "executionArgv": EXECUTION, "cwd": str(cwd), "before": before, "status": "LAUNCHER_UNAVAILABLE", "promptSent": False, "automaticRetries": 0}
    if launcher_diagnostics is not None:
        report["launcherResolution"] = launcher_diagnostics
    cleanup = {"directChildReaped": False, "windowsJobAtCreationProven": False, "treeContainmentProven": False, "errors": []}
    started = time.monotonic()

    def enqueue(value):
        """队列满时也能被清理中止，不无限阻塞 reader join。"""
        while not stop.is_set():
            try:
                incoming.put(value, timeout=0.1)
                return
            except queue.Full:
                continue

    def read_stdout():
        """一帧最多1MiB；非JSON交主循环固定分类，不落原文。"""
        try:
            while not stop.is_set():
                line = process.stdout.readline(1024 * 1024 + 1)
                if len(line) > 1024 * 1024:
                    enqueue(ProbeError("FRAME_TOO_LARGE"))
                    return
                enqueue(line)
                if not line:
                    return
        except (OSError, ValueError):
            enqueue(ProbeError("STDOUT_IO_ERROR"))

    def drain_stderr():
        """stderr只计字节，完全不保存内容。"""
        try:
            while True:
                chunk = process.stderr.read(4096)
                if not chunk:
                    break
                stderr_count[0] += len(chunk)
        except (OSError, ValueError):
            return

    def receive(timeout):
        """按当前RPC剩余期限等待下一帧。"""
        try:
            value = incoming.get(timeout=timeout)
        except queue.Empty:
            raise ProbeError("TIMEOUT") from None
        if isinstance(value, ProbeError):
            raise value
        return value

    def send(data):
        """flush成功才记wire；管道错误用固定枚举。"""
        try:
            process.stdin.write(data)
            process.stdin.flush()
        except (OSError, ValueError):
            raise ProbeError("STDIN_IO_ERROR") from None

    client = JsonlClient(send, receive)
    try:
        process = subprocess.Popen(EXECUTION, cwd=cwd, env=child_env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW)
        report["pid"] = process.pid
        threads = [threading.Thread(target=read_stdout, daemon=True), threading.Thread(target=drain_stderr, daemon=True)]
        for thread in threads:
            thread.start()
        report.update(handshake(client, str(cwd)))
        report["launcherAvailable"] = "initialize" in report
        if not report["launcherAvailable"]:
            report["status"] = "LAUNCHER_UNAVAILABLE_OR_NO_ACP_HANDSHAKE"
    except OSError:
        report["error"] = "LAUNCHER_SPAWN_FAILED"
    finally:
        # npx有中间子进程；使用本次owned PID进行有界树清理，仍不声称Job containment。
        if process is not None:
            try:
                killed = subprocess.run([r"C:\Windows\System32\taskkill.exe", "/PID", str(process.pid), "/T", "/F"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10, creationflags=subprocess.CREATE_NO_WINDOW)
                cleanup["treeKillExitCode"] = killed.returncode
            except (OSError, subprocess.TimeoutExpired):
                cleanup["errors"].append("TREE_KILL_FAILED")
            try:
                process.kill()
                process.wait(timeout=5)
                cleanup["directChildReaped"] = True
                cleanup["exitCode"] = process.returncode
            except (OSError, subprocess.TimeoutExpired):
                cleanup["errors"].append("DIRECT_CHILD_WAIT_FAILED")
            try:
                process.stdin.close()
            except OSError:
                cleanup["errors"].append("STDIN_CLOSE_FAILED")
            stop.set()
            for thread in threads:
                thread.join(timeout=2)
            cleanup["readersJoined"] = all(not thread.is_alive() for thread in threads)
            if cleanup["readersJoined"]:
                process.stdout.close()
                process.stderr.close()
            else:
                cleanup["errors"].append("READER_JOIN_TIMEOUT")
        report["cleanup"] = cleanup
        report["elapsedMs"] = int((time.monotonic() - started) * 1000)
        report["stderrBytesDrained"] = stderr_count[0]
        report["wire"] = client.rows
        report["notifications"] = dict(client.notifications)
        report["wrongRpcIdResponsesIgnored"] = client.wrong_ids
        report["prerouteNotifications"] = {"total": client.preroute_total, "buffered": len(client.preroute), "dropped": max(0, client.preroute_total - len(client.preroute)), "limit": 64}
        try:
            after = manifest(cwd)
            report["after"] = after
            report["delta"] = sorted(k for k in before.keys() | after.keys() if before.get(k) != after.get(k))
        except ProbeError as error:
            report["manifestError"] = str(error)
        try:
            workspace.cleanup()
            report["workspaceDeleted"] = True
        except OSError:
            report["workspaceDeleted"] = False
        save("wire-result.json", report)
        (EVIDENCE / "wire.jsonl").write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in client.rows), encoding="utf-8")
    print(json.dumps({k: report.get(k) for k in ["status", "stage", "error", "sessionId", "close", "delta", "workspaceDeleted"]}), flush=True)
    return 0 if report["status"] == "PASS" and report.get("delta") == [] and not cleanup["errors"] and cleanup["directChildReaped"] and report["workspaceDeleted"] else 1


if __name__ == "__main__":
    raise SystemExit(run())
