"""只运行一次每种真实场景；保存完整脱敏合同与实际 manifest hash。"""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
EVIDENCE = ROOT / "evidence/canonical-repair"
EXE = ROOT / "harness/target/debug/cb5-004-contract-probe.exe"
NAMES = {"before": "cancel-before-result.json", "after": "cancel-after-result.json", "permission": "permission-deny-result.json"}


def save(path, value):
    """JSON 只写当前 task；不落任意 CLI stderr。"""
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def hash_manifests(value):
    """所有 hash 来自 Rust 捕获的实际文件字节，不根据期望值填充。"""
    if isinstance(value, dict):
        if value.get("kind") == "file" and "bytes" in value:
            content = bytes(value.pop("bytes"))
            value.update(size=len(content), sha256=hashlib.sha256(content).hexdigest())
        for child in value.values():
            hash_manifests(child)
    elif isinstance(value, list):
        for child in value:
            hash_manifests(child)


def clean(report):
    """没有可靠清理事实则禁止开始下一个场景。"""
    return report.get("harnessExitCode") == 0 and report["cleanup"]["succeeded"] and report["workspaceDeleted"]


def summarize():
    """整理已有 evidence；从不重新发起未知副作用请求。"""
    reports = [json.loads((EVIDENCE / name).read_text(encoding="utf-8")) for name in NAMES.values() if (EVIDENCE / name).exists()]
    attempts = reports.copy()
    for filename, modes in [("cancellation.jsonl", ["before", "after"]), ("permission.jsonl", ["permission"])]:
        rows = [{"scenario": r["scenario"], "pid": r["pid"], **row} for r in attempts if r["scenario"] in modes for row in r["wire"]]
        (EVIDENCE / filename).write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows), encoding="utf-8")
    save(EVIDENCE / "permission-options.json", [{"pid": r["pid"], "sequence": row["sequence"], **row["message"]["params"]} for r in attempts if r["scenario"] == "permission" for row in r["wire"] if row["message"].get("method") == "session/request_permission"])
    diagnostic = EVIDENCE / "session-new-repair-result.json"
    if diagnostic.exists():
        attempts.append(json.loads(diagnostic.read_text(encoding="utf-8")))
    save(EVIDENCE / "process-evidence.json", [{k: r.get(k) for k in ["scenario", "argv", "cwd", "pid", "elapsedMs", "cleanup", "workspaceDeleted", "workspaceDeleteError", "harnessExitCode"]} for r in attempts])
    statuses = {mode: "NOT_RUN: C_GATE_FAILED" if not repair_passed() else "NOT_RUN" for mode in NAMES}
    for r in reports:
        convergence = r["providerTerminalReceived"] or (r["protocol"]["stage"] == "no_terminal_timeout" and r["runtimeTerminationEvidence"])
        if r["scenario"] == "before":
            proved = r["cancel"] is not None and r["delta"] == [] and r["protocol"].get("trigger", {}).get("kind") == "correlated_activity_and_zero_delta"
        elif r["scenario"] == "after":
            proved = r["cancel"] is not None and r["markerRetained"] and r["protocol"].get("trigger", {}).get("kind") == "actual_marker_exact_bytes"
        else:
            proved = bool(r["protocol"]["permissionDecisions"]) and r["delta"] == []
        statuses[r["scenario"]] = "PASS" if proved and convergence and clean(r) else "PARTIAL"
    result = {"scenarios": statuses, "status": "PASS" if len(statuses) == 3 and all(s == "PASS" for s in statuses.values()) else "PARTIAL", "automaticRetries": 0, "sessionNewRepair": repair_passed(), "windowsJobAtCreationProven": False}
    save(EVIDENCE / "runner-result.json", result)
    print(json.dumps(result), flush=True)
    return 0 if result["status"] == "PASS" else 1


def repair_passed():
    """C Gate 必须有 exact RPC response、非空 sessionId 和默认无 prompt 成功事实。"""
    path = EVIDENCE / "session-new-repair-result.json"
    if not path.exists():
        return False
    r = json.loads(path.read_text(encoding="utf-8"))
    new = r.get("sessionNew", {})
    request = new.get("request") or {}
    response = new.get("response") or {}
    return (clean(r) and r["protocol"]["stage"] == "session_ready_no_prompt"
            and bool(r.get("sessionId")) and r["prompt"]["request"] is None
            and request.get("message", {}).get("id") == response.get("message", {}).get("id")
            and response.get("message", {}).get("result", {}).get("sessionId") == r["sessionId"]
            and request.get("message", {}).get("params") == {"cwd": r["cwd"], "mcpServers": []})


def main():
    """用户授权一次repair，然后每场景最多一次；任何旧输出/identity均拒绝重放。"""
    if "--summarize" in sys.argv:
        return summarize()
    mode = sys.argv[1]
    if mode not in ["diagnostic", "before", "after", "permission"]:
        raise RuntimeError("Unknown fixed probe")
    if mode != "diagnostic" and not repair_passed():
        raise RuntimeError("C Gate not passed; no real scenario may launch")
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    output = EVIDENCE / ("session-new-repair-result.json" if mode == "diagnostic" else NAMES[mode])
    if output.exists() or output.with_suffix(".identity.json").exists():
        raise RuntimeError("Attempt already exists or unknown outcome; refusing replay")
    process = subprocess.Popen([str(EXE), mode, str(output)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        code = process.wait(timeout=175)
    except subprocess.TimeoutExpired:
        errors = []
        try:
            killed = subprocess.run([str(Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"), "/PID", str(process.pid), "/T", "/F"], capture_output=True, timeout=10)
            if killed.returncode:
                errors.append("taskkill_exit=" + str(killed.returncode))
        except (OSError, subprocess.TimeoutExpired) as error:
            errors.append(type(error).__name__)
        try:
            process.kill()
            process.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired) as error:
            errors.append(type(error).__name__)
        save(EVIDENCE / (mode + "-watchdog.json"), {"status": "TIMEOUT", "errors": errors, "pid": process.pid})
        return 2
    if not output.exists():
        save(EVIDENCE / (mode + "-failure.json"), {"harnessExitCode": code, "resultMissing": True})
        return 2
    report = json.loads(output.read_text(encoding="utf-8"))
    report["harnessExitCode"] = code
    hash_manifests(report)
    save(output, report)
    print(json.dumps({"scenario": mode, "terminal": report["providerTerminalReceived"], "stage": report["protocol"]["stage"], "delta": report["delta"], "error": report["error"]}), flush=True)
    if mode == "diagnostic":
        (EVIDENCE / "session-new-repair.jsonl").write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in report["wire"]), encoding="utf-8")
        return 0 if repair_passed() else 1
    summarize()
    return 0 if clean(report) else 2


if __name__ == "__main__":
    sys.exit(main())
