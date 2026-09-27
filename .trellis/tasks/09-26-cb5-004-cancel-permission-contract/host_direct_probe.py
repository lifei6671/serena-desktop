import os
from pathlib import Path
import gold_band_probe as wire

NODE = r"C:\nvm4w\nodejs\node.exe"
SCRIPT = r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy"

def main():
    wire.EVIDENCE = wire.ROOT / "evidence/host-direct-codebuddy-proof"
    wire.EVIDENCE.mkdir(parents=True, exist_ok=True)
    if (wire.EVIDENCE / "attempt-started.json").exists():
        raise RuntimeError("Host direct proof already attempted; no replay")
    wire.LAUNCHER = [NODE, SCRIPT, "--acp"]
    wire.EXECUTION = [NODE, SCRIPT, "--acp"]
    diagnostics = {
        "status": "READY",
        "launcher": "absolute-node-plus-codebuddy-script",
        "nodeExists": Path(NODE).is_file(),
        "scriptExists": Path(SCRIPT).is_file(),
        "environmentMode": "host-standard-user-env",
    }
    wire.save("launcher-resolution.json", diagnostics)
    return wire.run(child_env=os.environ.copy(), launcher_diagnostics=diagnostics)

if __name__ == "__main__":
    raise SystemExit(main())
