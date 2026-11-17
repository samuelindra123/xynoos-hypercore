#!/usr/bin/env python3
"""APT retrieves a signature-checked index and every retained version, without host install."""
import argparse
import functools
import getpass
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import tempfile
import threading
from archive import verify, run, require, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    parser.add_argument('--fingerprint', required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    ledger = verify(root, args.fingerprint)
    handler = functools.partial(SimpleHTTPRequestHandler, directory=str(root))
    server = ThreadingHTTPServer(('127.0.0.1', 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            for sub in ['lists/partial', 'cache/archives/partial', 'empty']:
                (work / sub).mkdir(parents=True)
            source = work / 'sources.list'
            source.write_text(f'deb [arch=amd64 signed-by={root}/xynoos-archive-keyring.gpg] http://127.0.0.1:{server.server_port} preview main\n')
            options = ['-o', f'Dir::Etc::sourcelist={source}', '-o', f'Dir::Etc::sourceparts={work}/empty',
                       '-o', f'Dir::State::lists={work}/lists', '-o', f'Dir::Cache={work}/cache',
                       '-o', f'APT::Sandbox::User={getpass.getuser()}']
            run('apt-get', *options, 'update')
            for version, record in ledger['versions'].items():
                run('apt-get', *options, 'download', f'xynoos-hypercore={version}', cwd=work)
                require(digest(work / Path(record['filename']).name) == record['sha256'], 'APT download digest mismatch')
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    print('APT verified signed indexes and downloaded every retained version')


if __name__ == '__main__':
    main()
