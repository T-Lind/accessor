"""Offline coordinator, organizer, and all-harness regression tests."""
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time
import unittest

from smoke import App, BINARY, ROOT


class RuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="accessor-runtime-")
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.env = dict(os.environ, ACC_HOME=str(self.home / "settings"), ACC_MCP_REGISTRY=str(self.home / "agy-mcp.json"))
        self.env["PATH"] = str(self.home) + os.pathsep + self.env["PATH"]
        for name, kind in [("claude", "claude"), ("agy", "antigravity"), ("codex-fixture", "codex")]:
            fixture = ROOT / "tests" / ("fake_codex.py" if kind == "codex" else "fake_stdio.py")
            shim = self.home / (name + (".cmd" if os.name == "nt" else ""))
            if os.name == "nt":
                shim.write_text(f'@echo off\n"{sys.executable}" "{fixture}" {kind} %*\n')
            else:
                shim.write_text(f'#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(fixture))} {kind} "$@"\n')
                shim.chmod(0o700)
        self.codex = self.home / ("codex-fixture.cmd" if os.name == "nt" else "codex-fixture")

    def cli(self, *args, ok=True):
        result = subprocess.run([str(BINARY), *args], env=self.env, cwd=ROOT,
                                capture_output=True, text=True, encoding="utf-8", timeout=15)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0)
        return result

    def config(self, key, value):
        self.cli("config", "set", key, value)

    def app(self, harness="codex"):
        self.config("routing.main", harness)
        app = App("--codex-bin", str(self.codex), env=self.env)
        self.addCleanup(app.close)
        return app

    def test_continuation_before_harness_ready_keeps_original_request(self):
        self.env["ACC_FIXTURE_START_DELAY"] = "0.7"
        app = self.app()
        app.send("29 first part of the request")
        app.expect("You: first part")
        app.send("and the second part")
        app.expect("Interrupting;")
        app.expect("fixture: first part of the request")
        app.expect("Additional user speech: and the second part")

    def test_continuation_while_thinking_needs_no_wake_code(self):
        app = self.app()
        app.send("29 hold")
        app.expect("▸ hold")
        app.send("and include Chicago please")
        app.expect("Interrupting;")
        app.expect("fixture: and include Chicago please")
        self.assertNotIn("Agent: interrupted", "".join(app.seen))

    def test_streaming_is_opt_in_and_does_not_select_cloud_implicitly(self):
        self.config("stt.streaming", "true")
        settings = json.loads((self.home / "settings" / "config.json").read_text(encoding="utf-8"))
        self.assertTrue(settings["stt"]["streaming"])
        self.assertEqual(settings["stt"]["conversation"], "local")
        app = self.app()
        app.send("29 private analytics sentinel")
        app.expect("fixture: private analytics sentinel")
        app.send("/analytics")
        app.expect("Speech health")
        app.expect("Latency")
        app.close()
        saved = json.loads((self.home / "settings" / "analytics.json").read_text(encoding="utf-8"))
        self.assertNotIn("session", saved)
        self.assertNotIn("private analytics sentinel", json.dumps(saved))

    def test_alarm_stop_is_an_agent_control_on_every_harness(self):
        for harness in ("codex", "claude", "antigravity"):
            with self.subTest(harness=harness):
                app = self.app(harness)
                app.send("29 stop the alarm")
                app.expect("You: stop the alarm")
                app.expect("No alarm is ringing.")
                self.assertNotIn("Stopped. Waiting for wake code.", "".join(app.seen))
                app.close()

    def test_live_mcp_controls_reach_the_running_session(self):
        app = self.app()
        app.send("29 hold")
        app.expect("hold")
        sessions = list((self.home / "settings" / "sessions").glob("*.json"))
        self.assertEqual(len(sessions), 1)
        attached = dict(self.env, ACC_CONTROL_ENDPOINT=sessions[0].read_text(encoding="utf-8"))

        def control(action):
            frames = [
                {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}},
                {"jsonrpc": "2.0", "method": "notifications/initialized"},
                {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "session_control", "arguments": {"action": action}}},
            ]
            result = subprocess.run([str(BINARY), "mcp"], input="".join(json.dumps(f)+"\n" for f in frames),
                                    cwd=ROOT, env=attached, capture_output=True, text=True, encoding="utf-8", timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            reply = json.loads(result.stdout.splitlines()[-1])["result"]
            self.assertFalse(reply["isError"], reply)
            return json.loads(reply["content"][0]["text"])

        self.assertTrue(control("status")["awake"])
        self.assertFalse(control("sleep")["awake"])
        app.expect("MCP: asleep")
        self.assertFalse(control("status")["awake"])
        self.assertFalse(control("stop_alarm")["stopped"])
        registry = json.loads((self.home / "agy-mcp.json").read_text(encoding="utf-8"))
        self.assertIn("accessor", registry["mcpServers"])
        app.close()
        self.assertFalse(sessions[0].exists())

    def test_bare_wake_interrupts_worker_then_waits_without_a_reply(self):
        app = self.app()
        app.send("29 delegate hold fixture")
        app.expect("Started worker:")
        app.expect("worker: ")
        app.send("29")
        app.expect("Listening. Take your time")
        before = len(app.seen)
        time.sleep(.3)
        app.send("/status")
        app.expect("| idle")
        self.assertNotIn("Agent:", "".join(app.seen[before:]))
        app.send("hello after interruption")
        app.expect("Agent: fixture:")
        app.expect("hello after interruption")

    def test_compaction_honors_harness_model_and_reasoning(self):
        for harness in ("codex", "claude", "antigravity"):
            with self.subTest(harness=harness):
                self.config("routing.compaction-harness", harness)
                self.config("routing.compaction-model", "fixture-summary")
                self.config("routing.compaction-reasoning", "high")
                app = self.app()
                app.send("29 hello")
                app.expect("fixture: hello")
                app.send("/compact")
                app.expect("Compacted Accessor-owned history")
                app.send("/context")
                app.expect("compact-model=fixture-summary; compact-effort=high")
                app.close()

    def test_plugin_inheritance_tracks_main_model(self):
        self.config("routing.main-model", "fixture-main")
        self.config("routing.plugin-model", "should-not-be-used")
        app = self.app()
        app.send("29 plugin delegate fixture")
        app.expect("model=fixture-main; effort=medium")

    def test_schedule_edit_pause_delete_and_required_model(self):
        self.cli("organizer", "task", "Report", "--in-seconds", "3600", ok=False)
        self.cli("organizer", "task", "Report", "--in-seconds", "3600", "--harness", "mock", "--model", "mock-light")
        book_path = self.home / "settings" / "schedules.json"
        original = json.loads(book_path.read_text())["tasks"][0]
        task_id = original["id"]
        self.cli("organizer", "edit", task_id, "--prompt", "New instructions", "--in-seconds", "7200",
                 "--harness", "claude", "--model", "sonnet", "--reasoning", "high", "--paused", "true")
        task = json.loads(book_path.read_text())["tasks"][0]
        self.assertEqual((task["harness"], task["model"], task["reasoning"]), ("claude", "sonnet", "high"))
        self.assertTrue(task["paused"])
        self.assertGreater(task["next_unix"], original["next_unix"])
        self.cli("organizer", "edit", task_id, "--harness", "codex", ok=False)
        self.assertEqual(json.loads(book_path.read_text())["tasks"][0], task)
        self.cli("organizer", "cancel", task_id)
        self.assertEqual(json.loads(book_path.read_text())["tasks"], [])

    def test_schedule_runs_in_isolation_and_records_completion(self):
        self.cli("organizer", "task", "scheduled fixture", "--in-seconds", "1", "--harness", "mock", "--model", "mock-light")
        app = self.app("mock")
        app.expect("started in an isolated mock worker", timeout=5)
        app.expect("finished (failed=false)")
        status = self.cli("organizer", "status").stdout
        self.assertIn("completed", status)

    def test_worker_uses_own_model_and_main_stays_light(self):
        app = self.app()
        app.send("29 delegate fixture")
        app.expect("Started worker:")
        app.expect("model=fixture-worker; effort=high")
        app.expect("Returning the local result")
        app.expect("main received worker result")
        app.send("29 which model")
        app.expect("model=gpt-5.6-luna")

    def test_worker_cancel_does_not_lose_main_session(self):
        app = self.app()
        app.send("29 delegate hold fixture")
        app.expect("Started worker:")
        app.expect("worker: ")
        app.send("/stop")
        app.expect("Worker cancelled")
        app.send("29 which model")
        app.expect("model=gpt-5.6-luna")

    def test_plugin_model_is_independent_on_same_harness(self):
        self.config("routing.plugin-use-main", "false")
        self.config("routing.plugin-model", "fixture-plugin")
        app = self.app()
        app.send("29 plugin delegate fixture")
        app.expect("Started worker:")
        app.expect("model=fixture-plugin; effort=medium")
        app.expect("main received worker result")
        app.send("29 which model")
        app.expect("model=gpt-5.6-luna")

    def test_all_harnesses_create_edit_and_delete_schedules(self):
        for harness in ("codex", "claude", "antigravity"):
            with self.subTest(harness=harness):
                app = self.app(harness)
                app.send("29 schedule fixture")
                app.expect("Scheduled task ")
                path = self.home / "settings" / "schedules.json"
                task_id = json.loads(path.read_text())["tasks"][0]["id"]
                app.send("29 edit fixture " + task_id)
                app.expect("Updated task " + task_id)
                task = json.loads(path.read_text())["tasks"][0]
                self.assertEqual(task["reasoning"], "high")
                self.assertTrue(task["paused"])
                app.send("29 delete fixture " + task_id)
                app.expect("Deleted " + task_id)
                self.assertEqual(json.loads(path.read_text())["tasks"], [])
                app.close()

    def test_stdio_harnesses_cancel_and_accept_followups(self):
        for harness in ("claude", "antigravity"):
            with self.subTest(harness=harness):
                app = self.app(harness)
                app.send("29 hello")
                app.expect("stdio reply: hello")
                app.send("follow up")
                app.expect("stdio reply: follow up")
                app.send("29 hold")
                app.expect("holding-stdio")
                app.send("29 replacement")
                app.expect("stdio reply: replacement")
                app.close()

    def test_claude_duplicate_messages_apply_note_once(self):
        app = self.app("claude")
        app.send("29 note fixture")
        app.expect("Note saved:")
        app.expect("Returning the local result")
        time.sleep(0.3)
        self.assertEqual(len(list((self.home / "settings" / "notes").glob("*.md"))), 1)

    def test_usage_failure_blocks_repeat_requests(self):
        app = self.app("claude")
        app.send("29 quota fixture")
        app.expect("usage limit reached")
        app.expect("| idle")
        app.send("try again")
        app.expect("cooling down")
        self.assertNotIn("stdio reply: try again", "".join(app.seen))
        app.send("/limits")
        app.expect("not a verified provider reset")


if __name__ == "__main__":
    unittest.main(verbosity=2)
