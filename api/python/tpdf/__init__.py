"""Drive tpdf without a shell or a GUI; requires the tpdf CLI installed separately.

Install from a checkout: uv pip install ./api/python

    from tpdf import Tpdf
    pdf = Tpdf()  # or Tpdf('/path/to/tpdf-cli')
    result = pdf.edit('input.pdf', 'output.pdf', [
        {'op': 'rotate', 'page': 1, 'degrees': 90},
        {'op': 'insert_blank', 'after': 1, 'width': 595, 'height': 842},
    ])
    assert result['written']
    assert len(pdf.info('output.pdf')['files']) == 1

The client uses the same versioned JSON and exit codes as the CLI. run() exposes
all commands, including new commands a newer executable adds. Nonzero exits raise
CommandError by default; use check=False to examine negative verification results
or a partial split. A timeout may occur after an output was published: it is not a
rollback and must never be interpreted as proof that nothing was written.
"""
from __future__ import annotations

from dataclasses import dataclass
import json
import math
import os
from pathlib import Path
import shutil
import signal
import subprocess
from typing import Any, Mapping, Sequence

__all__ = ['Tpdf', 'Result', 'CommandError', 'ProtocolError', 'CommandTimeout']


@dataclass(frozen=True)
class Result:
    """The report and actual process status, including partial/negative results."""

    report: dict[str, Any]
    exit_code: int
    stderr: str


class CommandError(RuntimeError):
    """A CLI refusal, failed verification, or internal failure; report is retained."""

    def __init__(self, result: Result):
        self.result = result
        self.report = result.report
        self.exit_code = result.exit_code
        error = result.report.get('error')
        message = error.get('message') if isinstance(error, dict) else None
        super().__init__(message or result.stderr.strip() or f'tpdf exited {result.exit_code}')


class ProtocolError(RuntimeError):
    """The executable did not return the supported JSON contract."""


class CommandTimeout(TimeoutError):
    """The timeout expired. Outputs may exist; this does not imply rollback."""


def _stop(process: subprocess.Popen[bytes]) -> None:
    # The group belongs to this invocation, never to the calling test runner.
    # On Windows taskkill must see the live parent to enumerate its descendants.
    if os.name == 'nt':
        subprocess.run(
            ['taskkill', '/PID', str(process.pid), '/T', '/F'],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
            timeout=10,
        )
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


