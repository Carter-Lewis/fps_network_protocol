#!/usr/bin/env python3
"""Serve the Godot web export with Cross-Origin Isolation headers.

Looks for cert.pem / key.pem (written by the Rust game server) to enable HTTPS.
HTTPS is required when serving from a non-localhost address.

Usage:
    python3 serve.py [port] [cert.pem] [key.pem]

Examples:
    python3 serve.py                          # HTTP on port 8000 (localhost dev only)
    python3 serve.py 8000 cert.pem key.pem    # HTTPS on port 8000
"""
import http.server
import os
import ssl
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
GAME_DIR = os.path.dirname(os.path.abspath(__file__))
SERVER_DIR = os.path.join(GAME_DIR, "..", "server")

# Resolve cert/key: explicit args > next to serve.py > ../server/ (EC2 layout)
def find_file(name, explicit=None):
    if explicit:
        return explicit
    for d in [GAME_DIR, SERVER_DIR]:
        p = os.path.join(d, name)
        if os.path.exists(p):
            return p
    return None

CERT = find_file("cert.pem", sys.argv[2] if len(sys.argv) > 2 else None)
KEY  = find_file("key.pem",  sys.argv[3] if len(sys.argv) > 3 else None)


class COIHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=GAME_DIR, **kwargs)

    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()

    def log_message(self, fmt, *args):
        pass


import socketserver
with socketserver.TCPServer(("", PORT), COIHandler) as httpd:
    if CERT and KEY:
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(CERT, KEY)
        httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
        scheme = "https"
    else:
        scheme = "http"
        if PORT != 8000 or True:  # always warn when no TLS
            print("WARNING: no cert.pem/key.pem found — serving HTTP (localhost only)")

    print(f"Serving {GAME_DIR}")
    print(f"Open {scheme}://localhost:{PORT}/FPS%20Networking%20Demo.html")
    httpd.serve_forever()
