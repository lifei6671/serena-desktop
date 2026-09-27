"""运行 task-local Rust harness，并从保存的实际字节生成 SHA256 与合同证据。"""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
EVIDENCE = ROOT / "evidence"
EXE = ROOT / "harness/target/debug/cb5-003-session-probe.exe"


def save(name, value):
    """只在当前 task evidence 下写入 UTF-8 JSON。"""
    (EVIDENCE / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main():
    """每个场景独立运行；Rust 拥有 CodeBuddy 子进程，外层只提供总时限。"""
    reports = []
    for mode, name in [("read", "read-only-result.json"), ("write", "isolated-write-result.json")]:
        output = EVIDENCE / name
        command = [str(EXE), mode, str(output)]
        # --summarize 只整理已保存证据，不重新启动真实 CLI。
        completed = None
        if "--summarize" not in sys.argv:
            process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                process.communicate(timeout=205)
                completed = process.returncode
            except subprocess.TimeoutExpired:
                # 外层 watchdog 仅处理本次拥有的进程树，所有等待均有上限。
                errors = []
                try:
                    killed = subprocess.run([str(Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"), "/PID", str(process.pid), "/T", "/F"], timeout=10, capture_output=True)
                    if killed.returncode:
                        errors.append(f"taskkill_exit={killed.returncode}")
                except (OSError, subprocess.TimeoutExpired) as error:
                    errors.append(type(error).__name__)
                try:
                    process.kill()
                except OSError as error:
                    errors.append(f"kill={type(error).__name__}")
                try:
                    process.wait(timeout=5)
                except (OSError, subprocess.TimeoutExpired) as error:
                    errors.append(f"wait={type(error).__name__}")
                save(f"{mode}-watchdog.json", {"status": "TIMEOUT", "errors": errors, "pid": process.pid})
                raise
            finally:
                process.stdout.close()
                process.stderr.close()
        if not output.exists():
            raise RuntimeError(f"{mode}: harness exit={completed}; no evidence")
        report = json.loads(output.read_text(encoding="utf-8"))
        if completed is not None:
            report["harnessExitCode"] = completed
        # Rust 已在 workspace 删除前复制实际字节；hash 从这些字节计算，不从期望值生成。
        for phase in ["before", "after"]:
            if report[phase] is None:
                continue
            for entry in report[phase].values():
                if entry["kind"] == "file" and "bytes" in entry:
                    content = bytes(entry.pop("bytes"))
                    entry.update(size=len(content), sha256=hashlib.sha256(content).hexdigest(), contentUtf8=content.decode("utf-8", errors="replace"))
        report["scenario"] = mode
        save(name, report)
        reports.append(report)
        print(json.dumps({"scenario": mode, "canExecuteEvidence": report["canExecuteEvidence"], "error": report["error"], "delta": report["delta"]}), flush=True)

    rows = []
    activity = []
    for report in reports:
        prompt = report["prompt"]
        request_sequence = (prompt["request"] or {}).get("sequence")
        terminal_sequence = (prompt["response"] or {}).get("sequence")
        summaries = []
        for row in report["wire"]:
            rows.append({"scenario": report["scenario"], **row, "globalSequence": len(rows) + 1})
            message = row.get("message", {})
            if message.get("method") == "session/update":
                params = message.get("params", {})
                update = params.get("update", {})
                summaries.append({"sequence": row["sequence"], "sessionId": params.get("sessionId"), "updateType": update.get("sessionUpdate"), "publicFields": list(update), "update": update, "beforePrompt": request_sequence is not None and row["sequence"] < request_sequence, "beforeTerminal": terminal_sequence is not None and row["sequence"] < terminal_sequence})
        activity.append({"scenario": report["scenario"], "promptRpcId": (prompt["request"] or {}).get("message", {}).get("id"), "promptSequence": request_sequence, "terminalSequence": terminal_sequence, "updateCount": len(summaries), "updates": summaries})
    (EVIDENCE / "fresh-session.jsonl").write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows), encoding="utf-8")
    save("activity-order.json", activity)
    a, b = reports
    identifier = b["conversationRequestId"]
    echoes = [row["sequence"] for row in b["wire"] if row["direction"] == "response" and identifier and identifier in json.dumps(row.get("message"))]
    b_sent = b["prompt"]["request"] is not None
    save("conversation-request-id.json", {"A": {"promptSent": a["prompt"]["request"] is not None, "metadataSent": False, "sessionId": a["sessionId"], "prompt": a["prompt"], "succeeded": a["protocolSucceeded"]}, "B": {"promptSent": b_sent, "metadataSent": b_sent, "generatedUuid": identifier, "sessionId": b["sessionId"], "prompt": b["prompt"], "succeeded": b["protocolSucceeded"], "echoSequences": echoes}, "required": False if a["protocolSucceeded"] else "NOT_PROVEN", "acceptedWhenPresent": b["protocolSucceeded"] if b_sent else "NOT_PROVEN", "echoed": bool(echoes) if b_sent else "NOT_PROVEN", "ignored": "NOT_PROVEN: absence of echo cannot establish server internal behavior", "correlationUseful": bool(echoes) if b_sent else "NOT_PROVEN", "recommendation": "Optional, with no adoption recommendation absent correlation benefit" if a["protocolSucceeded"] else "NOT_PROVEN: session/new blocked; no real prompt A/B was sent"})
    save("process-evidence.json", [{key: r[key] for key in ["scenario", "argv", "cwd", "pid", "environmentChanges", "cleanup", "workspaceDeleted", "workspaceDeleteError", "elapsedMs", "harnessExitCode"]} for r in reports])
    return 0 if all(r["canExecuteEvidence"] and r["cleanup"]["succeeded"] and r["workspaceDeleted"] for r in reports) else 1


if __name__ == "__main__":
    sys.exit(main())
