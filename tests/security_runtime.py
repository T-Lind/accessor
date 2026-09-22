"""Local lock regression tests. Synthetic passphrase, mock agent, no mic/network."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import unittest

from smoke import App, BINARY, ROOT

PHRASE = "marble otter meadow lantern"


class SecurityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="accessor-security-")
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.env = dict(os.environ, ACC_HOME=str(self.home),
                        ACC_MCP_REGISTRY=str(self.home / "registry.json"))
        self.apps = []
        self.addCleanup(lambda: [app.close() for app in self.apps])

    def app(self):
        app = App("--agent", "mock", env=self.env, ready="LOCKED" if (self.home / "password.json").exists() else "WHITE:")
        self.apps.append(app)
        return app

    def enroll(self, app):
        app.send("/password")
        app.expect("Enter at least three words")
        app.send(PHRASE)
        app.expect("Repeat the passphrase")
        app.send(PHRASE)
        app.expect("Locked. Work and playback stopped")
        saved = (self.home / "password.json").read_text()
        self.assertNotIn(PHRASE, saved)
        self.assertIn("$argon2id$", saved)

    def unlock(self, app):
        app.send("29 unlock " + PHRASE.upper() + "!")
        app.expect("Unlocked. Waiting")

    def test_locked_gate_spoken_unlock_relock_and_restart(self):
        app = self.app()
        self.enroll(app)
        for command in ["29 send my email", "/approve 1", "/settings", "/stt test", "/password remove", "/config set security.lock-seconds 86400"]:
            app.send(command)
            app.expect("Locked. Use /unlock")
        self.unlock(app)
        app.send("29 hello")
        app.expect("You: hello")
        app.expect("Mock agent received: hello")
        app.send("29 lock")
        app.expect("Locked. Work")
        app.close()
        again = self.app()
        again.send("29 hello again")
        again.expect("Locked. Use /unlock")
        again.send("29")
        again.expect("Local unlock listening is open")
        again.send("unlock " + PHRASE)
        again.expect("Unlocked. Waiting")
        again.send("29 unlock " + PHRASE)
        again.expect("Already unlocked")
        self.assertNotIn(PHRASE, "".join(app.seen + again.seen).lower())

    def test_failed_attempt_backoff_survives_restart(self):
        app = self.app()
        self.enroll(app)
        app.send("29 unlock not the correct phrase")
        app.expect("Incorrect passphrase")
        saved = json.loads((self.home / "password.json").read_text())
        self.assertEqual(saved["failures"], 1)
        self.assertGreater(saved["retry_at"], time.time() - 1)
        app.close()
        again = self.app()
        again.send("29 unlock " + PHRASE)
        again.expect("Too many attempts")
        time.sleep(2.1)
        self.unlock(again)

    def test_keyboard_only_and_absolute_auto_lock(self):
        app = self.app()
        app.send("/config set security.spoken-unlock false")
        app.expect("Saved security.spoken-unlock")
        app.send("/config set security.lock-seconds 2")
        app.expect("Saved security.lock-seconds")
        self.enroll(app)
        app.send("29 unlock " + PHRASE)
        app.expect("Use /unlock for masked")
        app.send("/unlock")
        app.expect("Enter passphrase")
        app.send(PHRASE)
        app.expect("Unlocked. Waiting")
        app.send("29 hold")
        app.expect("You: hold")
        app.expect("Locked. Work", timeout=5)
        app.send("/worker-approve 1")
        app.expect("Locked. Use /unlock")

    def test_bridge_cannot_change_settings_while_locked(self):
        app = self.app()
        self.enroll(app)
        endpoint = json.loads(next((self.home / "sessions").glob("*.json")).read_text())
        host, port = endpoint["address"].rsplit(":", 1)
        def call(action):
            with socket.create_connection((host, int(port)), timeout=5) as conn:
                conn.sendall((json.dumps({"token": endpoint["token"], "action": action}) + "\n").encode())
                return json.loads(conn.makefile().readline())
        self.assertTrue(call("status")["locked"])
        self.assertIn("locked", call({"settings_update": {"changes": {"speak": True}}})["error"])
        self.assertIn("locked", call("settings_read")["error"])
        self.assertIn("locked", call("sleep")["error"])

    def test_corrupt_password_fails_closed(self):
        (self.home / "password.json").write_text("{broken")
        result = subprocess.run([str(BINARY), "run", "--text", "--agent", "mock"],
                                cwd=ROOT, env=self.env, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to start unlocked", result.stderr)

    def test_password_remove_only_after_unlock_and_invalid_settings(self):
        app = self.app()
        self.enroll(app)
        self.unlock(app)
        app.send("/config set security.lock-seconds 0")
        app.expect("Setting not changed")
        app.send("/password remove")
        app.expect("Password removed")
        self.assertFalse((self.home / "password.json").exists())
        app.send("/lock")
        app.expect("Set a passphrase")

    def test_second_instance_refused_and_due_work_waits(self):
        app = self.app()
        self.enroll(app)
        second = subprocess.run([str(BINARY), "run", "--text", "--agent", "mock"],
                                cwd=ROOT, env=self.env, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(second.returncode, 0)
        self.assertIn("already running", second.stderr)
        task = subprocess.run([str(BINARY), "organizer", "task", "scheduled synthetic check",
                               "--in-seconds", "1", "--harness", "mock", "--model", "mock-worker"],
                              cwd=ROOT, env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(task.returncode, 0, task.stderr)
        time.sleep(2.2)
        book = json.loads((self.home / "schedules.json").read_text())
        self.assertEqual(len(book["tasks"]), 1)
        self.assertEqual(book["runs"], [])
        self.unlock(app)
        app.expect("Scheduled task")

    def test_only_owned_legacy_cache_files_are_removed(self):
        cache = self.home / "tts-cache"
        cache.mkdir()
        (cache / "0123456789abcdef.wav").write_bytes(b"old generated clip")
        (cache / "personal.wav").write_bytes(b"unrelated")
        result = subprocess.run([str(BINARY), "tts", "clear-cache"], cwd=ROOT,
                                env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((cache / "0123456789abcdef.wav").exists())
        self.assertTrue((cache / "personal.wav").exists())

    def test_lock_remains_available_during_masked_entry(self):
        app = self.app()
        self.enroll(app)
        app.send("/unlock")
        app.expect("Enter passphrase")
        app.send("/lock")
        app.expect("Locked. Work")
        self.assertEqual(json.loads((self.home / "password.json").read_text())["failures"], 0)
        self.unlock(app)
        app.send("/password")
        app.expect("Enter at least three words")
        app.send("/lock")
        app.expect("Locked. Work")


if __name__ == "__main__":
    unittest.main(verbosity=2)
