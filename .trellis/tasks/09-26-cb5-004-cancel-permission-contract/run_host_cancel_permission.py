"""Host 专用单次 runner：不包含重试或 force 开关。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

TASK = Path(__file__).absolute().parent
PROOF = TASK / "evidence" / "host-cancel-permission-proof"
EXE = TASK / "harness" / "target" / "debug" / "host_cancel_permission.exe"


def reserve(root, scenario):
    """原子占用场景并同步磁盘；失败或未知结果仍保留占用。"""
    root.mkdir(parents=True, exist_ok=True)
    path = root / (scenario + ".attempt-started.json")
    with path.open("x", encoding="utf-8") as stream:
        json.dump({"scenario": scenario, "startedNs": time.time_ns(), "replayAllowed": False}, stream)
        stream.flush()
        os.fsync(stream.fileno())
    return path


def write_new(path, value):
    """证据只能新增，不覆盖既有结果。"""
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def acceptance(report, scenario):
    """只根据真实 wire、收敛和 manifest 判定，进程 exit0 不等于场景 PASS。"""
    converged = report.get("providerTerminalReceived") is True or (
        report.get("protocol", {}).get("stage") == "no_terminal_timeout"
        and report.get("cleanup", {}).get("directChildReaped") is True)
    common = converged and report.get("cleanup", {}).get("succeeded") is True and report.get("workspaceDeleted") is True and report.get("manifestError") is None
    if scenario == "before":
        proven = bool(report.get("cancel")) and report.get("delta") == []
    elif scenario == "after":
        proven = bool(report.get("cancel")) and report.get("markerRetained") is True
    else:
        decisions = report.get("protocol", {}).get("permissionDecisions", [])
        proven = bool(decisions) and report.get("delta") == []
    return "PASS" if common and proven else "PARTIAL"


def run(scenario):
    """只启动已有编译产物；真实 CodeBuddy 由该 harness 持有和回收。"""
    if not EXE.is_file():
        raise SystemExit("HARNESS_BINARY_MISSING")
    reserve(PROOF, scenario)
    filename = {"before": "cancel-before-result.json", "after": "cancel-after-result.json", "permission": "permission-deny-result.json"}[scenario]
    output = PROOF / filename
    if output.exists():
        raise SystemExit("EVIDENCE_ALREADY_EXISTS")
    process = subprocess.Popen([str(EXE), scenario, str(output)], cwd=str(TASK).removeprefix("\\\\?\\"), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    outer_timeout = False
    tree_cleanup = None
    try:
        exit_code = process.wait(timeout=180)
    except subprocess.TimeoutExpired:
        outer_timeout = True
        try:
            killed = subprocess.run([r"C:\Windows\System32\taskkill.exe", "/PID", str(process.pid), "/T", "/F"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
            tree_cleanup = killed.returncode
        except (OSError, subprocess.TimeoutExpired):
            tree_cleanup = "CLEANUP_FAILED_OR_TIMEOUT"
        try:
            exit_code = process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            exit_code = None
    report = json.loads(output.read_text(encoding="utf-8")) if output.is_file() else {}
    status = acceptance(report, scenario) if exit_code == 0 and not outer_timeout else "PARTIAL"
    write_new(PROOF / (scenario + "-process-evidence.json"), {"scenario": scenario, "harnessPid": process.pid, "exitCode": exit_code, "outerTimeout": outer_timeout, "outerTreeCleanupExit": tree_cleanup, "cleanup": report.get("cleanup"), "status": status, "binarySha256": hashlib.sha256(EXE.read_bytes()).hexdigest()})
    wire_path = PROOF / ("permission.jsonl" if scenario == "permission" else "cancellation-" + scenario + ".jsonl")
    with wire_path.open("x", encoding="utf-8") as stream:
        for row in report.get("wire", []):
            stream.write(json.dumps(row, ensure_ascii=False) + "\n")
    if scenario == "permission":
        write_new(PROOF / "permission-options.json", [row["message"].get("params", {}).get("options", []) for row in report.get("wire", []) if row.get("message", {}).get("method") == "session/request_permission"])
    print(json.dumps({"scenario": scenario, "status": status, "result": str(output), "exitCode": exit_code}))
    return 0 if status == "PASS" else 2


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scenario", choices=["before", "after", "permission"])
    args = parser.parse_args()
    raise SystemExit(run(args.scenario))
