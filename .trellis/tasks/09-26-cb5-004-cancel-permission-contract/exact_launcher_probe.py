"""精确复现Host已观察的Windows CMD flags/quote与普通external cwd。"""
import ntpath
import re
from pathlib import Path
import gold_band_probe as wire
from path_resolver_probe import child_environment, NPX_PATH, NODE_DIRECTORY

# 使用Windows原始command line字符串交给Popen，不能经过list2cmdline重写尾部引号。
COMMAND_LINE = r'"C:\Windows\System32\cmd.exe" /e:ON /v:OFF /d /c ""C:\nvm4w\nodejs\npx.cmd" -y @tencent-ai/codebuddy-code@2.158.0 --acp"'


def project_external_cwd(canonical):
    """只把local verbatim drive路径投影到普通Win32；Authority原root另行保留。"""
    projected = canonical
    if projected.startswith("\\\\?\\"):
        projected = projected[4:]
    if not re.match(r"^[A-Za-z]:\\", projected) or projected.startswith("\\\\"):
        raise ValueError("EXTERNAL_CWD_MUST_BE_LOCAL_WIN32_DRIVE_PATH")
    return ntpath.normpath(projected)


def run():
    """固定一次精确launcher；失败不retry，不发送任何prompt。"""
    wire.EVIDENCE = wire.ROOT / "evidence/exact-launcher-proof"
    wire.EVIDENCE.mkdir(parents=True, exist_ok=True)
    if (wire.EVIDENCE / "launcher-resolution.json").exists():
        raise RuntimeError("Exact launcher proof already attempted; no replay")
    explicit = ";".join([NODE_DIRECTORY, r"C:\Windows\System32", r"C:\Windows"])
    child_env, diagnostics = child_environment(explicit)
    diagnostics["commandLineMode"] = "RAW_WINDOWS_STRING"
    diagnostics["flags"] = ["/e:ON", "/v:OFF", "/d", "/c"]
    wire.save("launcher-resolution.json", diagnostics)
    if diagnostics["status"] != "READY":
        wire.save("wire-result.json", {"status": diagnostics["status"], "stage": "launcher_resolution", "sessionNewSent": False})
        return 1
    wire.LAUNCHER = [NPX_PATH, "-y", "@tencent-ai/codebuddy-code@2.158.0", "--acp"]
    wire.EXECUTION = COMMAND_LINE
    return wire.run(child_env=child_env, launcher_diagnostics=diagnostics, cwd_projector=project_external_cwd)


if __name__ == "__main__":
    raise SystemExit(run())
