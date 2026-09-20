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

    def test_settings_patch_is_atomic_and_security_fields_are_not_exposed(self):
        results = self.mcp(self.a, [
            ("settings_update", {"changes":{"tts.speed":1.2,"tts.volume":0.5,"sounds.alarm":0.3}}),
            ("settings_update", {"changes":{"tts.speed":1.4,"sounds.wake":-1}}),
            ("settings_update", {"changes":{"approvals.reviewer":"auto"}}),
            ("settings_read", {}),
            ("usage_status", {"refresh":False}),
        ])
        self.assertFalse(results[0]["isError"])
        self.assertIn("next Accessor launch",results[0]["content"][0]["text"])
        self.assertTrue(results[1]["isError"])
        self.assertTrue(results[2]["isError"])
        values=json.loads(results[3]["content"][0]["text"])["values"]
        self.assertAlmostEqual(values["tts.speed"],1.2)
        self.assertNotIn("prompt",values)
        self.assertNotIn("codex-bin",values)
        quotas=json.loads(results[4]["content"][0]["text"])
        self.assertTrue(all(not p["available"] and p["age_seconds"] is None for p in quotas["providers"]))

    def test_statusline_ingest_preserves_last_sample_when_fields_disappear(self):
        def ingest(payload):
            return subprocess.run([str(BINARY),"usage","--ingest","claude"],env=self.env,input=json.dumps(payload),capture_output=True,text=True,encoding="utf-8",timeout=10)
        self.assertEqual(ingest({"rate_limits":{"five_hour":{"used_percentage":23,"resets_at":2000000000}}}).returncode,0)
        self.assertNotEqual(ingest({"context_window":{"used_percentage":99}}).returncode,0)
        result=self.mcp(self.a,[("usage_status",{"refresh":False})])[0]
        rows=json.loads(result["content"][0]["text"])["providers"]
        claude=next(p for p in rows if p["harness"]=="claude")
        self.assertEqual(claude["buckets"][0]["remaining_percent"],77)
        cache=self.root/"settings"/"quota-claude.json"
        old=json.loads(cache.read_text(encoding="utf-8"))
        old["observed_at"]=1
        cache.write_text(json.dumps(old),encoding="utf-8")
        # A broken memory store must not prevent unrelated quota/settings reads.
        (self.root/"settings"/"memory.json").write_text("not json",encoding="utf-8")
        results=self.mcp(self.a,[("usage_status",{"refresh":False}),("settings_read",{})])
        self.assertTrue(all(not r["isError"] for r in results))
        stale=next(p for p in json.loads(results[0]["content"][0]["text"])["providers"] if p["harness"]=="claude")
        self.assertTrue(stale["stale"])
        self.assertEqual(stale["buckets"][0]["remaining_percent"],77)


if __name__ == "__main__":
    unittest.main()
