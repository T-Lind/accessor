"""End-to-end CLI/protocol tests. No microphone, credentials, or network."""
import os
from pathlib import Path
import queue
import shlex
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parent.parent
BINARY = ROOT / "target" / "release" / ("acc.exe" if os.name == "nt" else "acc")

class App:
    def __init__(self, *extra):
        self.process = subprocess.Popen([str(BINARY), "run", "--wake-code", "29", "--text", *extra], cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8")
        self.lines = queue.Queue()
        self.seen = []
        def read():
            for line in self.process.stdout:
                self.lines.put(line)
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        self.expect("WHITE:")

    def send(self, line):
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()

    def expect(self, text, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                line = self.lines.get(timeout=max(0.01, deadline - time.monotonic()))
            except queue.Empty:
                break
            self.seen.append(line)
            if text in line:
                return line
        raise AssertionError(f"Missing {text!r}: {''.join(self.seen)}")

    def close(self):
        if self.process.poll() is None:
            self.send("/quit")
            try:
                self.process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.process.kill()
                raise
        self.reader.join(timeout=2)
        self.process.stdin.close()
        self.process.stdout.close()
        assert self.process.returncode == 0, self.process.returncode

class SmokeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix="accessor-test-")
        cls.old_home = os.environ.get("ACC_HOME")
        os.environ["ACC_HOME"] = str(Path(cls.temp.name) / "settings")
        cls.shim = Path(cls.temp.name) / ("fake.cmd" if os.name == "nt" else "fake")
        fixture = ROOT / "tests" / "fake_codex.py"
        if os.name == "nt":
            cls.shim.write_text(f'@echo off\n"{sys.executable}" "{fixture}"\n')
        else:
            cls.shim.write_text(f"#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(fixture))}\n")
            cls.shim.chmod(0o700)

    @classmethod
    def tearDownClass(cls):
        if cls.old_home is None:
            os.environ.pop("ACC_HOME", None)
        else:
            os.environ["ACC_HOME"] = cls.old_home
        cls.temp.cleanup()

    def app(self, *args):
        app = App(*args)
        self.addCleanup(app.close)
        return app

    def test_barge_in_replaces_busy_turn(self):
        app=self.app("--agent","codex","--codex-bin",str(self.shim))
        app.send("29 hold")
        app.expect("▸ hold")
        app.send("29 actually do this instead")
        app.expect("Interrupting;")
        app.expect("fixture: actually do this instead")
        app.expect("| idle")
        self.assertNotIn("Agent: interrupted","".join(app.seen))
        app.send("29 a later request")
        app.expect("fixture: a later request")

    def test_barge_in_can_be_disabled(self):
        app=self.app("--agent","codex","--codex-bin",str(self.shim))
        app.send("/settings barge-in false")
        app.expect("Saved barge-in: false")
        app.send("29 hold")
        app.expect("▸ hold")
        app.send("29 followup must wait")
        app.expect("Barge-ins are off")
        app.send("/cancel")
        app.expect("interrupted")
        app.send("/settings barge-in true")
        app.expect("Saved barge-in: true")

    def test_progress_is_queued_for_speech_while_working(self):
        # Off provider exercises the speech queue without using a sound device.
        app=self.app("--agent","codex","--codex-bin",str(self.shim),"--speak","--tts","off")
        app.send("29 hold")
        app.expect("▸ hold")
        app.expect("SPEAKING:")
        self.assertFalse(any("Agent: " in line for line in app.seen))
        app.send("/cancel")
        app.expect("interrupted")

    def test_progress_speech_can_be_disabled(self):
        app=self.app("--agent","codex","--codex-bin",str(self.shim),"--speak","--tts","off")
        app.send("/settings speak-progress false")
        app.expect("Saved speak-progress: false")
        app.send("29 hold")
        app.expect("▸ hold")
        time.sleep(.2)
        app.send("/status")
        app.expect("WHITE: waiting for wake code | agent working")
        self.assertNotIn("SPEAKING:", "".join(app.seen))
        app.send("/cancel")
        app.expect("interrupted")
        app.expect("| idle")
        app.send("/settings speak-progress true")
        app.expect("Saved speak-progress: true")

    def test_spoken_agent_switch_interrupts_existing_work(self):
        app=self.app("--agent","codex","--codex-bin",str(self.shim))
        app.send("29 hold")
        app.expect("▸ hold")
        app.send("29 switch agent to mock")
        app.expect("Interrupting current work")
        app.expect("Saved agent: mock")
        app.send("29 hello after switching")
        app.expect("hello after switching")
        app.expect("| idle")
        app.send("/settings agent codex")
        app.expect("Saved agent: codex")

    def test_settings_model_selection_and_spoken_switch(self):
        app=self.app("--agent","codex","--codex-bin",str(self.shim))
        app.send("/settings")
        app.expect("SETTINGS")
        app.expect("Loaded 2 available Codex models")
        app.send("3")
        app.expect("HARNESSES")
        app.send("4")
        app.expect("Fixture Astra")
        app.send("1")
        app.expect("Saved model: fixture-astra")
        app.send("0")
        app.send("0")
        app.expect("Settings closed")
        app.send("29 which Rust model")
        app.expect("model=fixture-astra")
        app.expect("| idle")
        app.send("29 switch model to Sol")
        app.expect("Saved model: fixture-sol")
        app.send("29 which Rust model")
        app.expect("model=fixture-sol")
        app.expect("| idle")
        app.send("29 switch model to imaginary")
        app.expect("not in the available list")
        app.send("/settings model default")
        app.expect("Saved model: default")

    def test_wake_policy_is_mandatory(self):
        app=self.app("--agent","mock")
        app.send("29 hello")
        app.expect("Mock agent received: hello")
        app.expect("| idle")
        app.send("private ignored followup")
        app.send("29 followup")
        app.expect("Mock agent received: followup")
        self.assertNotIn("private ignored", "".join(app.seen))
        app.send("/settings addressed false")
        app.expect("Unknown setting")

    def test_privacy_activation_followup_disconnect(self):
        app = self.app("--agent", "mock")
        app.send("private ambient phrase")
        app.send("129 false prefix")
        app.send("twenty nine, hello")
        app.expect("Mock agent received: hello")
        app.expect("| idle")
        self.assertNotIn("private ambient", "".join(app.seen))
        self.assertNotIn("false prefix", "".join(app.seen))
        app.send("29 follow up")
        app.expect("Mock agent received: follow up")
        app.expect("| idle")
        app.send("29 disconnect")
        app.expect("Asleep. Waiting for wake code.")
        app.send("more private ambient")
        app.send("/status")
        app.expect("WHITE:")
        self.assertNotIn("more private", "".join(app.seen))

    def test_mandatory_wake_and_mute(self):
        app = self.app("--agent", "mock")
        app.send("29")
        app.expect("GREEN:")
        app.send("hello after open")
        app.expect("Mock agent received: hello after open")
        app.send("29 hello")
        app.expect("Mock agent received: hello")
        app.expect("| idle")
        app.send("/mute")
        app.expect("MUTED")
        app.send("29 must be ignored")
        app.send("/unmute")
        app.expect("WHITE:")
        app.send("29 works")
        app.expect("Mock agent received: works")
        self.assertNotIn("must be ignored", "".join(app.seen))

    def test_session_expires(self):
        app = self.app("--agent", "mock", "--idle-seconds", "1")
        app.send("29")
        app.expect("GREEN:")
        app.expect("WHITE:", timeout=3)
        app.send("expired request")
        app.send("29 fresh request")
        app.expect("Mock agent received: fresh request")
        self.assertNotIn("expired request", "".join(app.seen))

    def test_piped_input_waits_for_result(self):
        result = subprocess.run([str(BINARY), "run", "--wake-code", "29", "--text", "--agent", "mock"], input="29 piped request\n", text=True, capture_output=True, timeout=10, cwd=ROOT)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Mock agent received: piped request", result.stdout)

    def test_busy_time_does_not_expire_conversation(self):
        app = self.app("--agent", "codex", "--codex-bin", str(self.shim), "--idle-seconds", "1")
        app.send("29 hold")
        app.expect("▸ hold")
        time.sleep(1.4)
        app.send("/status")
        app.expect("WHITE: waiting for wake code | agent working")
        app.send("/cancel")
        app.expect("interrupted")
        app.expect("| idle")
        app.send("29 follow up after long work")
        app.expect("fixture: follow up after long work")

    def test_stop_cancels_and_sleeps(self):
        app = self.app("--agent", "codex", "--codex-bin", str(self.shim))
        app.send("29 hold")
        app.expect("▸ hold")
        app.send("29 stop")
        app.expect("Stopped. Waiting for wake code.")
        app.expect("interrupted")
        app.expect("| idle")
        app.send("ambient after stop")
        app.send("29 new request")
        app.expect("fixture: new request")
        self.assertNotIn("ambient after stop", "".join(app.seen))

    def test_analytics_context_and_compact(self):
        app = self.app("--agent", "codex", "--codex-bin", str(self.shim))
        app.send("/analytics")
        app.expect("Lifetime")
        app.expect("Past 7 days")
        app.expect("Jev")
        app.send("/context")
        app.expect("Context")
        app.send("29 hello analytics")
        app.expect("fixture: hello analytics")
        app.send("/compact")
        app.expect("Compacted")
        app.send("/analytics")
        app.expect("Harness")

    def test_short_command_and_saved_settings(self):
        for key, value in [("wake-code", "42"), ("idle-seconds", "0")]:
            result = subprocess.run([str(BINARY), "config", "set", key, value], cwd=ROOT, capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
        try:
            result = subprocess.run([str(BINARY), "--text", "--agent", "mock"], input="42 saved config works\n", text=True, capture_output=True, timeout=10, cwd=ROOT)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Mock agent received: saved config works", result.stdout)
        finally:
            subprocess.run([str(BINARY), "config", "set", "wake-code", "29"], check=True)
            subprocess.run([str(BINARY), "config", "set", "idle-seconds", "120"], check=True)

    def test_event_wakes_agent_without_opening_voice_and_deduplicates(self):
        subprocess.run([str(BINARY), "config", "set", "event-owner", "owner@example.test"], check=True)
        app = self.app("--agent", "mock", "--events")
        command = [str(BINARY), "events", "emit", "--thread-id", "testthread", "--message-id", "testmessage"]
        for _ in range(2):
            subprocess.run(command, check=True, capture_output=True)
        app.expect("Gmail notification received")
        app.expect("Mock agent received:")
        app.expect("WHITE: waiting for wake code | idle")
        time.sleep(1.3)
        app.send("/status")
        app.expect("WHITE:")
        self.assertEqual(sum("Mock agent received:" in line for line in app.seen), 1)
        receipts = list((Path(os.environ["ACC_HOME"]) / "events" / "receipts").glob("*.json"))
        self.assertEqual(len(receipts), 1)

    def test_dashboard_setup_and_transcription_controls(self):
        app = self.app("--agent", "mock")
        app.send("/setup")
        app.expect("Setup 1/4")
        app.send("42")
        app.expect("Setup 2/4")
        app.send("9000")
        app.expect("0–3600")
        app.send("0")
        app.expect("Setup 3/4")
        app.send("system")
        app.expect("Setup 4/4")
        app.send("no")
        app.expect("Setup saved")
        app.send("/tts")
        app.expect("/tts provider")
        app.send("/stt test")
        app.expect("test ON")
        app.send("transcription without wake code")
        app.expect("Transcript: transcription without wake code")
        app.send("/stt off")
        app.expect("test OFF")
        app.send("42 setup works")
        app.expect("Mock agent received: setup works")
        app.expect("| idle")
        for key, value in [("wake-code", "29"), ("idle-seconds", "120"), ("speak", "true")]:
            subprocess.run([str(BINARY), "config", "set", key, value], check=True)

    def test_codex_protocol_approval_is_specific(self):
        app = self.app("--agent", "codex", "--codex-bin", str(self.shim))
        app.send("29 approval test")
        app.expect("Type /approve 1")
        app.send("/approve 999")
        app.expect("absent, expired")
        app.send("/deny 1")
        app.expect("decision=decline")
        app.expect("| idle")
        app.send("29 approval test")
        app.expect("Type /approve 2")
        app.send("/approve 2")
        app.expect("decision=accept")

    def test_unknown_permissions_denied_and_cancel_live(self):
        app = self.app("--agent", "codex", "--codex-bin", str(self.shim))
        app.send("29 grant test")
        app.expect("grant denied")
        app.expect("| idle")
        app.send("29 hold")
        app.expect("BLUE:")
        app.send("/cancel")
        app.expect("interrupted")

    def test_hey_wake_and_go_back_to_sleep(self):
        app = self.app("--agent", "mock")
        app.send("Hey 29, hello there")
        app.expect("Mock agent received: hello there")
        app.expect("| idle")
        app.send("29 please go back to sleep")
        app.expect("Asleep. Waiting for wake code.")
        app.send("/status")
        app.expect("WHITE:")
        app.send("ignored after sleep")
        app.send("ok 29 followup")
        app.expect("Mock agent received: followup")
        self.assertNotIn("ignored after sleep", "".join(app.seen))

    def test_chat_shows_tools_and_hides_commentary(self):
        app = self.app("--agent", "codex", "--codex-bin", str(self.shim))
        app.send("29 tool test")
        app.expect("▸ gmail/search")
        app.expect("✓ gmail/search")
        app.expect("tool done")
        app.send("29 hold")
        app.expect("▸ hold")
        self.assertNotIn("Holding task", "".join(app.seen))
        app.send("/cancel")
        app.expect("interrupted")
        app.send("/settings chat transcript")
        app.expect("Saved chat: transcript")
        app.send("29 hold")
        app.expect("Holding task")
        app.send("/cancel")
        app.expect("interrupted")

if __name__ == "__main__":
    unittest.main(verbosity=2)
