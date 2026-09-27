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

    def test_prompt_sent_forbids_followup(self):
        """即使调用方明确要求 followup，也不能重放已发送 prompt。"""
        with tempfile.TemporaryDirectory() as folder:
            evidence = Path(folder)
            (evidence / "cancel-before-result.json").write_text(json.dumps({"prompt": {"request": {"id": "sent"}}}))
            with patch.object(run_probe, "EVIDENCE", evidence), patch("sys.argv", ["runner", "--fresh-session-new-followup"]), patch.object(run_probe.subprocess, "Popen") as spawn:
                with self.assertRaises(RuntimeError):
                    run_probe.main()
                spawn.assert_not_called()

    def test_missing_current_result_never_accepts_previous(self):
        """child exit0但本次输出缺失，必须失败，不复用旧报告。"""
        with tempfile.TemporaryDirectory() as folder:
            evidence = Path(folder)
            old = {"prompt": {"request": None}, "protocol": {"stage": "session/new"}, "harnessExitCode": 0, "cleanup": {"succeeded": True}, "workspaceDeleted": True}
            (evidence / "cancel-before-result.json").write_text(json.dumps(old))
            process = Mock(pid=123)
            process.wait.return_value = 0
            with patch.object(run_probe, "EVIDENCE", evidence), patch("sys.argv", ["runner", "--fresh-session-new-followup"]), patch.object(run_probe.subprocess, "Popen", return_value=process):
                self.assertEqual(run_probe.main(), 2)
            self.assertTrue(json.loads((evidence / "before-failure.json").read_text())["resultMissing"])
            self.assertEqual(json.loads((evidence / "cancel-before-result.json").read_text()), old)


if __name__ == "__main__":
    unittest.main()
