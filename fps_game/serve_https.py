#!/usr/bin/env python3
"""
Simple HTTPS server for serving exported Godot HTML5 builds.
Generates a self-signed certificate if one doesn't exist.
"""

import http.server
import ssl
import os
import subprocess

# Generate a self-signed cert if one doesn't exist
cert_file = 'cert.pem'
if not os.path.exists(cert_file):
    print("Generating self-signed certificate...")
    subprocess.run([
        'openssl', 'req', '-x509', '-newkey', 'rsa:2048',
        '-keyout', cert_file, '-out', cert_file,
        '-days', '365', '-nodes', '-subj', '/CN=localhost'
    ], check=True)
    print(f"Certificate generated: {cert_file}")

port = 8000
handler = http.server.SimpleHTTPRequestHandler

# Create SSL context with modern API
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(cert_file)

httpd = http.server.ThreadingHTTPServer(('0.0.0.0', port), handler)
httpd.socket = context.wrap_socket(httpd.socket, server_side=True)

print(f"Serving on https://localhost:{port}")
print("Press Ctrl+C to stop")
print("\nOpen https://localhost:8000/FPS%20Networking%20Demo.html in your browser")
print("(Ignore the certificate warning—it's self-signed for local testing)")

try:
    httpd.serve_forever()
except KeyboardInterrupt:
    print("\nServer stopped")
