"""Real stdio MCP framing and durable cross-process memory; no network/credentials."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from smoke import BINARY


class MemoryRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="accessor-memory-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = dict(os.environ, ACC_HOME=str(self.root / "settings"))
        self.a = self.root / "a"
        self.b = self.root / "b"
        self.a.mkdir()
        self.b.mkdir()

    def mcp(self, workspace, calls):
        frames = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
        ]
        frames += [{"jsonrpc": "2.0", "id": i + 2, "method": "tools/call", "params": {"name": name, "arguments": args}} for i, (name, args) in enumerate(calls)]
        result = subprocess.run([str(BINARY), "mcp", "--workspace", str(workspace)],
                                input="".join(json.dumps(f) + "\n" for f in frames),
                                env=self.env, capture_output=True, text=True, encoding="utf-8", timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(replies), len(calls) + 1)
        self.assertEqual(replies[0]["result"]["protocolVersion"], "2025-11-25")
        return [reply["result"] for reply in replies[1:]]

    def test_shared_process_store_scopes_conflicts_and_forgetting(self):
        fact = dict(scope="project", key="language", text="Rust", source="user chose Rust", revision=0)
        first = self.mcp(self.a, [("memory_save", fact), ("memory_save", dict(fact, scope="global", key="tone", text="Be concise"))])
        self.assertTrue(all(not item["isError"] for item in first))
        other = self.mcp(self.b, [("memory_search", {"query": "", "limit": 100})])
        self.assertEqual([e["key"] for e in json.loads(other[0]["content"][0]["text"])["entries"]], ["tone"])
        result = self.mcp(self.a, [
            ("memory_save", dict(fact, text="Python")),
            ("memory_forget", dict(scope="project", key="language", revision=1)),
            ("memory_save", dict(fact, revision=2)),
            ("memory_search", {"query": "language"}),
        ])
        self.assertTrue(result[0]["isError"])
        self.assertFalse(result[1]["isError"])
        self.assertTrue(result[2]["isError"])
        entry = json.loads(result[3]["content"][0]["text"])["entries"][0]
        self.assertTrue(entry["deleted"])
        self.assertEqual(entry["text"], "")
        self.assertEqual(entry["source"], "")

    def test_organizer_mcp_validates_execution_and_reports_real_receipts(self):
        results = self.mcp(self.a, [
            ("organizer_control", {"directive": {"action": "schedule", "prompt": "hello", "delay_seconds": 100}}),
            ("organizer_control", {"directive": {"action": "alarm", "label": "tea", "delay_seconds": 100}}),
            ("organizer_status", {}),
            ("organizer_control", {"directive": {"action": "stop_alarm"}}),
        ])
        self.assertTrue(results[0]["isError"])
        self.assertFalse(results[1]["isError"])
        self.assertIn("tea", results[2]["content"][0]["text"])
        self.assertTrue(results[3]["isError"])


if __name__ == "__main__":
    unittest.main()
