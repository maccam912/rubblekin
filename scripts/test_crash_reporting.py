#!/usr/bin/env python3
"""Exercise real Sentry startup-error, panic and native-abort capture locally.

Build with `cargo build -p rubblekin_client --example crash_report`, then run
this script. No events are sent to hosted Sentry.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def items(envelope):
    _, _, remaining = envelope.partition(b"\n")
    while remaining:
        header, _, remaining = remaining.partition(b"\n")
        metadata = json.loads(header)
        length = metadata["length"]
        yield metadata, remaining[:length]
        remaining = remaining[length:].lstrip(b"\n")


def verify(binary):
    received = queue.Queue()

    class Collector(BaseHTTPRequestHandler):
        def do_POST(self):
            received.put(self.rfile.read(int(self.headers["Content-Length"])))
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"{}")

        def log_message(self, *args):
            pass

    with ThreadingHTTPServer(("127.0.0.1", 0), Collector) as server:
        threading.Thread(target=server.serve_forever, daemon=True).start()
        environment = os.environ.copy()
        environment["SENTRY_DSN"] = f"http://public@127.0.0.1:{server.server_port}/1"
        environment["SENTRY_ENVIRONMENT"] = "verification"
        environment.pop("SENTRY_AUTH_TOKEN", None)
        for mode in ("error", "renderer", "panic", "abort"):
            result = subprocess.run([str(binary), mode], env=environment,
                                    capture_output=True, timeout=30)
            expected_success = mode in ("error", "renderer")
            assert (result.returncode == 0) == expected_success, result.stderr.decode(errors="replace")
            records = list(items(received.get(timeout=15)))
            event = next(json.loads(payload) for metadata, payload in records
                         if metadata["type"] == "event")
            assert event["release"].startswith("rubblekin@"), event
            assert event["environment"] == "verification", event
            assert event["tags"]["client.phase"] == "verification", event
            assert "user" not in event and "server_name" not in event, event
            if mode == "abort":
                assert any(metadata.get("attachment_type") == "event.minidump"
                           and payload.startswith(b"MDMP") for metadata, payload in records), records
            else:
                assert event.get("exception", {}).get("values"), event
            if mode == "renderer":
                assert "Renderer DeviceLost" in json.dumps(event), event
                assert received.empty(), "Repeated renderer polling sent duplicate events"
            print(f"PASS: {mode} uploaded a real Sentry event with build/context metadata")
        for dsn in ("", "invalid-dsn"):
            environment["SENTRY_DSN"] = dsn
            result = subprocess.run([str(binary), "error"], env=environment,
                                    capture_output=True, timeout=10)
            assert result.returncode == 0, result.stderr
            assert received.empty(), "Disabled reporting sent an event"
        print("PASS: absent/invalid DSN preserves startup and sends no reports")
        server.shutdown()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path,
                        default=Path("target/debug/examples/crash_report.exe" if os.name == "nt"
                                     else "target/debug/examples/crash_report"))
    verify(parser.parse_args().binary.resolve())
