"""仅task-local的Windows child PATH证明；一次Gold Band JSONL new，不修改产品。"""
from pathlib import Path
import ntpath
import os
import re
import winreg
import gold_band_probe as wire

NPX_PATH = r"C:\nvm4w\nodejs\npx.cmd"
NODE_DIRECTORY = r"C:\nvm4w\nodejs"
EXTENSIONS = (".exe", ".com", ".cmd", ".bat")
PATH_VARIABLES = ("NVM_HOME", "NVM_SYMLINK", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "SYSTEMROOT", "WINDIR", "PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432", "SYSTEMDRIVE", "HOMEDRIVE", "HOMEPATH")


def registry_paths():
    """只查询PATH及目录变量白名单，不枚举registry环境、token或credential。"""
    result = {}
    for name, hive, key_name in [
        ("user", winreg.HKEY_CURRENT_USER, r"Environment"),
        ("machine", winreg.HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
    ]:
        values = {}
        try:
            with winreg.OpenKey(hive, key_name, 0, winreg.KEY_READ) as key:
                for variable in ("Path",) + PATH_VARIABLES:
                    try:
                        value, kind = winreg.QueryValueEx(key, variable)
                        if kind in (winreg.REG_SZ, winreg.REG_EXPAND_SZ) and isinstance(value, str):
                            values[variable.upper()] = value
                    except FileNotFoundError:
                        continue
        except OSError:
            values = {}
        result[name] = values
    return result


def expand_directory(value, variables):
    """仅展开已读取的目录变量，大小写不敏感；未知引用不当作可执行目录。"""
    for _ in range(8):
        expanded = re.sub(r"%([^%]+)%", lambda m: variables.get(m[1].upper(), m[0]), value)
        if expanded == value:
            break
        value = expanded
    return None if "%" in value else value.strip().strip('"')


def path_entries(explicit_path, process_path, user_path, machine_path, common_dirs, variables):
    """explicit→process→HKCU→HKLM→common，Windows大小写不敏感去重。"""
    result = []
    seen = set()
    for source in [explicit_path, process_path, user_path, machine_path, ";".join(common_dirs)]:
        for candidate in source.split(";"):
            expanded = expand_directory(candidate, variables)
            if not expanded:
                continue
            normalized = ntpath.normpath(expanded)
            if not ntpath.isabs(normalized):
                continue
            identity = ntpath.normcase(normalized).casefold()
            if identity not in seen:
                result.append(normalized)
                seen.add(identity)
    return result


def resolve_bare(name, entries, exists):
    """bare executable只选exe/com/cmd/bat，不执行ps1或无扩展shim。"""
    if ntpath.basename(name) != name:
        return None
    suffix = ntpath.splitext(name)[1].lower()
    if suffix and suffix not in EXTENSIONS:
        return None
    names = [name] if suffix else [name + extension for extension in EXTENSIONS]
    for directory in entries:
        for executable in names:
            candidate = ntpath.join(directory, executable)
            if exists(candidate):
                return candidate
    return None


def launcher_resolution(npx, entries, exists):
    """已知npm npx.cmd先用同目录node.exe，否则才解析child PATH中的node。"""
    sibling = ntpath.join(ntpath.dirname(npx), "node.exe")
    node = sibling if exists(sibling) else resolve_bare("node", entries, exists)
    status = "READY" if exists(npx) and node else "LAUNCHER_DEPENDENCY_UNAVAILABLE"
    return {"status": status, "failureLayer": "launcher" if status != "READY" else None, "acpFailure": False, "resolvedNpxPath": npx if exists(npx) else None, "resolvedNodePath": node, "siblingNodeExists": exists(sibling), "resolvedBareNpxPath": resolve_bare("npx", entries, exists), "resolvedCodebuddyPath": resolve_bare("codebuddy", entries, exists), "childPathContainsNvmSymlink": ntpath.normcase(NODE_DIRECTORY) in {ntpath.normcase(x) for x in entries}}


def child_environment(explicit_path=NODE_DIRECTORY):
    """复用精确目录变量读取与PATH组合，返回仅供child继承的环境和安全诊断。"""
    registry = registry_paths()
    # machine→user→process 的目录变量覆盖，与PATH源的搜索优先序分别处理。
    variables = {key: value for key, value in registry["machine"].items() if key != "PATH"}
    variables.update({key: value for key, value in registry["user"].items() if key != "PATH"})
    for name in PATH_VARIABLES:
        value = os.environ.get(name)
        if value:
            variables[name] = value
    entries = path_entries(explicit_path, os.environ.get("PATH", ""), registry["user"].get("PATH", ""), registry["machine"].get("PATH", ""), [r"C:\Windows\System32", r"C:\Windows", r"C:\Program Files\nodejs"], variables)
    diagnostics = launcher_resolution(NPX_PATH, entries, lambda path: Path(path).is_file())
    diagnostics["registryPathSourcesRead"] = {name: "PATH" in values for name, values in registry.items()}
    # 子进程原样继承其余父环境；只覆写PATH，不解析/输出其它值。
    child_env = os.environ.copy()
    for key in list(child_env):
        if key.upper() == "PATH":
            del child_env[key]
    child_env["PATH"] = ";".join(entries)
    return child_env, diagnostics


def run():
    """只替换子进程PATH，不全局改环境，不记录完整PATH或其它环境值。"""
    wire.EVIDENCE = wire.ROOT / "evidence/path-resolver-proof"
    wire.EVIDENCE.mkdir(parents=True, exist_ok=True)
    child_env, diagnostics = child_environment()
    if (wire.EVIDENCE / "launcher-resolution.json").exists():
        raise RuntimeError("Proof already attempted; no replay")
    wire.save("launcher-resolution.json", diagnostics)
    if diagnostics["status"] != "READY":
        wire.save("wire-result.json", {"status": diagnostics["status"], "stage": "launcher_resolution", "initializeSent": False, "sessionNewSent": False, "acpFailure": False})
        return 1
    wire.LAUNCHER = [NPX_PATH, "-y", "@tencent-ai/codebuddy-code@2.158.0", "--acp"]
    wire.EXECUTION = [r"C:\Windows\System32\cmd.exe", "/d", "/s", "/c", " ".join(wire.LAUNCHER)]
    return wire.run(child_env=child_env, launcher_diagnostics=diagnostics)


if __name__ == "__main__":
    raise SystemExit(run())
