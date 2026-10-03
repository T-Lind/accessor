"""Offline MPV IPC / Soloist fixtures; never decode or play audio."""
import json
import os
from pathlib import Path
import socket
import sys
import time

mode, *args = sys.argv[1:]
log = Path(os.environ["ACC_MUSIC_TEST_LOG"])

def record(value):
    with log.open("a") as output:
        output.write(json.dumps(value) + "\n")

if mode == "soloist":
    record({"backend": mode, "args": args})
    if os.environ.get("ACC_MUSIC_TEST_FAIL"):
        print("not logged in", file=sys.stderr)
        sys.exit(3)
    print("fixture: playing" if args[-1] == "status" else "")
else:
    path = next(arg.split("=", 1)[1] for arg in args if arg.startswith("--input-ipc-server="))
    server = socket.socket(socket.AF_UNIX)
    server.bind(path)
    # A bound path exists before the player is ready to accept commands.
    time.sleep(0.05)
    server.listen(8)
    record({"backend": mode, "pid": os.getpid(), "args": args})
    values = {"pause": False, "idle-active": True, "volume": 25,
              "audio-out-params": {"samplerate": 44100}}
    while True:
        connection, _ = server.accept()
        with connection:
            request = json.loads(connection.makefile().readline())
            command = request["command"]
            record({"backend": mode, "command": command})
            error, data = "success", None
            if command[0] == "loadfile":
                if os.environ.get("ACC_MUSIC_TEST_FAIL"):
                    error = "loading failed"
                else:
                    values["idle-active"] = False
            elif command[0] == "set_property":
                values[command[1]] = command[2]
            elif command[0] == "get_property":
                data = values.get(command[1])
            connection.sendall((json.dumps({"request_id": request["request_id"],
                                           "error": error, "data": data}) + "\n").encode())
