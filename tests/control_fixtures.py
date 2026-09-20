"""Deterministic local-control replies shared by offline harness fixtures."""
import json


def control_reply(text):
    if text == "stop the alarm":
        action = dict(action="stop_alarm")
    elif text == "schedule fixture":
        action = dict(action="schedule", prompt="Report", delay_seconds=3600,
                      harness="mock", model="mock-light", reasoning="medium")
    elif text.startswith("edit fixture "):
        action = dict(action="update_schedule", id=text.split()[-1],
                      changes=dict(prompt="Changed by harness", reasoning="high", paused=True))
    elif text.startswith("delete fixture "):
        action = dict(action="delete_schedule", id=text.split()[-1])
    else:
        return None
    return json.dumps({"accessor": action})
