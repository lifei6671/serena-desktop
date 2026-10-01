"""只运行一次每种真实场景；保存完整脱敏合同与实际 manifest hash。"""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
EVIDENCE = ROOT / "evidence"
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
    initial = [json.loads(p.read_text(encoding="utf-8")) for p in sorted((EVIDENCE / "initial-scenarios").glob("*-result.json"))]
    attempts = initial + reports
    for filename, modes in [("cancellation.jsonl", ["before", "after"]), ("permission.jsonl", ["permission"])]:
        rows = [{"scenario": r["scenario"], "pid": r["pid"], **row} for r in attempts if r["scenario"] in modes for row in r["wire"]]
        (EVIDENCE / filename).write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows), encoding="utf-8")
    save(EVIDENCE / "permission-options.json", [{"pid": r["pid"], "sequence": row["sequence"], **row["message"]["params"]} for r in attempts if r["scenario"] == "permission" for row in r["wire"] if row["message"].get("method") == "session/request_permission"])
    diagnostic = EVIDENCE / "session-new-diagnostic.json"
    if diagnostic.exists():
        attempts.append(json.loads(diagnostic.read_text(encoding="utf-8")))
    save(EVIDENCE / "process-evidence.json", [{k: r.get(k) for k in ["scenario", "argv", "cwd", "pid", "elapsedMs", "cleanup", "workspaceDeleted", "workspaceDeleteError", "harnessExitCode"]} for r in attempts])
    statuses = {}
    for r in reports:
        convergence = r["providerTerminalReceived"] or (r["protocol"]["stage"] == "no_terminal_timeout" and r["runtimeTerminationEvidence"])
        if r["scenario"] == "before":
            proved = r["cancel"] is not None and r["delta"] == [] and r["protocol"].get("trigger", {}).get("kind") == "correlated_activity_and_zero_delta"
        elif r["scenario"] == "after":
            proved = r["cancel"] is not None and r["markerRetained"] and r["protocol"].get("trigger", {}).get("kind") == "actual_marker_exact_bytes"
        else:
            proved = bool(r["protocol"]["permissionDecisions"]) and r["delta"] == []
        statuses[r["scenario"]] = "PASS" if proved and convergence and clean(r) else "PARTIAL"
    result = {"scenarios": statuses, "status": "PASS" if len(statuses) == 3 and all(s == "PASS" for s in statuses.values()) else "PARTIAL", "automaticRetries": 0, "explicitFreshFollowupAfterSessionNewOnly": bool(initial), "windowsJobAtCreationProven": False}
    save(EVIDENCE / "runner-result.json", result)
    print(json.dumps(result), flush=True)
    return 0 if result["status"] == "PASS" else 1


def main():
    """外层 watchdog 只处理本次 harness 子树；失败保留 evidence，不重放。"""
    if "--summarize" in sys.argv:
        return summarize()
    for mode, name in NAMES.items():
        output = EVIDENCE / name
        attempt = 1
        if output.exists():
            previous = json.loads(output.read_text(encoding="utf-8"))
            archived = EVIDENCE / "initial-scenarios" / name
            # 显式一次追加仅允许完全未发送 prompt 的已清理 session/new 失败。
            if "--fresh-session-new-followup" not in sys.argv or archived.exists() or previous["prompt"]["request"] is not None or previous["protocol"]["stage"] != "session/new" or not clean(previous):
                raise RuntimeError("Existing real evidence; refusing replay: " + mode)
            archived.parent.mkdir(exist_ok=True)
            archived.write_bytes(output.read_bytes())
            attempt = 2
        current_output = EVIDENCE / f"{mode}-attempt-{attempt}.json"
        if current_output.exists():
            raise RuntimeError("Attempt artifact already exists; refusing replay")
        process = subprocess.Popen([str(EXE), mode, str(current_output)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
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
        if not current_output.exists():
            save(EVIDENCE / (mode + "-failure.json"), {"harnessExitCode": code, "resultMissing": True})
            return 2
        report = json.loads(current_output.read_text(encoding="utf-8"))
        report["harnessExitCode"] = code
        hash_manifests(report)
        save(current_output, report)
        save(output, report)
        print(json.dumps({"scenario": mode, "terminal": report["providerTerminalReceived"], "stage": report["protocol"]["stage"], "delta": report["delta"], "error": report["error"]}), flush=True)
        if not clean(report):
            summarize()
            return 2
    return summarize()


if __name__ == "__main__":
    sys.exit(main())
