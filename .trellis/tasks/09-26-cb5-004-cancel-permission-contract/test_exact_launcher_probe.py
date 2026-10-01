"""精确CMD引用/cwd边界证明，mock Popen，不执行真实CLI。"""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import exact_launcher_probe as exact
import gold_band_probe as wire


class ExactLauncherTests(unittest.TestCase):
    """直接检查交给Windows Popen的原始字符串与cwd。"""

    def test_exact_raw_command_line_and_flags(self):
        """固定Host quote形状，禁止/s和Roaming launcher。"""
        self.assertEqual(exact.COMMAND_LINE, '"C:\\Windows\\System32\\cmd.exe" /e:ON /v:OFF /d /c ""C:\\nvm4w\\nodejs\\npx.cmd" -y @tencent-ai/codebuddy-code@2.158.0 --acp"')
        self.assertNotIn(" /s ", exact.COMMAND_LINE.lower())
        self.assertNotIn("roaming", exact.COMMAND_LINE.lower())

    def test_actual_popen_input_is_raw_string_and_normal_cwd(self):
        """验证真实run接线：无list2cmdline，cwd投影普通绝对Win32。"""
        with tempfile.TemporaryDirectory() as folder:
            env = {"PATH": r"C:\nvm4w\nodejs;C:\Windows\System32;C:\Windows"}
            with patch.object(wire, "ROOT", Path(folder)), patch.object(exact, "child_environment", return_value=(env, {"status": "READY"})), patch.object(wire.subprocess, "Popen", side_effect=OSError("fixture spawn denied")) as spawn:
                self.assertEqual(exact.run(), 1)
                args, kwargs = spawn.call_args
                self.assertIsInstance(args[0], str)
                self.assertEqual(args[0], exact.COMMAND_LINE)
                self.assertFalse(str(kwargs["cwd"]).startswith("\\\\?\\"))
                self.assertRegex(str(kwargs["cwd"]), r"^[A-Za-z]:\\")
                self.assertEqual(kwargs["env"], env)
                self.assertNotIn("shell", kwargs)
                report = json.loads((Path(folder) / "evidence/exact-launcher-proof/wire-result.json").read_text(encoding="utf-8"))
                self.assertEqual(report["executionCommandLine"], exact.COMMAND_LINE)
                self.assertTrue(report["workspaceDeleted"])

    def test_local_verbatim_projection_preserves_authority_input(self):
        """local canonical Authority不变，仅external cwd使用普通形式。"""
        canonical = r"\\?\C:\Users\fixture\Temp\probe"
        self.assertEqual(exact.project_external_cwd(canonical), r"C:\Users\fixture\Temp\probe")
        self.assertEqual(canonical, r"\\?\C:\Users\fixture\Temp\probe")
        self.assertEqual(exact.project_external_cwd(r"C:\Users\fixture"), r"C:\Users\fixture")
        with self.assertRaises(ValueError):
            exact.project_external_cwd(r"\\?\UNC\server\share")

    def test_stderr_only_static_categories(self):
        """只输出固定分类，正文/credential哨兵不能返回或落盘。"""
        self.assertEqual(wire.stderr_category(b"npm error code ENOENT\nsecret=PRIVATE"), "NPM_ENOENT")
        self.assertEqual(wire.stderr_category(b"UNC paths are not supported. PRIVATE"), "CMD_CWD_UNSUPPORTED")
        self.assertEqual(wire.stderr_category(b"'node' is not recognized as an internal or external command\nPRIVATE"), "CMD_EXECUTABLE_NOT_FOUND")
        self.assertIsNone(wire.stderr_category(b"token=PRIVATE\nrandom non-error body"))


if __name__ == "__main__":
    unittest.main()
