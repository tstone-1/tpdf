"""Drive tpdf without a shell or a GUI; requires the tpdf CLI installed separately.

Install from a checkout: uv pip install ./api/python

    from tpdf_client import Tpdf
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

from . import reports

__all__ = ['Tpdf', 'Result', 'CommandError', 'ProtocolError', 'CommandTimeout', 'reports']


@dataclass(frozen=True)
class Result:
    """The report and actual process status, including partial/negative results."""

    report: dict[str, Any]
    exit_code: int
    stderr: str

    @property
    def typed(self) -> Any:
        """The report, for a method that names its shape in `tpdf_client.reports`.

        The shape is the CLI's promise, held against its committed samples by
        `test_reports.py`; nothing here checks a report against it at run time.
        """
        return self.report


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
        new_password: str | None = None,
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
        if new_password is not None:
            env['TPDF_API_NEW_PASSWORD'] = new_password
            options += ['--new-password-env', 'TPDF_API_NEW_PASSWORD']
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

    def help(self, command: str | None = None) -> reports.HelpReport:
        """Discover the executable's commands and application version.

        With command, the report lists that command alone.
        """
        if command is None:
            return self.run('help').typed
        if not command or command.startswith('-'):
            raise ValueError('command must be a subcommand name, such as info or edit')
        return self.run('help', command).typed

    def info(self, *paths: str | os.PathLike[str], password: str | None = None) -> reports.InfoReport:
        """Read document information."""
        return self.run('info', '--', *paths, password=password).typed

    def text(self, path: str | os.PathLike[str], *, password: str | None = None) -> reports.TextReport:
        """Extract text and its reading order."""
        return self.run('text', '--', path, password=password).typed

    def search(
        self, *paths: str | os.PathLike[str],
        texts: Sequence[str] = (), patterns: Sequence[str] = (),
        pages: str | None = None, case_sensitive: bool = False, whole_word: bool = False,
        password: str | None = None,
    ) -> reports.SearchReport:
        """Find text or regex matches, as the viewer's find and redact() find them.

        Returns one entry per document in report['files'], each with its matches
        (page, before, hit, after) and pages_without_text: a scanned page has
        nothing to search, which is not the same as holding no match.

        Finding nothing is an answer, not an error: the CLI's exit 1 is returned
        as a report whose files have empty matches. A document that could not be
        read raises CommandError, and the report keeps the other documents.
        """
        if not paths:
            raise ValueError('search needs at least one document')
        args = []
        for flag, queries in [('--text', texts), ('--pattern', patterns)]:
            if isinstance(queries, (str, bytes)):
                raise TypeError(f'{flag} queries must be a sequence of strings, not one string')
            for query in queries:
                if not isinstance(query, str):
                    raise TypeError(f'{flag} queries must be strings')
                args += [flag, query]
        if not args:
            raise ValueError('search needs something to find: texts= or patterns=')
        if pages is not None:
            args += ['--pages', pages]
        for enabled, flag in [(case_sensitive, '--case-sensitive'), (whole_word, '--whole-word')]:
            if enabled:
                args.append(flag)
        result = self.run('search', *args, '--', *paths, password=password, check=False)
        if result.exit_code not in (0, 1):
            raise CommandError(result)
        return result.typed

    def fields(self, path: str | os.PathLike[str], *, password: str | None = None) -> reports.FieldsReport:
        """Inspect form fields and the answers they accept."""
        return self.run('fields', '--', path, password=password).typed

    def comments(self, path: str | os.PathLike[str], *, password: str | None = None) -> reports.CommentsReport:
        """Read annotations; incomplete scans raise CommandError with their report."""
        return self.run('comments', '--', path, password=password).typed

    def text_runs(self, path: str | os.PathLike[str], *, page: int = 1, password: str | None = None) -> reports.TextRunsReport:
        """Inspect original text runs and the revision required for replacement."""
        return self.run('text-runs', '--page', str(page), '--', path, password=password).typed

    def hidden(
        self, path: str | os.PathLike[str], *, pages: str | None = None, password: str | None = None,
    ) -> reports.HiddenReport:
        """List text that is in a document and not visible on its pages.

        The check for a document redacted elsewhere: words under a black box,
        under an annotation, in the background's colour, never painted, or
        outside the page. Each entry of report['found'] has the page, the words
        and where they are.

        Finding something is an answer, not an error: the CLI's exit 1 is
        returned as a report whose found is not empty. An empty found means
        nothing was found, not that the document is clean: without_text lists
        the pages that have no text and were not compared, and unjudged counts
        the characters that could not be decided.
        """
        args = [] if pages is None else ['--pages', pages]
        result = self.run('hidden', *args, '--', path, password=password, check=False)
        if result.exit_code not in (0, 1):
            raise CommandError(result)
        return result.typed

    def render(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        page: int = 1, dpi: int = 144, force: bool = False, password: str | None = None,
    ) -> reports.RenderReport:
        """Render one page to PNG for visual assertions, without opening a window."""
        args = ['-o', os.fspath(output), '--page', str(page), '--dpi', str(dpi)]
        if force:
            args.append('--force')
        return self.run('render', *args, '--', source, password=password).typed

    def ocr(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        pages: str | None = None, languages: Sequence[str] = (), force: bool = False,
        invalidate_signatures: bool = False, password: str | None = None,
    ) -> reports.OcrReport:
        """Write a copy whose scanned pages can be searched and selected.

        Pages without text are read by the operating system's text recogniser;
        pages that already have text are left as they are. `languages` are
        BCP-47 tags such as "de-DE", most preferred first.
        """
        args = ['-o', os.fspath(output)]
        if pages is not None:
            args += ['--pages', pages]
        for language in languages:
            args += ['--language', language]
        if force:
            args.append('--force')
        if invalidate_signatures:
            args.append('--invalidate-signatures')
        return self.run('ocr', *args, '--', source, password=password).typed

    def images(
        self, pictures: Sequence[str | os.PathLike[str]], output: str | os.PathLike[str], *,
        paper: str | None = None, dpi: int | None = None, force: bool = False,
    ) -> reports.ImagesReport:
        """Write a document with one page for each PNG or JPEG picture, in order.

        A page is the picture's own size unless `paper` is "a4" or "letter";
        `dpi` replaces the resolution each file states.
        """
        if isinstance(pictures, (str, bytes, os.PathLike)):
            raise TypeError('pictures must be a sequence of paths, not one path')
        args = ['-o', os.fspath(output)]
        if paper is not None:
            args += ['--paper', paper]
        if dpi is not None:
            args += ['--dpi', str(dpi)]
        if force:
            args.append('--force')
        return self.run('images', *args, '--', *pictures).typed

    def protect(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str],
        new_password: str, *, force: bool = False,
        invalidate_signatures: bool = False, password: str | None = None,
    ) -> reports.ProtectReport:
        """Write a copy that needs `new_password` to open, encrypted with AES-256.

        `password` opens a source that already has one; the copy gets the new
        one instead. Neither password is put on the command line.
        """
        args = ['-o', os.fspath(output)]
        if force:
            args.append('--force')
        if invalidate_signatures:
            args.append('--invalidate-signatures')
        return self.run(
            'protect', *args, '--', source, password=password, new_password=new_password,
        ).typed

    def compress(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str] | None = None, *,
        pictures: str | None = None, dpi: int | None = None, quality: int | None = None,
        jpeg: bool = True, preview: str | os.PathLike[str] | None = None,
        force: bool = False, invalidate_signatures: bool = False, password: str | None = None,
    ) -> reports.CompressReport:
        """Write a smaller copy, or with no `output` say what one would come to.

        On its own nothing a reader sees changes. `pictures` names a preset
        (`screen`, `balanced`, `print`) that scales pictures down and stores
        photographs as JPEG; `dpi` and `quality` set those numbers directly,
        and `jpeg=False` keeps lossless pictures lossless. `preview` writes one
        part of one page before and after as a PNG. A copy that would not be
        smaller is refused.
        """
        args: list[str] = ['--dry-run'] if output is None else ['-o', os.fspath(output)]
        if pictures is not None:
            args += ['--pictures', pictures]
        for flag, number in (('--dpi', dpi), ('--quality', quality)):
            if number is not None:
                if isinstance(number, bool) or not isinstance(number, int):
                    raise TypeError(f'{flag[2:]} is a whole number')
                args += [flag, str(number)]
        if not jpeg:
            args.append('--no-jpeg')
        if preview is not None:
            args += ['--preview', os.fspath(preview)]
        if force:
            args.append('--force')
        if invalidate_signatures:
            args.append('--invalidate-signatures')
        return self.run('compress', *args, '--', source, password=password).typed

    def add_fields(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str],
        fields: Sequence[Mapping[str, Any]], *, force: bool = False,
        invalidate_signatures: bool = False, password: str | None = None,
    ) -> reports.FormReport:
        """Write a copy with form fields added, which `fill` can then answer.

        Each field is a mapping with `name`, `kind` (`text`, `multiline`,
        `checkbox` or `dropdown`), `page` counted from 1 and `rect` as `[left,
        top, width, height]` in points from the page's top-left corner; a
        dropdown also has `options`, the list of its choices; `tooltip`,
        `required` and `max_length` are optional. One field that cannot be
        added means none is, and the error names every problem.
        """
        args = ['-o', os.fspath(output), '--fields', '-']
        if force:
            args.append('--force')
        if invalidate_signatures:
            args.append('--invalidate-signatures')
        return self.run(
            'form', *args, '--', source, input_json=[dict(field) for field in fields],
            password=password,
        ).typed

    def unprotect(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str],
        password: str, *, force: bool = False, invalidate_signatures: bool = False,
    ) -> reports.ProtectReport:
        """Write a copy that opens without a password, from a source that needs one.

        Refused for a source that opens without a password: restrictions such a
        document carries are left in place.
        """
        args = ['-o', os.fspath(output)]
        if force:
            args.append('--force')
        if invalidate_signatures:
            args.append('--invalidate-signatures')
        return self.run('unprotect', *args, '--', source, password=password).typed

    def fill(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str],
        values: Mapping[str, Any], *, force: bool = False, password: str | None = None,
    ) -> reports.FillReport:
        """Fill fields from a Python mapping without an intermediate JSON file."""
        args = ['-o', os.fspath(output), '--values', '-']
        if force:
            args.append('--force')
        return self.run('fill', *args, '--', source, input_json=dict(values), password=password).typed

    def edit(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str],
        operations: Sequence[Mapping[str, Any]], *, dry_run: bool = False,
        force: bool = False, invalidate_signatures: bool = False,
        password: str | None = None,
    ) -> reports.EditReport:
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
        ).typed

    def mark_matches(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        texts: Sequence[str] = (), patterns: Sequence[str] = (),
        kind: str = 'highlight', color: Sequence[float] | None = None,
        pages: str | None = None, case_sensitive: bool = False, whole_word: bool = False,
        force: bool = False, invalidate_signatures: bool = False,
        password: str | None = None,
    ) -> reports.EditReport | None:
        """Highlight, underline, strike out or squiggle every match, into output.

        search() finds the matches and edit() marks each of their rectangles.
        Returns the edit report, or None when nothing matched: no file is
        written then, so an output that exists means something was marked.
        color is red, green and blue from 0 to 1; without it edit's default applies.

        A match whose characters have no position on the page raises ValueError
        before anything is written, rather than being left unmarked in silence.
        """
        if kind not in ('highlight', 'underline', 'strikeout', 'squiggly'):
            raise ValueError('kind must be highlight, underline, strikeout or squiggly')
        if color is not None:
            color = [float(channel) for channel in color]
            if len(color) != 3 or not all(0 <= channel <= 1 for channel in color):
                raise ValueError('color must be red, green and blue from 0 to 1')
        found = self.search(source, texts=texts, patterns=patterns, pages=pages,
                            case_sensitive=case_sensitive, whole_word=whole_word,
                            password=password)['files'][0]
        operations = []
        for match in found['matches']:
            if not match['rects']:
                raise ValueError(
                    f"page {match['page']}: {match['hit']!r} matched and has no position, "
                    'so it cannot be marked'
                )
            for area in match['rects']:
                operation = {'op': 'annotate', 'page': area['page'], 'kind': kind, 'rect': area['rect']}
                if color is not None:
                    operation['color'] = color
                operations.append(operation)
        if not operations:
            return None
        return self.edit(source, output, operations, force=force,
                         invalidate_signatures=invalidate_signatures, password=password)

    def verify(self, *paths: str | os.PathLike[str], strict: bool = False) -> reports.VerifyReport:
        """Inspect signatures. With strict=True, unsigned/untrusted files raise CommandError.

        The exception retains the full verification report and exit code. Use
        run('verify', '--strict', '--', *paths, check=False) for a Result instead.
        """
        args = ['--strict'] if strict else []
        return self.run('verify', *args, '--', *paths).typed

    def _pages(
        self, command: str, sources: Sequence[str | os.PathLike[str]],
        output: str | os.PathLike[str], options: Sequence[str], *,
        force: bool, invalidate_signatures: bool, password: str | None,
    ) -> reports.PagesReport:
        args = ['-o', os.fspath(output), *options]
        if force:
            args.append('--force')
        if invalidate_signatures:
            args.append('--invalidate-signatures')
        return self.run(command, *args, '--', *sources, password=password).typed

    def merge(
        self, sources: Sequence[str | os.PathLike[str]], output: str | os.PathLike[str], *,
        force: bool = False, invalidate_signatures: bool = False, password: str | None = None,
    ) -> reports.PagesReport:
        """Combine at least two documents in order. Only the first may be encrypted."""
        if isinstance(sources, (str, bytes, os.PathLike)):
            raise TypeError('sources must be a sequence of document paths, not one path')
        return self._pages('merge', sources, output, [], force=force,
            invalidate_signatures=invalidate_signatures, password=password)

    def extract(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        pages: str, force: bool = False, invalidate_signatures: bool = False,
        password: str | None = None,
    ) -> reports.PagesReport:
        """Copy a page range (e.g. '1-3,7'), in document order, each page once."""
        return self._pages('extract', [source], output, ['--pages', pages], force=force,
            invalidate_signatures=invalidate_signatures, password=password)

    def split(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        every: int = 1, force: bool = False, invalidate_signatures: bool = False,
        password: str | None = None,
    ) -> reports.PagesReport:
        """Write groups as output-1.pdf, output-2.pdf, etc.; report lists actual paths.

        A publication failure can leave some parts written. CommandError.typed
        retains those paths; do not interpret an exception as a rollback.
        """
        return self._pages('split', [source], output, ['--every', str(every)], force=force,
            invalidate_signatures=invalidate_signatures, password=password)

    def rotate(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        degrees: int, pages: str | None = None, force: bool = False,
        invalidate_signatures: bool = False, password: str | None = None,
    ) -> reports.PagesReport:
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
    ) -> reports.PagesReport:
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
        fill: str = 'black',
        password: str | None = None, check: bool = True,
    ) -> reports.RedactReport:
        """Remove text, regex matches or rectangles, using the CLI's verified writer.

        Regions are {'page': 1, 'rect': [x, y, width, height]} in display points
        from top-left, sent through stdin. pages limits searches only; regions
        name their own pages. Matching ignores case unless case_sensitive=True.
        A dry run (output optional) writes nothing and has verified=None.
        fill is the colour of the boxes drawn over what went: 'black', 'white'
        or 'red'. White boxes cannot be seen on white paper.

        A written but unverified copy raises CommandError with exit_code=1;
        its report and file remain available. check=False returns that report
        directly: inspect written, verified and reasons. Exit 0 alone does not
        prove a redaction: no matches also writes nothing with verified=None.
        """
        if fill not in ('black', 'white', 'red'):
            raise ValueError("fill must be 'black', 'white' or 'red'")
        args = []
        if output is not None:
            args += ['-o', os.fspath(output)]
        if fill != 'black':
            args += ['--fill', fill]
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
                        password=password, check=check).typed

    def identities(self) -> reports.IdentitiesReport:
        """List usable OS-held signing certificates and rejected ones with reasons.

        Enumeration does not sign. Pass a chosen certificate's id to sign(); the
        client never chooses the first available identity automatically.
        """
        return self.run('identities').typed

    def sign(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        identity: str, rect: Sequence[float] | None = None, page: int | None = None,
        anchor: str | None = None, size: Sequence[float] | None = None,
        offset: Sequence[float] | None = None, anchor_match: int | None = None,
        no_image: bool = False, image: str | os.PathLike[str] | None = None,
        lines: Sequence[str] | None = None,
        text: Sequence[str] | None = None, date_format: str | None = None,
        reason: str | None = None, location: str | None = None,
        contact: str | None = None,
        hide: Sequence[str] | None = None,
        timestamp: str | None = None, long_term: bool = False, force: bool = False,
    ) -> reports.SignReport:
        """Sign with an explicitly selected OS-held certificate; no key is exported.

        Use identities()['usable'] to select its SHA-256 id (or its sha1
        thumbprint, or exact subject).
        rect=[x,y,width,height] creates a visible signature, in display points
        from top-left; page defaults to 1. anchor with size=[width, height]
        creates one beside text on the page instead: its top-left corner is the
        text's, moved by offset=[dx, dy]; text found more than once is refused
        unless anchor_match says which, counted from 1. rect and anchor
        together are refused. Other appearance options require rect;
        reason, location and contact do not.
        lines selects any of 'label', 'name', 'date'; [] hides all three.
        Visible signatures use the saved image unless no_image=True. image names
        a PNG or JPEG file to draw instead, for this signature only: the saved
        image is neither read nor changed. image and no_image=True together are
        refused. A file that is missing or is not a usable image is a refusal,
        before any certificate or key is asked for.

        text gives the lines to draw instead of the standard three, one string
        a line: '{name}', '{date}', '{reason}' and '{location}' are filled in,
        and '{{' and '}}' are a brace each. text and lines together are refused,
        and so are text and hide. date_format writes the date with the tokens
        YYYY, MM, DD, HH, mm and ss; the time is UTC.

        reason, location and contact are written to the signature, visible or
        not. A visible signature draws the reason and location as lines of it;
        with text they are drawn only where the text asks for them. contact is
        never drawn. hide names which of 'reason', 'location' are written without being
        drawn; nothing is hidden unless it is named.

        timestamp selects a CLI authority name or URL and explicitly enables its
        network request. long_term requires timestamp and asks certificate
        authorities for revocation evidence. Neither is enabled by default.
        Encrypted inputs are not supported by signing. The OS may prompt for
        key access; configure the client's timeout for interactive use.
        """
        if not isinstance(identity, str) or not identity.strip():
            raise ValueError('identity must name an explicitly selected certificate')
        args = ['-o', os.fspath(output), '--identity', identity]
        if rect is not None:
            if isinstance(rect, (str, bytes)) or len(rect) != 4:
                raise ValueError('rect must contain four numbers: x, y, width, height')
            args += ['--visible', '--rect', ','.join(str(value) for value in rect)]
        if anchor is not None:
            if rect is not None:
                raise ValueError('rect says where the signature goes and anchor finds where; give one')
            if size is None or isinstance(size, (str, bytes)) or len(size) != 2:
                raise ValueError('anchor needs size: two numbers, width and height')
            args += ['--visible', '--anchor', anchor, '--size', ','.join(str(value) for value in size)]
            if offset is not None:
                if isinstance(offset, (str, bytes)) or len(offset) != 2:
                    raise ValueError('offset must contain two numbers: dx, dy')
                args += ['--offset', ','.join(str(value) for value in offset)]
            if anchor_match is not None:
                args += ['--anchor-match', str(anchor_match)]
        elif size is not None or offset is not None or anchor_match is not None:
            raise ValueError('size, offset and anchor_match describe a signature placed with anchor')
        if page is not None:
            args += ['--page', str(page)]
        if image is not None and no_image:
            raise ValueError('image names the image to draw and no_image draws none; give one')
        if no_image:
            args.append('--no-image')
        if image is not None:
            args += ['--image', os.fspath(image)]
        if lines is not None:
            if isinstance(lines, (str, bytes)):
                raise TypeError('lines must be a sequence of label, name, date')
            args += ['--lines', ','.join(lines)]
        if hide is not None:
            if isinstance(hide, (str, bytes)):
                raise TypeError('hide must be a sequence of reason, location')
            args += ['--hide', ','.join(hide)]
        if text is not None:
            if isinstance(text, (str, bytes)):
                raise TypeError('text must be a sequence of lines')
            for line in text:
                args += ['--text', line]
        if date_format is not None:
            args += ['--date-format', date_format]
        for flag, value in [('--reason', reason), ('--location', location), ('--contact', contact),
                            ('--timestamp', timestamp)]:
            if value is not None:
                args += [flag, value]
        if long_term:
            args.append('--long-term')
        if force:
            args.append('--force')
        return self.run('sign', *args, '--', source).typed

    def long_term(
        self, source: str | os.PathLike[str], output: str | os.PathLike[str], *,
        timestamp: str, force: bool = False,
    ) -> reports.LongTermReport:
        """Add long-term validation data to a document that is already signed.

        No identity and no key: the document was signed by somebody, with
        anything. The copy at output is source with the certificate
        authorities' answers about every signature's and timestamp's
        certificates, and an archive timestamp over the whole from timestamp,
        a CLI authority name or URL. timestamp is required and explicitly
        enables the network requests: to that authority, and to the revocation
        hosts the document's certificates name, which are asked only for
        certificates that chain to a root this computer trusts.

        Every signature in the document is covered or nothing is written. A
        refusal raises CommandError and names the signature and the reason: no
        signature, an encrypted document, a signature that does not verify, a
        certification with no changes permitted, a signer or authority this
        computer does not trust, or revocation data that cannot be had or says
        a certificate is revoked. report['covered'] lists the fields the data
        was added for and report['archive'] the new timestamp's field.

        Running it again on its own result adds a further archive timestamp.
        That works only while every certificate involved is still valid: once
        one has expired the run is refused.
        """
        if not isinstance(timestamp, str) or not timestamp.strip():
            raise ValueError('timestamp must name the timestamp authority to ask')
        args = ['-o', os.fspath(output), '--timestamp', timestamp]
        if force:
            args.append('--force')
        return self.run('long-term', *args, '--', source).typed
