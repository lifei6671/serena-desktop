from pathlib import Path
import exact_launcher_probe as exact
import gold_band_probe as wire
from path_resolver_probe import child_environment, NODE_DIRECTORY, NPX_PATH

def main():
    wire.EVIDENCE = wire.ROOT / "evidence/host-exact-launcher-proof"
    wire.EVIDENCE.mkdir(parents=True, exist_ok=True)
    if (wire.EVIDENCE / "attempt-started.json").exists():
        raise RuntimeError("Host exact proof already attempted; no replay")
    explicit = ";".join([NODE_DIRECTORY, r"C:\Windows\System32", r"C:\Windows"])
    child_env, diagnostics = child_environment(explicit)
    diagnostics["commandLineMode"] = "RAW_WINDOWS_STRING"
    diagnostics["flags"] = ["/e:ON", "/v:OFF", "/d", "/c"]
    diagnostics["runner"] = "serena-command-host"
    wire.save("launcher-resolution.json", diagnostics)
    wire.LAUNCHER = [NPX_PATH, "-y", "@tencent-ai/codebuddy-code@2.158.0", "--acp"]
    wire.EXECUTION = exact.COMMAND_LINE
    return wire.run(
        child_env=child_env,
        launcher_diagnostics=diagnostics,
        cwd_projector=exact.project_external_cwd,
    )

if __name__ == "__main__":
    raise SystemExit(main())
