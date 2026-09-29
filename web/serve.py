"""Local dev server for the Orbi landing page.

Same as `python3 -m http.server`, but tells the browser never to cache, so a
reload always shows the latest HTML, CSS and JS together.

    python3 serve.py            # http://127.0.0.1:4310
"""
import http.server
import sys
from functools import partial
from pathlib import Path

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 4310


class NoCache(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store, max-age=0")
        super().end_headers()

    def log_message(self, *args):
        pass


# Threaded: browsers open several connections at once, and a single-threaded
# server drops some of them (connection reset → missing CSS/JS/images).
http.server.ThreadingHTTPServer.allow_reuse_address = True
http.server.ThreadingHTTPServer.daemon_threads = True
with http.server.ThreadingHTTPServer(("127.0.0.1", PORT), partial(NoCache, directory=str(Path(__file__).parent))) as srv:
    print(f"Orbi landing page on http://127.0.0.1:{PORT}")
    srv.serve_forever()
