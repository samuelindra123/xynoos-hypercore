#!/usr/bin/env python3
"""Run the three independent static concepts. Ctrl-C stops this launcher."""
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from functools import partial
from pathlib import Path
from threading import Thread
import signal
root = Path(__file__).resolve().parent
servers = []
try:
    for port, concept in zip((4101, 4102, 4103), ('a', 'b', 'c')):
        handler = partial(SimpleHTTPRequestHandler, directory=str(root / f'concept-{concept}'))
        server = ThreadingHTTPServer(('0.0.0.0', port), handler)
        servers.append(server)
        Thread(target=server.serve_forever, daemon=True).start()
        print(f'Concept {concept.upper()}: http://localhost:{port}/', flush=True)
    signal.pause()
except KeyboardInterrupt:
    pass
finally:
    for server in servers:
        server.shutdown()
        server.server_close()
