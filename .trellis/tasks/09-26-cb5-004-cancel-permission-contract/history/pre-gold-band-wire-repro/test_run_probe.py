"""runner 证据新鲜度与 replay 禁止回归；不调用真实 CLI。"""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch, Mock
import run_probe


class RunnerTests(unittest.TestCase):
    """每个测试独立 evidence，不改真实运行记录。"""

    def test_actual_bytes_hash(self):
        """hash 只能来自捕获字节。"""
        value = {"x": {"kind": "file", "bytes": list(b"abc")}}
        run_probe.hash_manifests(value)
        self.assertEqual(value["x"]["sha256"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        self.assertNotIn("bytes", value["x"])

    def test_failed_c_gate_forbids_scenario(self):
        """C尚未成功，不能启动任何真实场景。"""
        with tempfile.TemporaryDirectory() as folder:
            with patch.object(run_probe, "EVIDENCE", Path(folder)), patch("sys.argv", ["runner", "before"]), patch.object(run_probe.subprocess, "Popen") as spawn:
                with self.assertRaises(RuntimeError):
                    run_probe.main()
                spawn.assert_not_called()

    def test_existing_identity_forbids_diagnostic_replay(self):
        """只有durable identity而没有result也是未知结果，不能再次启动。"""
        with tempfile.TemporaryDirectory() as folder:
            evidence=Path(folder)
            (evidence / "session-new-repair-result.identity.json").write_text("{}")
            with patch.object(run_probe,"EVIDENCE",evidence), patch("sys.argv",["runner","diagnostic"]), patch.object(run_probe.subprocess,"Popen") as spawn:
                with self.assertRaises(RuntimeError):
                    run_probe.main()
                spawn.assert_not_called()

    def test_missing_current_result_fails(self):
        """child exit0但本次输出缺失，必须失败。"""
        with tempfile.TemporaryDirectory() as folder:
            evidence = Path(folder)
            process = Mock(pid=123)
            process.wait.return_value = 0
            with patch.object(run_probe, "EVIDENCE", evidence), patch("sys.argv", ["runner", "diagnostic"]), patch.object(run_probe.subprocess, "Popen", return_value=process):
                self.assertEqual(run_probe.main(), 2)
            self.assertTrue(json.loads((evidence / "diagnostic-failure.json").read_text())["resultMissing"])


if __name__ == "__main__":
    unittest.main()
