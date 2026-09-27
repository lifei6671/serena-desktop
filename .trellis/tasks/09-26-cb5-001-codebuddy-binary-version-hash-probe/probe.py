"""CB5-001 本地只读探针；仅采集身份，不启动 ACP。"""

import argparse
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

TASK = Path(__file__).resolve().parent
VERSION = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"


def parse_version(raw):
    """接受单行版本或实测的版本/commit/arch 三行；其他输出关闭解析。"""
    if raw.get("exit_code") != 0 or raw.get("timed_out"):
        return {"status": "REJECTED", "reason": "PROCESS_FAILED", "value": None}
    value = raw["stdout"].strip()
    if not value:
        return {"status": "REJECTED", "reason": "EMPTY", "value": None}
    match = re.fullmatch(f"({VERSION})(?:\r?\n([0-9a-f]{{40}})\r?\n(x64|arm64|ia32))?", value)
    if not match:
        return {"status": "REJECTED", "reason": "MALFORMED", "value": None}
    return {"status": "PARSED", "reason": None, "value": match[1],
            "source": "cli_stdout", "commit": match[2], "architecture": match[3]}


def version_candidate(raw, pe):
    """候选采用产品所有的 PE 元数据，绝不把 CLI 或 base 版本静默提升为产品版本。"""
    parsed = parse_version(raw)
    value = pe.get("ProductVersion", "")
    candidate = None
    if pe.get("ProductName") == "CodeBuddy CN" and re.fullmatch(VERSION, value):
        candidate = {"value": value, "source": "pe_product_version", "supported": False}
    return {"parsed_cli_version": parsed, "product_version_candidate": candidate}


def resolve_entry(where):
    """只解析实测 shim 的固定语法，不执行批处理；缺失入口稳定返回 NOT_FOUND。"""
    if where["exit_code"] != 0:
        return {"status": "NOT_FOUND"}
    paths = [Path(line.strip()) for line in where["stdout"].splitlines() if line.strip()]
    shim = next((p for p in paths if p.name.lower() == "buddycn.cmd" and p.is_file()), None)
    if shim is None:
        return {"status": "NOT_FOUND"}
    content = shim.read_text(encoding="utf-8-sig")
    match = re.search(r'^"%~dp0([^"\r\n]+\.exe)" "%~dp0([^"\r\n]+\.js)" %\*$', content, re.M)
    if not match or not re.search(r"^set ELECTRON_RUN_AS_NODE=1$", content, re.M):
        return {"status": "MALFORMED_SHIM", "shim_path": str(shim)}
    exe, cli = [(shim.parent / part).resolve() for part in match.groups()]
    entry = {"shim_path": str(shim.resolve()), "shim_content": content,
             "canonical_exe": str(exe), "cli_js": str(cli),
             "environment": {"ELECTRON_RUN_AS_NODE": "1", "VSCODE_DEV": None}}
    entry["status"] = "FOUND" if exe.is_file() and cli.is_file() else "NOT_FOUND"
    return entry


def run(command, cwd, env_changes=None):
    """限时运行允许的只读命令；保留精确字节及显示文本，不导出完整环境。"""
    env = os.environ.copy()
    for key, value in (env_changes or {}).items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    result = {"argv": command, "command_line": subprocess.list2cmdline(command),
              "cwd": str(cwd), "environment_changes": env_changes or {}, "timeout_seconds": 15}
    try:
        completed = subprocess.run(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                   capture_output=True, timeout=15, check=False)
        stdout, stderr = completed.stdout, completed.stderr
        result.update(exit_code=completed.returncode, timed_out=False)
    except subprocess.TimeoutExpired as error:
        stdout, stderr = error.stdout or b"", error.stderr or b""
        result.update(exit_code=None, timed_out=True)
    # where.exe 使用 Windows OEM 编码；其他命令的 JSON / CLI 按 UTF-8 读取。
    encoding = "oem" if Path(command[0]).name.lower() == "where.exe" else "utf-8-sig"
    result.update(stdout=stdout.decode(encoding, errors="replace"),
                  stderr=stderr.decode(encoding, errors="replace"), encoding=encoding,
                  stdout_base64=base64.b64encode(stdout).decode("ascii"),
                  stderr_base64=base64.b64encode(stderr).decode("ascii"))
    return result