class Tpdf:
    """A stateless client. Each call runs an isolated CLI process and its workers.

    Relative document paths are resolved against cwd, when supplied. Passwords
    are passed in a child-only environment variable, never on the command line.
    No server is started and no global environment variable is changed.
    """

    def __init__(
        self,
        executable: str | os.PathLike[str] | None = None,
        *,
        timeout: float = 120,
        cwd: str | os.PathLike[str] | None = None,
    ):
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError('timeout must be a finite positive number of seconds')
        found = os.fspath(executable) if executable is not None else (
            shutil.which('tpdf-cli') or shutil.which('tpdf')
        )
        if not found:
            raise FileNotFoundError('tpdf CLI not found; pass executable= or install it on PATH')
        # Resolve a caller-supplied relative executable before changing the child cwd.
        self.executable = str(Path(shutil.which(found) or found).resolve())
        self.timeout = timeout
        self.cwd = os.fspath(cwd) if cwd is not None else None

    def run(
        self,
        command: str,
        *arguments: str | os.PathLike[str],
        input_json: Any = None,
        password: str | None = None,
        check: bool = True,
    ) -> Result:
        """Run any JSON-capable command with literal arguments (never shell syntax).

        Use input_json with commands accepting stdin: edit --plan -, fill --values -.
        Keep reports on stdout; text -o/--output is rejected because it redirects
        that report into a file. Read text through text() and write it in the caller.
        """
        if not command or command.startswith('-'):
            raise ValueError('command must be a subcommand name, such as info or edit')
        args = [os.fspath(arg) for arg in arguments]
        flags = args[:args.index('--')] if '--' in args else args
        if command == 'text' and any(arg in ('-o', '--output') for arg in flags):
            raise ValueError('text output must remain on stdout for the API')
        env = os.environ.copy()
        options = ['--json']
        if password is not None:
            env['TPDF_API_DOCUMENT_PASSWORD'] = password
            options += ['--password-env', 'TPDF_API_DOCUMENT_PASSWORD']
        payload = None if input_json is None else json.dumps(
            input_json, ensure_ascii=False, allow_nan=False, separators=(',', ':'),
        ).encode('utf-8')
        process = subprocess.Popen(
            [self.executable, command, *options, *args],
            stdin=subprocess.PIPE if payload is not None else subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env=env, cwd=self.cwd, start_new_session=os.name != 'nt',
            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == 'nt' else 0,
        )
        try:
            stdout, stderr = process.communicate(payload, timeout=self.timeout)
        except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
            try:
                _stop(process)
            except (OSError, subprocess.SubprocessError):
                # Keep cleanup bounded even if taskkill is unavailable or denied.
                pass
            finally:
                process.kill()
                try:
                    process.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    # A descendant may have retained a pipe after the parent died.
                    # Closing our copies keeps the caller's deadline bounded.
                    for stream in (process.stdin, process.stdout, process.stderr):
                        if stream is not None:
                            stream.close()
                    process.wait(timeout=5)
            if isinstance(error, KeyboardInterrupt):
                raise
            raise CommandTimeout(
                f'tpdf {command} exceeded {self.timeout} seconds; outputs may already exist'
            ) from None
        try:
            report = json.loads(stdout.decode('utf-8'))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ProtocolError(
                f'tpdf {command} exited {process.returncode} without a valid JSON report'
            ) from error
        if not isinstance(report, dict) or type(report.get('schema')) is not int or report['schema'] != 1:
            raise ProtocolError('unsupported or missing tpdf report schema; expected 1')
        if report.get('command') != command:
            raise ProtocolError(f'tpdf report command does not match {command}')
        result = Result(report, process.returncode, stderr.decode('utf-8', errors='replace'))
        if check and result.exit_code != 0:
            raise CommandError(result)
        return result

    def help(self) -> dict[str, Any]:
        """Discover the executable's commands and application version."""
        return self.run('help').report

    def info(self, *paths: str | os.PathLike[str], password: str | None = None) -> dict[str, Any]:
        """Read document information."""
        return self.run('info', '--', *paths, password=password).report

    def text(self, path: str | os.PathLike[str], *, password: str | None = None) -> dict[str, Any]:
        """Extract text and its reading order."""
        return self.run('text', '--', path, password=password).report

    def fields(self, path: str | os.PathLike[str], *, password: str | None = None) -> dict[str, Any]:
        """Inspect form fields and the answers they accept."""
        return self.run('fields', '--', path, password=password).report

    def comments(self, path: str | os.PathLike[str], *, password: str | None = None) -> dict[str, Any]:
        """Read annotations; incomplete scans raise CommandError with their report."""
        return self.run('comments', '--', path, password=password).report

    def text_runs(self, path: str | os.PathLike[str], *, page: int = 1, password: str | None = None) -> dict[str, Any]:
        """Inspect original text runs and the revision required for replacement."""
        return self.run('text-runs', '--page', str(page), '--', path, password=password).report

    def render(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        page: int = 1, dpi: int = 144, force: bool = False, password: str | None = None,
    ) -> dict[str, Any]:
        """Render one page to PNG for visual assertions, without opening a window."""
        args = ['-o', os.fspath(output), '--page', str(page), '--dpi', str(dpi)]
        if force:
            args.append('--force')
        return self.run('render', *args, '--', source, password=password).report

    def fill(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str],
        values: Mapping[str, Any], *, force: bool = False, password: str | None = None,
    ) -> dict[str, Any]:
        """Fill fields from a Python mapping without an intermediate JSON file."""
        args = ['-o', os.fspath(output), '--values', '-']
        if force:
            args.append('--force')
        return self.run('fill', *args, '--', source, input_json=dict(values), password=password).report

    def edit(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str],
        operations: Sequence[Mapping[str, Any]], *, dry_run: bool = False,
        force: bool = False, invalidate_signatures: bool = False,
        password: str | None = None,
    ) -> dict[str, Any]:
        """Apply ordered schema-1 edits. A rejected plan publishes no output."""
        args = ['--plan', '-', '-o', os.fspath(output)]
        if dry_run:
            args.append('--dry-run')
        if force:
            args.append('--force')
        if invalidate_signatures:
            args.append('--invalidate-signatures')
        args += ['--', os.fspath(source)]
        return self.run(
            'edit', *args, input_json={'schema': 1, 'operations': list(operations)},
            password=password,
        ).report

    def verify(self, *paths: str | os.PathLike[str], strict: bool = False) -> dict[str, Any]:
        """Inspect signatures. With strict=True, unsigned/untrusted files raise CommandError.

        The exception retains the full verification report and exit code. Use
        run('verify', '--strict', '--', *paths, check=False) for a Result instead.
        """
        args = ['--strict'] if strict else []
        return self.run('verify', *args, '--', *paths).report

    def _pages(
        self, command: str, sources: Sequence[str | os.PathLike[str]],
        output: str | os.PathLike[str], options: Sequence[str], *,
        force: bool, invalidate_signatures: bool, password: str | None,
    ) -> dict[str, Any]:
        args = ['-o', os.fspath(output), *options]
        if force:
            args.append('--force')
        if invalidate_signatures:
            args.append('--invalidate-signatures')
        return self.run(command, *args, '--', *sources, password=password).report

    def merge(
        self, sources: Sequence[str | os.PathLike[str]], output: str | os.PathLike[str], *,
        force: bool = False, invalidate_signatures: bool = False, password: str | None = None,
    ) -> dict[str, Any]:
        """Combine at least two documents in order. Only the first may be encrypted."""
        if isinstance(sources, (str, bytes, os.PathLike)):
            raise TypeError('sources must be a sequence of document paths, not one path')
        return self._pages('merge', sources, output, [], force=force,
            invalidate_signatures=invalidate_signatures, password=password)

    def extract(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        pages: str, force: bool = False, invalidate_signatures: bool = False,
        password: str | None = None,
    ) -> dict[str, Any]:
        """Copy a page range (e.g. '1-3,7'), in document order, each page once."""
        return self._pages('extract', [source], output, ['--pages', pages], force=force,
            invalidate_signatures=invalidate_signatures, password=password)

    def split(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        every: int = 1, force: bool = False, invalidate_signatures: bool = False,
        password: str | None = None,
    ) -> dict[str, Any]:
        """Write groups as output-1.pdf, output-2.pdf, etc.; report lists actual paths.

        A publication failure can leave some parts written. CommandError.report
        retains those paths; do not interpret an exception as a rollback.
        """
        return self._pages('split', [source], output, ['--every', str(every)], force=force,
            invalidate_signatures=invalidate_signatures, password=password)

    def rotate(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        degrees: int, pages: str | None = None, force: bool = False,
        invalidate_signatures: bool = False, password: str | None = None,
    ) -> dict[str, Any]:
        """Turn selected pages clockwise by 90, 180 or 270 degrees; default all pages."""
        options = ['--degrees', str(degrees)]
        if pages is not None:
            options += ['--pages', pages]
        return self._pages('rotate', [source], output, options, force=force,
            invalidate_signatures=invalidate_signatures, password=password)

    def crop(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        rect: Sequence[float], pages: str | None = None, force: bool = False,
        invalidate_signatures: bool = False, password: str | None = None,
    ) -> dict[str, Any]:
        """Set visible x,y,width,height in display points from top-left.

        Cropping hides content; it does not redact it. The CLI validates geometry.
        """
        if isinstance(rect, (str, bytes)) or len(rect) != 4:
            raise ValueError('rect must contain four numbers: x, y, width, height')
        options = ['--rect', ','.join(str(value) for value in rect)]
        if pages is not None:
            options += ['--pages', pages]
        return self._pages('crop', [source], output, options, force=force,
            invalidate_signatures=invalidate_signatures, password=password)

    def redact(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str] | None = None, *,
        texts: Sequence[str] = (), patterns: Sequence[str] = (),
        regions: Sequence[Mapping[str, Any]] | None = None,
        pages: str | None = None, case_sensitive: bool = False, dry_run: bool = False,
        force: bool = False, invalidate_signatures: bool = False,
        password: str | None = None, check: bool = True,
    ) -> dict[str, Any]:
        """Remove text, regex matches or rectangles, using the CLI's verified writer.

        Regions are {'page': 1, 'rect': [x, y, width, height]} in display points
        from top-left, sent through stdin. pages limits searches only; regions
        name their own pages. Matching ignores case unless case_sensitive=True.
        A dry run (output optional) writes nothing and has verified=None.

        A written but unverified copy raises CommandError with exit_code=1;
        its report and file remain available. check=False returns that report
        directly: inspect written, verified and reasons. Exit 0 alone does not
        prove a redaction: no matches also writes nothing with verified=None.
        """
        args = []
        if output is not None:
            args += ['-o', os.fspath(output)]
        for flag, queries in [('--text', texts), ('--pattern', patterns)]:
            if isinstance(queries, (str, bytes)):
                raise TypeError(f'{flag} queries must be a sequence of strings, not one string')
            for query in queries:
                if not isinstance(query, str):
                    raise TypeError(f'{flag} queries must be strings')
                args += [flag, query]
        payload = None
        if regions is not None:
            if isinstance(regions, (str, bytes, Mapping)):
                raise TypeError('regions must be a sequence of page/rect mappings')
            payload = [dict(region) for region in regions]
            args += ['--regions', '-']
        if pages is not None:
            args += ['--pages', pages]
        for enabled, flag in [(case_sensitive, '--case-sensitive'), (dry_run, '--dry-run'),
                              (force, '--force'), (invalidate_signatures, '--invalidate-signatures')]:
            if enabled:
                args.append(flag)
        return self.run('redact', *args, '--', source, input_json=payload,
                        password=password, check=check).report
