"""纯临时安装布局 fixture，不调用真实 CodeBuddy 或 ACP。"""

from pathlib import Path
import tempfile
import unittest

from probe import parse_version, resolve_entry, version_candidate

SHIM = '@echo off\nset ELECTRON_RUN_AS_NODE=1\n"%~dp0..\\CodeBuddy CN.exe" "%~dp0..\\resources\\app\\out\\cli.js" %*\n'
PE = {"ProductName": "CodeBuddy CN", "ProductVersion": "4.12.0", "FileVersion": "1.106.1.0"}


class ProbeTests(unittest.TestCase):
    """冻结发现与版本失败关闭行为。"""

    def setUp(self):
        """每个用例创建隔离安装布局，结束后自动删除。"""
        temporary = tempfile.TemporaryDirectory(prefix="cb5-001-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.shim = self.root / "bin" / "buddycn.cmd"
        self.shim.parent.mkdir()
        self.shim.write_text(SHIM, encoding="utf-8")
        self.exe = self.root / "CodeBuddy CN.exe"
        self.exe.write_bytes(b"fixture-exe")
        self.cli = self.root / "resources" / "app" / "out" / "cli.js"
        self.cli.parent.mkdir(parents=True)
        self.cli.write_bytes(b"fixture-cli")
        self.where = {"exit_code": 0, "stdout": str(self.shim) + "\r\n"}

    def test_found(self):
        """找到 fixture 时解析实际路径并保留独立 CLI 版本。"""
        entry = resolve_entry(self.where)
        self.assertEqual(entry["status"], "FOUND")
        self.assertEqual(entry["canonical_exe"], str(self.exe.resolve()))
        self.assertEqual(entry["cli_js"], str(self.cli.resolve()))
        result = version_candidate({"exit_code": 0, "stdout": "1.106.1\r\n"}, PE)
        self.assertEqual(result["parsed_cli_version"]["value"], "1.106.1")
        self.assertEqual(result["product_version_candidate"]["value"], "4.12.0")

    def test_missing_discovery(self):
        """where 未找到命令时不把路径文本当成有效发现。"""
        self.assertEqual(resolve_entry({"exit_code": 1, "stdout": str(self.shim)}), {"status": "NOT_FOUND"})

    def test_missing_shim(self):
        """不存在的 shim 不构成安装证据。"""
        self.shim.unlink()
        self.assertEqual(resolve_entry(self.where)["status"], "NOT_FOUND")

    def test_missing_real_entries(self):
        """EXE 或 CLI 任一缺失均拒绝成功发现。"""
        for path in (self.exe, self.cli):
            with self.subTest(path=path.name):
                content = path.read_bytes()
                path.unlink()
                self.assertEqual(resolve_entry(self.where)["status"], "NOT_FOUND")
                path.write_bytes(content)

    def test_malformed_shim(self):
        """未知 shim 语法不能猜测为已知布局。"""
        self.shim.write_text("echo unexpected", encoding="utf-8")
        self.assertEqual(resolve_entry(self.where)["status"], "MALFORMED_SHIM")

    def test_empty_version(self):
        """空输出以及只有空白均是解析失败。"""
        for stdout in ("", " \r\n"):
            with self.subTest(stdout=stdout):
                self.assertEqual(parse_version({"exit_code": 0, "stdout": stdout})["reason"], "EMPTY")

    def test_malformed_version(self):
        """噪音、四段版本和混合输出不能冒充 CLI 版本。"""
        for stdout in ("garbage", "4.12", "4.12.0.0", "04.12.0", "4.12.0\nnoise", "CodeBuddy 4.12.0"):
            with self.subTest(stdout=stdout):
                self.assertEqual(parse_version({"exit_code": 0, "stdout": stdout})["reason"], "MALFORMED")

    def test_structured_cli_version(self):
        """实测三行结构可解析，但仍不能冒充 PE 产品候选。"""
        raw = {"exit_code": 0, "stdout": "1.106.1\nb4c35ed08ffb428910211608831a314565c1256e\nx64\n"}
        result = version_candidate(raw, PE)
        self.assertEqual(result["parsed_cli_version"]["value"], "1.106.1")
        self.assertEqual(result["parsed_cli_version"]["source"], "cli_stdout")
        self.assertEqual(result["parsed_cli_version"]["architecture"], "x64")
        self.assertEqual(result["product_version_candidate"]["value"], "4.12.0")
        for stdout in ("1.106.1\ninvalid\nx64", raw["stdout"] + "noise", raw["stdout"].replace("x64", "unknown")):
            self.assertEqual(parse_version({"exit_code": 0, "stdout": stdout})["status"], "REJECTED")

    def test_failed_process(self):
        """非零退出和超时不能通过版本解析。"""
        for raw in ({"exit_code": 1, "stdout": "4.12.0"}, {"exit_code": None, "timed_out": True, "stdout": "4.12.0"}):
            self.assertEqual(parse_version(raw)["reason"], "PROCESS_FAILED")

    def test_explicit_metadata_fallback(self):
        """CLI 空或畸形时必须保留失败及明确的 PE 候选来源。"""
        for stdout in ("", "malformed"):
            result = version_candidate({"exit_code": 0, "stdout": stdout}, PE)
            self.assertEqual(result["parsed_cli_version"]["status"], "REJECTED")
            self.assertIsNone(result["parsed_cli_version"]["value"])
            self.assertEqual(result["product_version_candidate"], {"value": "4.12.0", "source": "pe_product_version", "supported": False})

    def test_no_base_version_fallback(self):
        """缺失产品来源不能拿 FileVersion 或 embedded 版本补齐。"""
        for pe in ({"FileVersion": "1.106.1.0", "version": "1.106.1"}, {**PE, "ProductVersion": "invalid"}, {**PE, "ProductName": "Other"}):
            self.assertIsNone(version_candidate({"exit_code": 0, "stdout": ""}, pe)["product_version_candidate"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