def sha256(path):
    """分块计算原始文件 SHA256，不更改安装文件。"""
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    """采集当前 Windows 安装身份，并写入当前 task 的证据文件。"""
    if sys.platform != "win32":
        raise SystemExit("Windows probe only")
    cwd = Path.cwd()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shim", help="显式只读 shim 路径；不改变 PATH 发现结果")
    args = parser.parse_args()
    commands = {name: run(["where.exe", name], cwd) for name in ("codebuddy", "buddycn")}
    entry = resolve_entry(commands["buddycn"])
    discovery_status = entry["status"]
    source = "path_discovery"
    if args.shim:
        entry = resolve_entry({"exit_code": 0, "stdout": str(Path(args.shim).resolve())})
        source = "explicit_shim_argument"
    result = {"task": "CB5-001", "captured_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "platform": sys.platform, "discovery_command_name": "buddycn",
              "where_executable": shutil.which("where.exe"), "entry_source": source,
              "discovery_status": discovery_status,
              "discovery": commands, "entry": entry, "actual_probe_status": "BLOCKED"}
    if entry["status"] == "FOUND":
        exe, cli = Path(entry["canonical_exe"]), Path(entry["cli_js"])
        result["bin_listing"] = sorted(p.name for p in Path(entry["shim_path"]).parent.iterdir())
        result["bin_in_process_path"] = str(Path(entry["shim_path"]).parent).lower() in [p.rstrip("\\").lower() for p in os.environ.get("PATH", "").split(";")]
        # 与 shim 一致，只在子进程设置两个环境项，使用真实 EXE + CLI JS。
        raw = {flag: run([str(exe), str(cli), flag], cwd, entry["environment"]) for flag in ("--version", "--help")}
        result["raw_cli"] = raw
        quoted = str(exe).replace("'", "''")
        script = "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $v=(Get-Item -LiteralPath '" + quoted + "').VersionInfo; [ordered]@{FileVersion=$v.FileVersion;ProductVersion=$v.ProductVersion;ProductName=$v.ProductName}|ConvertTo-Json -Compress"
        metadata_command = run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script], cwd)
        result["pe_metadata_command"] = metadata_command
        if metadata_command["exit_code"] != 0:
            raise RuntimeError("PE metadata command failed")
        pe = json.loads(metadata_command["stdout"])
        result["pe_version_info"] = pe
        result.update(version_candidate(raw["--version"], pe))
        result["embedded_metadata"] = {}
        for filename, keys in (("product.json", ("applicationName", "version", "commit", "quality")), ("package.json", ("name", "version"))):
            path = exe.parent / "resources" / "app" / filename
            metadata = json.loads(path.read_text(encoding="utf-8"))
            result["embedded_metadata"][filename] = {"path": str(path), "sha256": sha256(path), "fields": {k: metadata.get(k) for k in keys}}
        result["identities"] = [{"role": role, "path": entry[key], "sha256": sha256(entry[key])} for role, key in (("exe", "canonical_exe"), ("cli", "cli_js"), ("shim", "shim_path"))]
        product = result["embedded_metadata"]["product.json"]["fields"]
        result["supported_version_candidate_evidence"] = {
            "product_version": result["product_version_candidate"],
            "exe_sha256": result["identities"][0]["sha256"], "cli_sha256": result["identities"][1]["sha256"],
            "commit": product["commit"], "applicationName": product["applicationName"], "support_status": "UNVERIFIED"}
        result["binary_identity_status"] = "PASS" if result["product_version_candidate"] and all(r["exit_code"] == 0 for r in raw.values()) else "PARTIAL"
        result["actual_probe_status"] = "PASS" if result["binary_identity_status"] == "PASS" and discovery_status == "FOUND" else "PARTIAL"
        (TASK / "binary.sha256").write_text("".join(f'{i["sha256"]}  {i["path"]}\n' for i in result["identities"]), encoding="utf-8")
    (TASK / "probe-result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"actual_probe_status": result["actual_probe_status"], "entry_status": entry["status"]}))
    return 0 if result["actual_probe_status"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
