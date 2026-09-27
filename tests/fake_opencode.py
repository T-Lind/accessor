"""Offline opencode fixture: emits opencode-style NDJSON for one headless run."""
import json
import sys


def emit(value):
    print(json.dumps(value), flush=True)


text = sys.stdin.read().strip()
prompt = (text.split("Current request:\n")[-1] if "Current request:\n" in text else text.split("\n\n")[-1]).strip()
session = "ses_fixture"

emit({"type": "step_start", "timestamp": 1, "sessionID": session, "part": {"type": "step-start"}})
emit(
    {
        "type": "text",
        "timestamp": 2,
        "sessionID": session,
        "part": {"type": "text", "text": "opencode reply: " + prompt},
    }
)
emit({"type": "step_finish", "timestamp": 3, "sessionID": session, "part": {"type": "step-finish"}})
