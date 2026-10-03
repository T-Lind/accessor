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
        for name, fixture_name in [("opencode", "fake_opencode.py"), ("cursor-agent", "fake_cursor.py")]:
            fixture = ROOT / "tests" / fixture_name
            shim = self.home / (name + (".cmd" if os.name == "nt" else ""))
            if os.name == "nt":
                shim.write_text(f'@echo off\n"{sys.executable}" "{fixture}" %*\n')
            else:
                shim.write_text(f'#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(fixture))} "$@"\n')
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

    @unittest.skipIf(os.name == "nt", "Unix advisory-lock regression")
    def test_memory_review_remains_responsive_when_store_is_busy(self):
        import fcntl
        app = self.app("mock")
        lockpath = self.home / "settings" / "memory.lock"
        with lockpath.open("a+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            app.send("/memory review")
            app.send("/status")
            app.expect("WHITE: waiting for wake code", timeout=1)
            if "Memory is busy" not in "".join(app.seen):
                app.expect("Memory is busy", timeout=1)
            app.send("/quit")
            app.process.wait(timeout=2)
        app.close()

    def test_memory_forget_zero_rejects_without_mutating_first_entry(self):
        self.cli("memory", "save", "fixture", "synthetic fact", "--source", "test")
        app = self.app("mock")
        app.send("/memory forget 0")
        app.expect("Memory numbers start at 1")
        entries = json.loads((self.home / "settings" / "memory.json").read_text())
        self.assertEqual(entries[0]["text"], "synthetic fact")
        self.assertFalse(entries[0]["deleted"])
        app.send("/memory review")
        app.expect("1. [project] fixture")
        app.expect("Forget one with /memory forget N.")
        app.send("/memory forget 1")
        app.expect("Forgot fixture")

    @unittest.skipIf(os.name == "nt", "Unix executable fixture")
    def test_piper_waits_for_completed_file_receipt_and_reuses_worker(self):
        assets = self.home / "assets"
        voices = assets / "piper" / "voices"
        voices.mkdir(parents=True)
        (voices / "en_GB-alan-medium.onnx").touch()
        (voices / "en_GB-alan-medium.onnx.json").write_text("{}")
        binary = assets / "piper" / "bin" / "piper"
        binary.parent.mkdir()
        binary.write_text(f"#!{sys.executable}\n" + '''import pathlib, struct, sys, time
folder = pathlib.Path(sys.argv[sys.argv.index('-d') + 1])
for index, line in enumerate(sys.stdin):
    data = bytes(22050 * 2)
    header = struct.pack('<4sI4s4sIHHIIHH4sI', b'RIFF', 36 + len(data), b'WAVE', b'fmt ', 16, 1, 1, 22050, 44100, 2, 16, b'data', len(data))
    path = folder / f'{index}.wav'
    with path.open('wb') as output:
        output.write(header + data[:4096])
        output.flush()
        time.sleep(0.2)
        output.write(data[4096:])
    print(path, flush=True)
''')
        binary.chmod(0o700)
        self.env["ACC_ASSETS"] = str(assets)
        report = json.loads(self.cli("tts", "benchmark", "--provider", "piper", "--runs", "3", "First line.\nSecond line.").stdout)
        self.assertEqual(len(report["runs"]), 3)
        for row in report["runs"]:
            self.assertAlmostEqual(row["audio_seconds"], 1.0)
            self.assertGreaterEqual(row["total_ms"], 190)

    def app(self, harness="codex"):
        self.config("routing.main", harness)
        app = App("--codex-bin", str(self.codex), env=self.env)
        self.addCleanup(app.close)
        return app

    def music_fixture(self):
        log = self.home / "music.jsonl"
        self.env["ACC_MUSIC_TEST_LOG"] = str(log)
        for name, mode in [("mpv", "mpv"), ("soloist", "soloist")]:
            shim = self.home / name
            shim.write_text(f'#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(ROOT / "tests/fake_music.py"))} {mode} "$@"\n')
            shim.chmod(0o700)
            self.env["ACC_MPV_BIN" if mode == "mpv" else "ACC_SOLOIST_BIN"] = str(shim)
        return log

    @unittest.skipIf(os.name == "nt", "Unix local music IPC")
    def test_music_live_controls_keep_paths_as_data_and_cancel_player(self):
        log = self.music_fixture()
        app = self.app("mock")
        endpoint = next((self.home / "settings" / "sessions").glob("*.json"))
        source = self.home / "song $ ; ' with spaces.mp3"
        source.write_bytes(b"synthetic fixture")
        def music(*args, ok=True):
            return self.cli("music", "--session", str(endpoint), *args, ok=ok)
        result = json.loads(music("play", str(source), "--volume", "17", "--repeat").stdout)
        self.assertEqual(result["state"], "playing")
        self.assertEqual(result["volume"], 17)
        self.assertTrue(result["repeat"])
        rows = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertIn(["loadfile", str(source), "replace"], [row.get("command") for row in rows])
        pid = rows[0]["pid"]
        music("pause")
        self.assertTrue(json.loads(music("status").stdout)["paused"])
        music("resume")
        self.assertFalse(json.loads(music("status").stdout)["paused"])
        music("volume", "9")
        self.assertEqual(json.loads(music("status").stdout)["volume"], 9)
        self.assertIn("No such file", music("play", str(self.home / "missing.mp3"), ok=False).stderr)
        app.send("/cancel")
        app.send("/status")
        app.expect("WHITE:")
        self.assertEqual(json.loads(music("status").stdout)["state"], "stopped")
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                break
            time.sleep(0.01)
        else:
            self.fail("Cancelled music player remained alive")
        app.send("/music ambience brown")
        app.expect('"state":"playing"')
        app.send("/music stop")
        app.expect('"state":"stopped"')

    @unittest.skipIf(os.name == "nt", "Unix fixture executable")
    def test_music_failed_load_and_unpaired_spotify_are_visible(self):
        self.music_fixture()
        self.env["ACC_MUSIC_TEST_FAIL"] = "1"
        app = self.app("mock")
        app.send("/music ambience rain")
        app.expect("loading failed")
        app.send("/music status")
        app.expect('"state":"stopped"')
        app.send("/music spotify play spotify:track:2fFlmlePz9hrVMv4LvdQxN")
        app.expect("not logged in")
        endpoint = next((self.home / "settings" / "sessions").glob("*.json"))
        attached = dict(self.env, ACC_CONTROL_ENDPOINT=endpoint.read_text())
        frame = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "music_control", "arguments": {"backend": "spotify", "action": "play", "source": "spotify:track:2fFlmlePz9hrVMv4LvdQxN"}}}
        initialize = {"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}}
        initialized = {"jsonrpc": "2.0", "method": "notifications/initialized"}
        result = subprocess.run([str(BINARY), "mcp"], input="".join(json.dumps(item)+"\n" for item in [initialize, initialized, frame]), env=attached, cwd=ROOT, capture_output=True, text=True, timeout=10)
        reply = json.loads(result.stdout.splitlines()[-1])["result"]
        self.assertTrue(reply["isError"])
        self.assertIn("not logged in", reply["content"][0]["text"])
        app.send("/status")
        app.expect("WHITE:")

    @unittest.skipIf(os.name == "nt", "Unix fixture executable")
    def test_music_spotify_passes_only_validated_commands(self):
        log = self.music_fixture()
        app = self.app("mock")
        app.send("/music spotify play spotify:playlist:2fFlmlePz9hrVMv4LvdQxN")
        app.expect("Soloist accepted the command")
        app.send("/music spotify volume 22")
        app.expect("Soloist accepted the command")
        app.send("/music spotify play --api-key")
        app.expect("Use a Spotify track")
        rows = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertEqual([row["args"] for row in rows], [["ctl", "play", "spotify:playlist:2fFlmlePz9hrVMv4LvdQxN"], ["ctl", "volume", "22"]])

    @unittest.skipIf(os.name == "nt", "Unix local music IPC")
    def test_lock_stops_owned_music_and_blocks_new_playback(self):
        log = self.music_fixture()
        app = self.app("mock")
        app.send("/music ambience white")
        app.expect('"state":"playing"')
        pid = json.loads(log.read_text().splitlines()[0])["pid"]
        app.send("/password")
        app.expect("Enter at least three words")
        app.send("marble otter meadow lantern")
        app.expect("Repeat the passphrase")
        app.send("marble otter meadow lantern")
        app.expect("Locked. Work and playback stopped")
        app.send("/music ambience brown")
        app.expect("Locked. Use /unlock")
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                break
            time.sleep(0.01)
        else:
            self.fail("Locked session left music playing")

    @unittest.skipIf(os.name == "nt", "Unix executable fixture")
    def test_cancel_kills_piper_synthesis_before_next_request(self):
        assets = self.home / "assets"
        voices = assets / "piper" / "voices"
        voices.mkdir(parents=True)
        (voices / "en_GB-alan-medium.onnx").touch()
        (voices / "en_GB-alan-medium.onnx.json").write_text("{}")
        binary = assets / "piper" / "bin" / "piper"
        binary.parent.mkdir()
        log = self.home / "piper-requests.jsonl"
        binary.write_text(f"#!{sys.executable}\n" + f'''import json, os, pathlib, sys, time
for line in sys.stdin:
    with pathlib.Path({str(log)!r}).open('a') as output:
        output.write(json.dumps([os.getpid(), line.strip()]) + '\\n')
    time.sleep(10)
''')
        binary.chmod(0o700)
        self.env["ACC_ASSETS"] = str(assets)
        self.config("tts.provider", "piper")
        app = App("--agent", "mock", "--speak", env=self.env)
        self.addCleanup(app.close)
        pids = []
        for index, text in enumerate(["First request", "Second request"]):
            app.send("/tts test " + text)
            app.expect("Testing piper voice")
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                rows = log.read_text().splitlines() if log.exists() else []
                if len(rows) > index:
                    break
                time.sleep(0.01)
            self.assertEqual(len(rows), index + 1)
            pid, received = json.loads(rows[-1])
            self.assertEqual(received, text)
            pids.append(pid)
            app.send("/cancel")
            app.send("/status")
            app.expect("WHITE:")
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.01)
            else:
                self.fail("Cancelled Piper worker remained alive")
        self.assertNotEqual(*pids)

    @unittest.skipIf(os.name == "nt", "Unix clipboard shim")
    def test_copy_preserves_answer_as_data_and_jobs_stays_local(self):
        captured = self.home / "clipboard.txt"
        self.env["ACC_CLIPBOARD_FILE"] = str(captured)
        shim = self.home / "wl-copy"
        shim.write_text(f"#!{sys.executable}\nimport os, sys\nfrom pathlib import Path\nPath(os.environ['ACC_CLIPBOARD_FILE']).write_text(sys.stdin.read())\n")
        shim.chmod(0o700)
        app = self.app()
        app.send("/copy")
        app.expect("No answer to copy yet")
        payload = "quotes ' dollars $ and backticks ` as plain text"
        app.send("29 " + payload)
        app.expect("fixture: " + payload)
        app.send("/copy")
        app.expect("Copied the last answer to the clipboard")
        self.assertEqual(captured.read_text(), "fixture: " + payload)
        app.send("/repeat")
        app.expect("Speech is off")
        app.send("/mic reconnect")
        app.expect("Microphone is off in text mode")
        app.send("/jobs")
        app.expect("Waiting: 0 scheduled task(s)")
        app.close()
        analytics = json.loads((self.home / "settings" / "analytics.json").read_text())
        self.assertEqual(analytics["lifetime"]["harness_turns"], 1)

    @unittest.skipIf(os.name == "nt", "Unix advisory-lock regression")
    def test_busy_organizer_lock_does_not_stall_commands_or_lose_due_task(self):
        import fcntl
        self.cli("organizer", "task", "scheduled fixture", "--in-seconds", "1", "--harness", "mock", "--model", "mock-light")
        lockpath = self.home / "settings" / "organizer.lock"
        with lockpath.open("a+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            app = self.app("mock")
            time.sleep(1.3)
            app.send("/jobs")
            app.expect("Waiting: 0 scheduled task(s)", timeout=3)
            app.send("/status")
            app.expect("WHITE: waiting for wake code", timeout=3)
            fcntl.flock(lock, fcntl.LOCK_UN)
            app.expect("started in an isolated mock worker", timeout=5)
            app.expect("finished (failed=false)")
        self.assertIn("completed", self.cli("organizer", "status").stdout)

    def test_logs_flush_before_tail_and_shutdown(self):
        self.config("logging.enabled", "true")
        app = self.app()
        app.send("29 synthetic logging marker")
        app.expect("fixture: synthetic logging marker")
        app.send("/logs")
        app.expect("Master log:")
        app.expect("synthetic logging marker")
        app.close()
        log = (self.home / "settings" / "logs" / "accessor.log").read_text()
        self.assertIn("fixture: synthetic logging marker", log)

    def test_continuation_before_harness_ready_keeps_original_request(self):
        self.env["ACC_FIXTURE_START_DELAY"] = "0.7"
        app = self.app()
        app.send("29 first part of the request")
        app.expect("You: first part")
        app.send("and the second part")
        app.expect("Interrupting;")
        app.expect("fixture: first part of the request")
        app.expect("Additional user speech: and the second part")

    def test_subscription_quota_queries_all_harnesses_without_model_turns(self):
        self.config("codex-bin",str(self.codex))
        result=self.cli("usage","--json")
        rows=json.loads(result.stdout)["providers"]
        self.assertEqual([p["harness"] for p in rows],["codex","claude","antigravity"])
        self.assertTrue(all(p["available"] and not p["stale"] for p in rows),rows)
        self.assertEqual(rows[0]["buckets"][0]["remaining_percent"],73)
        self.assertEqual(rows[1]["buckets"][0]["remaining_percent"],58)
        self.assertEqual(rows[2]["buckets"][0]["remaining_percent"],81)
        app=self.app()
        app.send("/usage")
        app.expect("SUBSCRIPTION USAGE")
        app.expect("73% left")
        app.expect("Resets:")

    def test_continuation_while_thinking_needs_no_wake_code(self):
        app = self.app()
        app.send("29 hold")
        app.expect("▸ hold")
        app.send("and include Chicago please")
        app.expect("Interrupting;")
        app.expect("fixture: and include Chicago please")
        # The interrupted turn's partial output is preserved as context.
        self.assertIn("Interrupted; preserved", "".join(app.seen))

    def test_streaming_does_not_select_cloud_implicitly(self):
        self.config("stt.conversation", "local")
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

    def test_config_export_import_keeps_device_settings(self):
        self.config("microphone", "Local Test Mic")
        self.config("stt.threads", "3")
        self.config("wake-code", "42")
        export = self.home / "portable.json"
        self.cli("config", "export", str(export))
        exported = json.loads(export.read_text(encoding="utf-8"))
        self.assertNotIn("microphone", exported)
        self.assertNotIn("threads", exported["stt"])
        self.assertEqual(exported["wake_code"], "42")
        self.config("wake-code", "99")
        self.config("stt.threads", "5")
        self.cli("config", "import", str(export))
        device = json.loads((self.home / "settings" / "device.json").read_text(encoding="utf-8"))
        self.assertEqual(device["microphone"], "Local Test Mic")
        self.assertEqual(device["stt"]["threads"], 5)  # Device-local tuning is kept.
        settings = json.loads((self.home / "settings" / "config.json").read_text(encoding="utf-8"))
        self.assertEqual(settings["wake_code"], "42")
        self.assertNotIn("microphone", settings)
        self.assertNotIn("threads", settings["stt"])

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

        def control(action, tool="session_control", arguments=None):
            frames = [
                {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}},
                {"jsonrpc": "2.0", "method": "notifications/initialized"},
                {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": arguments if arguments is not None else {"action": action}}},
            ]
            result = subprocess.run([str(BINARY), "mcp"], input="".join(json.dumps(f)+"\n" for f in frames),
                                    cwd=ROOT, env=attached, capture_output=True, text=True, encoding="utf-8", timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            reply = json.loads(result.stdout.splitlines()[-1])["result"]
            self.assertFalse(reply["isError"], reply)
            return json.loads(reply["content"][0]["text"])

        self.assertTrue(control("status")["awake"])
        self.assertEqual(control(None,"music_control",{"action":"status"})["state"],"stopped")
        changed=control(None,"settings_update",{"changes":{"tts.speed":1.2,"tts.volume":0.65,"routing.reasoning":"high","sounds.think":0}})
        self.assertIn("next turn",changed["receipt"])
        live_settings=control(None,"settings_read",{})["values"]
        self.assertEqual(live_settings["routing.reasoning"],"high")
        self.assertAlmostEqual(live_settings["tts.volume"],0.65)
        self.assertTrue(control("status")["busy"])  # Updating settings never aborts its caller.
        self.assertFalse(control("sleep")["awake"])
        app.expect("MCP: asleep")
        self.assertFalse(control("status")["awake"])
        self.assertFalse(control("stop_alarm")["stopped"])
        listed=control(None,"organizer_control",{"directive":{"action":"list_schedules"}})
        self.assertIn("status",listed)
        delegated=control(None,"delegate_task",{"prompt":"hold fixture","harness":"mock","model":"mock-worker","reasoning":"low"})
        self.assertIn("Started",delegated["receipt"])
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
        app.expect("Agent: fixture: hello after interruption")

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

    def test_connector_management_uses_antigravity_when_configured(self):
        self.config("routing.plugin-use-main", "false")
        self.config("agent", "antigravity")
        self.assertIn("fixture-plugin (enabled)", self.cli("connectors", "list").stdout)
        status = self.cli("connectors", "status").stdout
        self.assertIn("Plugins reported by antigravity", status)
        self.assertIn("fixture-plugin (enabled)", status)
        self.assertIn("fixture-connector enabled", status)

    def test_memory_inference_is_responsive_cancellable_and_honors_cli_binary(self):
        self.env["ACC_FIXTURE_START_DELAY"] = "2"
        configured = self.home / ("configured-codex.cmd" if os.name == "nt" else "configured-codex")
        configured.write_text("@echo off\nexit /b 99\n" if os.name == "nt" else "#!/bin/sh\nexit 99\n")
        if os.name != "nt":
            configured.chmod(0o700)
        self.config("codex-bin", str(configured))
        self.config("memory.capture", "false")
        app = self.app()
        app.send("/memory infer")
        app.expect("Extracting durable facts")
        app.send("/jobs")
        app.expect("Background helper: running", timeout=1)
        app.send("/status")
        app.expect("WHITE: waiting for wake code", timeout=1)
        app.expect("Extracted 0 candidate(s); nothing new to save.", timeout=5)
        app.send("/memory infer")
        app.expect("Extracting durable facts")
        app.send("/cancel")
        app.send("/jobs")
        app.expect("Main: idle")
        before = len(app.seen)
        app.expect("/cancel stops active work")
        self.assertNotIn("Background helper: running", "".join(app.seen[before:]))
        app.close()

    @unittest.skipIf(os.name == "nt", "Unix executable marker")
    def test_mock_does_not_launch_compactor_or_memory_model(self):
        marker = self.home / "unexpected-model-call"
        self.config("codex-bin", str(self.codex))
        self.codex.write_text(f"#!{sys.executable}\nfrom pathlib import Path\nPath({str(marker)!r}).write_text('called')\nraise SystemExit(97)\n")
        self.codex.chmod(0o700)
        app = self.app("mock")
        app.send("29 remember that I prefer concise replies")
        app.expect("Mock agent received:")
        app.send("/compact")
        app.expect("Compacted Accessor-owned history")
        app.send("/memory infer")
        app.expect("model inference is off")
        app.send("/jobs")
        app.expect("Waiting: 0 scheduled task(s)")
        app.close()
        self.assertFalse(marker.exists(), "mock launched a model process")

    def test_followup_interrupts_control_feedback_without_losing_request(self):
        self.env["ACC_FIXTURE_FEEDBACK_DELAY"] = "0.4"
        for harness in ("codex", "claude", "antigravity"):
            with self.subTest(harness=harness):
                app = self.app(harness)
                app.send("29 schedule fixture")
                app.expect("Scheduled task ")
                path = self.home / "settings" / "schedules.json"
                task_id = json.loads(path.read_text())["tasks"][0]["id"]
                app.expect("Returning the local result")
                app.send("29 edit fixture " + task_id)
                app.expect("Updated task " + task_id)
                task = json.loads(path.read_text())["tasks"][0]
                self.assertTrue(task["paused"])
                self.assertEqual(task["reasoning"], "high")
                app.close()
                self.cli("organizer", "cancel", task_id)

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

    def test_antigravity_read_only_does_not_skip_permissions_writable_does(self):
        # The fixture asserts the bypass is absent in plan mode and present in
        # accept-edits mode, so a failure surfaces as a missing reply here.
        read_only = self.app("antigravity")
        read_only.send("29 hello")
        read_only.expect("stdio reply: hello")
        read_only.close()
        self.config("routing.main", "antigravity")
        writable = App("--codex-bin", str(self.codex), "--workspace-write", env=self.env)
        self.addCleanup(writable.close)
        writable.send("29 hello writable")
        writable.expect("stdio reply: hello writable")

    def test_claude_permission_request_is_surfaced_and_approved(self):
        app = self.app("claude")
        app.send("29 permission fixture")
        app.expect("Approval ")
        app.send("/approve 1")
        app.expect("approval=allow")
        app.close()

    def test_claude_permission_request_can_be_denied(self):
        app = self.app("claude")
        app.send("29 permission fixture")
        app.expect("Approval ")
        app.send("/deny 1")
        app.expect("approval=deny")
        app.close()

    def test_opencode_and_cursor_run_headless_and_follow_up(self):
        for harness, reply in [
            ("opencode", "opencode reply: hello"),
            ("cursor", "cursor reply: hello"),
        ]:
            with self.subTest(harness=harness):
                app = self.app(harness)
                app.send("29 hello")
                app.expect("You: hello")
                app.expect(reply)
                app.send("follow up")
                app.expect(reply.replace("hello", "follow up"))
                app.close()

    def test_opencode_and_cursor_are_accepted_roles(self):
        for harness in ("opencode", "cursor"):
            with self.subTest(harness=harness):
                self.config("routing.coding", harness)
                settings = json.loads((self.home / "settings" / "config.json").read_text(encoding="utf-8"))
                self.assertEqual(settings["routing"]["coding"], harness)

    def test_connector_autodetection_reads_harness_config(self):
        opencode_config = self.home / "opencode.json"
        opencode_config.write_text(
            json.dumps({"mcp": {"github": {"type": "local"}}, "plugin": ["superpowers"]})
        )
        cursor_dir = self.home / "cursor-config"
        cursor_dir.mkdir()
        (cursor_dir / "mcp.json").write_text(json.dumps({"mcpServers": {"linear": {"command": "x"}}}))

        self.config("routing.plugin-use-main", "false")
        self.config("agent", "opencode")
        env = dict(self.env, OPENCODE_CONFIG=str(opencode_config))
        opencode = subprocess.run(
            [str(BINARY), "connectors", "status"], env=env, cwd=ROOT,
            capture_output=True, text=True, encoding="utf-8", timeout=20,
        )
        self.assertIn("github", opencode.stdout)
        self.assertIn("superpowers", opencode.stdout)

        self.config("agent", "cursor")
        env = dict(self.env, CURSOR_CONFIG_DIR=str(cursor_dir))
        cursor = subprocess.run(
            [str(BINARY), "connectors", "status"], env=env, cwd=ROOT,
            capture_output=True, text=True, encoding="utf-8", timeout=20,
        )
        self.assertIn("linear", cursor.stdout)

    def test_claude_duplicate_messages_apply_note_once(self):
        app = self.app("claude")
        app.send("29 note fixture")
        app.expect("Note saved:")
        app.expect("Returning the local result")
        time.sleep(0.3)
        self.assertEqual(len(list((self.home / "settings" / "notes").glob("*.md"))), 1)

    def test_watch_slash_lifecycle(self):
        app = self.app("mock")
        app.send("/watch every 5m threshold=0.7 night=22:00-07:00 urgent release email")
        app.expect("created: every 5m, threshold 0.70")
        book_path = self.home / "settings" / "schedules.json"
        watch = json.loads(book_path.read_text())["tasks"][0]
        watch_id = watch["id"]
        self.assertEqual(watch["every_seconds"], 300)
        self.assertEqual(watch["watch"]["guidelines"], "urgent release email")
        self.assertEqual(watch["watch"]["threshold"], 0.7)
        self.assertEqual(watch["quiet"], {"start": "22:00", "end": "07:00"})
        app.send("/watch list")
        app.expect("threshold 0.70 · speak true · quiet 22:00–07:00")
        app.send("/watch edit " + watch_id + " threshold=0.9 quiet")
        app.expect("Watch " + watch_id + " updated")
        self.assertEqual(json.loads(book_path.read_text())["tasks"][0]["watch"]["threshold"], 0.9)
        app.send("/watch stop " + watch_id)
        app.expect("Stopped watch " + watch_id)
        self.assertEqual(json.loads(book_path.read_text())["tasks"], [])

    def test_agent_notification_directive_is_stored_and_deduped(self):
        app = self.app("codex")
        path = self.home / "settings" / "notifications.json"
        app.send("29 notify fixture")
        app.expect("saved.")
        items = json.loads(path.read_text())["items"]
        self.assertEqual(len(items), 1)
        self.assertTrue(items[0]["unread"])
        self.assertEqual(items[0]["source"], "agent")
        # An exact repeat from the same source is suppressed, not appended.
        app.send("29 notify fixture")
        app.expect("suppressed")
        self.assertEqual(len(json.loads(path.read_text())["items"]), 1)
        # A distinct story is kept.
        app.send("29 notify distinct")
        app.expect("saved.")
        titles = {item["title"] for item in json.loads(path.read_text())["items"]}
        self.assertEqual(titles, {"Fixture notice", "Server alert"})

    def test_quiet_hours_deferral_records_a_visible_receipt(self):
        app = self.app("codex")
        app.send("29 quiet fixture")
        app.expect("Scheduled task ")
        deadline = time.time() + 20
        status = ""
        while time.time() < deadline:
            status = self.cli("organizer", "status").stdout
            if "quiet hours" in status:
                break
            time.sleep(0.5)
        self.assertIn("deferred to", status)
        self.assertIn("(quiet hours", status)
        self.assertIn("Run ", status)

    def test_conversation_captures_explicit_memory_off_the_hot_path(self):
        app = self.app("mock")
        app.send("29 remember that the workshop filter size is 20 by 25")
        app.expect("Mock agent received: remember that the workshop filter size is 20 by 25")
        path = self.home / "settings" / "memory.json"
        entries = []
        deadline = time.time() + 10
        while time.time() < deadline:
            if path.exists():
                entries = json.loads(path.read_text())
                if entries:
                    break
            time.sleep(0.2)
        self.assertTrue(entries, "expected an explicit memory to be captured")
        captured = next(entry for entry in entries if entry["kind"] == "explicit")
        self.assertEqual(captured["scope"], "global")
        self.assertIn("workshop filter size is 20 by 25", captured["text"])
        self.assertIn("user said:", captured["source"])

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
