"""Exercise certificate-file signing from a foreground Python client on Linux."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

from test_api import BINARY, fixture


def sign_from_terminal(executable):
    from concurrent.futures import ThreadPoolExecutor
    import fcntl
    import hashlib
    import ssl
    import termios
    from tpdf_client import CommandTimeout, Tpdf

    # The parent owns the PTY master. Make the slave our controlling terminal.
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)
    initial = termios.tcgetattr(0)
    subprocess.run([executable, 'identities', '--add', 'test.p12'], check=True, timeout=15)
    signer = Tpdf(executable)
    cert = ssl.PEM_cert_to_DER_cert(Path('cert.pem').read_text(encoding='ascii'))
    identity = hashlib.sha256(cert).hexdigest()
    assert any(i['id'] == identity for i in signer.identities()['usable'])
    signer.sign('source.pdf', 'signed.pdf', identity=identity)
    signatures = signer.verify('signed.pdf')['files'][0]['signatures']
    assert len(signatures) == 1 and signatures[0]['integrity']['verdict'] == 'intact'

    with ThreadPoolExecutor(max_workers=1) as pool:
        first = pool.submit(Tpdf(executable, timeout=4).sign, 'source.pdf', 'cancelled.pdf', identity=identity)
        deadline = time.monotonic() + 3
        while termios.tcgetattr(0)[3] & termios.ECHO and time.monotonic() < deadline:
            time.sleep(0.01)
        assert not termios.tcgetattr(0)[3] & termios.ECHO, 'first prompt never took the terminal'
        try:
            signer.sign('source.pdf', 'overlap.pdf', identity=identity)
        except RuntimeError as error:
            assert 'using the terminal' in str(error)
            assert not termios.tcgetattr(0)[3] & termios.ECHO
        else:
            raise AssertionError('two signers borrowed the same terminal')
        competing = subprocess.run(
            [sys.executable, '-c',
             "from tpdf_client import Tpdf; import sys; "
             "Tpdf(sys.argv[1]).sign('source.pdf', 'overlap.pdf', identity=sys.argv[2])",
             executable, identity],
            capture_output=True, timeout=2,
        )
        assert competing.returncode != 0 and b'using the terminal' in competing.stderr
        try:
            first.result(timeout=6)
        except CommandTimeout:
            assert termios.tcgetattr(0) == initial and not Path('cancelled.pdf').exists()
            assert not Path('overlap.pdf').exists()
            assert not Path(f'/proc/self/task/{os.getpid()}/children').read_text().strip()
        else:
            raise AssertionError('expected a timeout waiting for the password')


@unittest.skipUnless(sys.platform == 'linux' and shutil.which('openssl'), 'Linux and OpenSSL required')
class LinuxSigningTests(unittest.TestCase):
    def test_sign_and_timeout_restore_the_terminal(self):
        import pty
        import select

        with tempfile.TemporaryDirectory(prefix='tpdf-python-sign-') as directory:
            folder = Path(directory)
            fixture(folder / 'source.pdf')
            for args in (
                ['req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256', '-nodes',
                 '-subj', '/CN=Synthetic signer', '-addext', 'basicConstraints=CA:FALSE',
                 '-addext', 'keyUsage=digitalSignature', '-keyout', 'key.pem', '-out', 'cert.pem'],
                ['pkcs12', '-export', '-inkey', 'key.pem', '-in', 'cert.pem', '-out', 'test.p12',
                 '-passout', 'pass:synthetic'],
            ):
                subprocess.run(['openssl', *args], cwd=folder, check=True, capture_output=True, timeout=15)
            master, slave = pty.openpty()
            with os.fdopen(master, 'r+b', buffering=0) as terminal, os.fdopen(slave, 'rb') as child_tty:
                with subprocess.Popen(
                    [sys.executable, __file__, str(BINARY.resolve())], cwd=folder,
                    env={**os.environ, 'XDG_CONFIG_HOME': directory}, start_new_session=True,
                    stdin=child_tty, stdout=child_tty, stderr=child_tty,
                ) as child:
                    prompts = 0
                    output = bytearray()
                    deadline = time.monotonic() + 30
                    try:
                        while child.poll() is None and time.monotonic() < deadline:
                            if not select.select([terminal], [], [], 0.2)[0]:
                                continue
                            output.extend(os.read(terminal.fileno(), 4096))
                            count = output.count(b'Password: ')
                            if count > prompts:
                                prompts = count
                                if prompts <= 2:
                                    terminal.write(b'synthetic\n')
                        self.assertEqual(child.poll(), 0, output.decode(errors='replace'))
                        self.assertEqual(prompts, 3)
                    finally:
                        if child.poll() is None:
                            child.kill()


if __name__ == '__main__':
    sign_from_terminal(sys.argv[1])
