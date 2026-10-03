"""Types for the JSON reports `tpdf-cli --json` prints, schema 1.

Each class describes one JSON object shape. They are `typing.TypedDict`
classes, so at run time a report is still the plain `dict` that `json.loads`
returned; the types are for a type checker and for the reader.

The shapes follow `src-tauri/src/cli/report.rs`. `test_reports.py` holds them
against the committed samples in `src-tauri/testdata/cli/` in both directions.

A JSON array is always a `list`, including the fixed-length ones (a rectangle
is a `list[float]` of four values). A key that may be left out of the JSON is
declared in a `total=False` class; every other key is always present, and a
value that may be `null` says so with `| None`.
"""

from typing import Literal, TypedDict

__all__ = [
    "REPORTS",
    "HitRect",
    "PadesLevel",
    "Appearance",
    "ChainCertificate",
    "ChainEnd",
    "ChainReport",
    "Comment",
    "CommentKind",
    "CommentLimits",
    "CommentsReport",
    "CommandError",
    "CommandErrorKind",
    "Conformance",
    "DescribedFile",
    "Document",
    "DocumentLimits",
    "DrawnLine",
    "EditPageSize",
    "EditReport",
    "Encryption",
    "ErrorReport",
    "Field",
    "FieldKind",
    "FieldOption",
    "FieldValue",
    "FieldsReport",
    "FileError",
    "FileErrorKind",
    "FillProblem",
    "FillProblemKind",
    "FillReport",
    "FilledField",
    "FontUsed",
    "Form",
    "HelpCommand",
    "HelpReport",
    "ImagePage",
    "ImagesReport",
    "IdentitiesReport",
    "InfoReport",
    "IntegrityReport",
    "IntegrityVerdict",
    "IntegrityWhy",
    "MetadataField",
    "NotEditable",
    "NotUsableIdentity",
    "OcrPage",
    "OcrReport",
    "PageOutput",
    "ProtectReport",
    "PageSize",
    "PageText",
    "PagesReport",
    "Permission",
    "RedactReport",
    "RedactSearch",
    "RedactedPage",
    "RenderReport",
    "RevocationBasis",
    "RevocationGap",
    "RevocationReason",
    "RevocationReport",
    "RevocationSource",
    "RevocationStatus",
    "SearchHit",
    "SearchKind",
    "SearchQuery",
    "SearchReport",
    "SearchedFile",
    "SignReport",
    "Signature",
    "TextEncoding",
    "TextOrder",
    "TextReport",
    "TextRun",
    "TextRunsReport",
    "TimestampReport",
    "TrustDoubt",
    "TrustReport",
    "TrustStanding",
    "TrustStore",
    "UsableIdentity",
    "VerifiedFile",
    "VerifyReport",
]


# --- Enumerations ----------------------------------------------------------

CommandErrorKind = Literal["usage", "refused", "failed"]

FileErrorKind = Literal["unreadable", "refused", "locked", "failed"]

IntegrityVerdict = Literal["unchecked", "broken", "altered", "weak", "intact"]

IntegrityWhy = Literal[
    "format",
    "range",
    "unreadable",
    "certificate",
    "algorithm",
    "attributes",
    "budget",
    "binding",
]

TrustStanding = Literal[
    "unchecked",
    "untrusted",
    "not_yet_valid",
    "expired",
    "trusted",
    "trusted_at_timestamp",
]

TrustStore = Literal["mac", "windows"]

TrustDoubt = Literal[
    "incomplete",
    "root",
    "dates",
    "purpose",
    "timestamping",
    "not_in_force",
    "rejected",
    "certificate",
    "unavailable",
]

RevocationStatus = Literal["none", "unchecked", "unknown", "revoked", "good"]

RevocationGap = Literal[
    "unreadable",
    "bound",
    "issuer",
    "signature",
    "unauthorised",
    "algorithm",
    "unsupported",
    "stale",
    "expired",
    "dates",
    "budget",
]

RevocationSource = Literal["ocsp", "crl"]

RevocationReason = Literal[
    "unspecified",
    "key_compromise",
    "ca_compromise",
    "affiliation_changed",
    "superseded",
    "cessation_of_operation",
    "certificate_hold",
    "remove_from_crl",
    "privilege_withdrawn",
    "aa_compromise",
]

RevocationBasis = Literal["now", "claimed", "stated", "attested"]

ChainEnd = Literal["root", "no_issuer", "loop"]

PadesLevel = Literal["B-B", "B-T", "B-LT", "B-LTA"]

TextOrder = Literal["tagged", "geometric", "none"]

TextEncoding = Literal["stated", "guessed", "unknown"]

FieldKind = Literal["text", "checkbox", "radio", "choice_combo", "choice_list", "other"]

NotEditable = Literal[
    "read_only",
    "password",
    "file_select",
    "comb",
    "rich_text",
    "hidden",
    "unsupported",
    "other",
]

FillProblemKind = Literal[
    "unknown",
    "ambiguous",
    "not_editable",
    "type",
    "option",
    "characters",
    "length",
    "line",
    "layout",
    "read_back",
]

SearchKind = Literal["text", "pattern"]

# The annotation subtypes, lowercased without separators.
CommentKind = Literal[
    "text",
    "freetext",
    "highlight",
    "underline",
    "squiggly",
    "strikeout",
    "square",
    "circle",
    "line",
    "polygon",
    "polyline",
    "ink",
    "stamp",
    "caret",
    "fileattachment",
    "sound",
    "redact",
]

# A form field's answer: a string for text, a boolean for a checkbox, an export
# value or None for a radio group or a single choice, a list of export values
# for a multiple-selection list; None for kind "other".
FieldValue = str | bool | list[str] | None


# --- Errors ----------------------------------------------------------------


class CommandError(TypedDict):
    """An error shared by command failures and partially published page operations."""

    kind: CommandErrorKind
    # The process exit status.
    exit_code: int
    # Human-readable explanation; branch on kind and exit_code instead.
    message: str


class ErrorReport(TypedDict):
    """A command that failed before it had a more detailed report."""

    schema: int
    # The requested command, including an unknown command on a usage error.
    command: str
    error: CommandError


class FileError(TypedDict):
    """Why one document could not be read."""

    kind: FileErrorKind
    # The sentence, as stderr prints it.
    message: str


# --- help ------------------------------------------------------------------


class HelpCommand(TypedDict):
    """One command in discovery output."""

    # Subcommand name.
    name: str
    # Arguments accepted by the command, as shown in help.
    usage: str
    # Human-readable description.
    summary: str


class HelpReport(TypedDict):
    """`help --json`: machine-readable command discovery."""

    schema: int
    # Always "help".
    command: str
    # Application version.
    version: str
    # Every registered command, in help order.
    commands: list[HelpCommand]


# --- render ----------------------------------------------------------------


class RenderReport(TypedDict):
    """`render --json`: one page rendered to PNG."""

    schema: int
    command: str
    input: str
    output: str
    # Counted from 1.
    page: int
    dpi: int
    width_px: int
    height_px: int


# --- ocr -------------------------------------------------------------------


class OcrPage(TypedDict):
    """One page given a text layer."""

    # Counted from 1.
    page: int
    words: int


class OcrReport(TypedDict):
    """`ocr --json`: a copy with a text layer on the pages that had no text."""

    schema: int
    command: str
    input: str
    output: str
    # The recogniser and the build it reported, such as "vision (26A428)".
    engine: str
    pages: list[OcrPage]
    # Selected pages that already had text and were left as they are.
    already_text: list[int]
    # Selected pages without text on which nothing was recognised.
    nothing_read: list[int]
    signatures_invalidated: int
    signatures_unknown: bool


class ImagePage(TypedDict):
    """One page made from one picture."""

    page: int
    source: str
    width_pt: float
    height_pt: float


class ImagesReport(TypedDict):
    """`images --json`: a document with one page for each picture."""

    schema: int
    command: str
    output: str
    pages: list[ImagePage]


class ProtectReport(TypedDict):
    """`protect --json` and `unprotect --json`: a copy with a password set or removed."""

    schema: int
    command: str
    input: str
    output: str
    pages: int
    # Whether the copy needs a password to open.
    protected: bool
    # Whether the source was encrypted.
    was_protected: bool
    signatures_invalidated: int
    signatures_unknown: bool


# --- edit ------------------------------------------------------------------


class EditPageSize(TypedDict):
    """One page's geometry in PDF points."""

    width_pt: float
    height_pt: float


class FontUsed(TypedDict):
    """What a `replace_text` operation's font came to on this document."""

    # The operation's place in the plan, counted from 1.
    operation: int
    # The editor's name for the font, with its reason when it is a fallback.
    font: str


class EditReport(TypedDict):
    """`edit --json`: an edit plan, validated without writing or published."""

    schema: int
    command: str
    input: str
    output: str
    written: bool
    operations: int
    pages: list[EditPageSize]
    # Newly added annotations remaining after all operations, including undo.
    annotations: int
    signatures_invalidated: int
    signatures_unknown: bool
    # The font each `replace_text` that named one was set in, in plan order.
    fonts: list[FontUsed]


# --- comments --------------------------------------------------------------


class Comment(TypedDict):
    """One original annotation."""

    # Assigned in document order; stable for one scan.
    id: int
    # Counted from 1.
    page: int
    kind: CommentKind
    # /T, the author; empty when the document names none.
    author: str
    # /Contents, flattened; empty for a mark made without typing anything.
    body: str
    # /Subj.
    subject: str
    # /M as "YYYY-MM-DD HH:MM"; None when missing or not a date.
    date: str | None
    # /Rect in display space: four values from the displayed page's top-left.
    rect: list[float]
    # The words covered, four values per rectangle, in the same space.
    quads: list[float]
    # The comment this one replies to, by id.
    reply_to: int | None
    # /F bit 2.
    hidden: bool
    # /C as RGB in 0..1; None when absent or unreadable.
    color: list[float] | None
    # [number, generation] of the annotation's object; None for a direct
    # dictionary, which cannot be edited in place.
    object: list[int] | None


class CommentLimits(TypedDict):
    """What the comment scan's bounds cut off."""

    # Pages whose comments were cut at the per-page bound.
    crowded_pages: int
    # The total bound was reached, so later pages were not read.
    over_budget: bool
    # Bodies shortened or declined.
    bodies_clipped: int
    # Annotations with an unknown /Subtype, left out.
    unknown_kinds: int
    # /Annots entries that could not be read at all.
    unreadable: int
    # Reply links dropped because following them would have looped.
    cycles: int
    # Pages that could not be accounted for.
    pages_missed: int


class CommentsReport(TypedDict):
    """`comments --json`: the document's original annotations."""

    schema: int
    command: str
    input: str
    complete: bool
    comments: list[Comment]
    limits: CommentLimits


# --- text-runs -------------------------------------------------------------


class _TextRunRequired(TypedDict):
    operator: int
    text: str
    font: str
    size: float
    # Text matrix (six values) in the page's original user space.
    matrix: list[float]
    advance: float
    # Hit rectangle (four values) in the original displayed page.
    display_rect: list[float]


class TextRun(_TextRunRequired, total=False):
    """One editable run of text."""

    # Required single-line box height in points; left out unless deeper
    # than 1.25 em.
    minimum_height: float


class TextRunsReport(TypedDict):
    """`text-runs --json`: editable text and the revision to return with a replacement."""

    schema: int
    command: str
    input: str
    # Counted from 1.
    page: int
    # Bytes, each 0..255.
    revision: list[int]
    runs: list[TextRun]


# --- pages: merge, extract, split, rotate, crop ------------------------------


class PageOutput(TypedDict):
    """One completed output file of a page operation."""

    # Destination as derived from the command line.
    path: str
    # Number of pages checked before publication.
    pages: int


class PagesReport(TypedDict):
    """A merge, extract, split, rotation or crop, including partial publication."""

    schema: int
    # The requested operation.
    command: str
    # Source paths, in command-line order.
    inputs: list[str]
    # True only when all requested files were published.
    complete: bool
    # Files actually published, in order; empty on a pre-publication failure.
    outputs: list[PageOutput]
    # Number of source signatures affected by this rewrite.
    signatures_invalidated: int
    # Signature enumeration was incomplete in at least one input.
    signatures_unknown: bool
    # None on success; otherwise the failure after partial publication.
    error: CommandError | None


# --- identities ------------------------------------------------------------


class UsableIdentity(TypedDict):
    """A certificate with a key that may sign now."""

    # SHA-256 of the certificate, lowercase hex: what `--identity` accepts.
    id: str
    # SHA-1 of the certificate, lowercase hex; also accepted by `--identity`.
    sha1: str
    # The subject's common name, or its whole name.
    subject: str
    # The issuer's common name, or its whole name.
    issuer: str
    # "YYYY-MM-DD HH:MM:SS UTC".
    expires: str
    # "RSA 3072", "ECDSA P-256".
    method: str


class NotUsableIdentity(TypedDict):
    """A certificate with a key that may not sign, and why."""

    id: str
    sha1: str
    # Who it names, as far as it could be read.
    subject: str
    # Why it is not offered, as a clause.
    why: str


class IdentitiesReport(TypedDict):
    """`identities --json`."""

    schema: int
    command: str
    usable: list[UsableIdentity]
    not_usable: list[NotUsableIdentity]


# --- signatures: verify, info, sign ----------------------------------------


class IntegrityReport(TypedDict):
    """Whether a signature still covers the bytes it was made over."""

    verdict: IntegrityVerdict
    # Why nothing was concluded, for "unchecked"; None otherwise.
    why: IntegrityWhy | None
    # "SHA-256", or empty.
    digest: str
    # "RSA", "RSA-PSS", "ECDSA P-256", or empty.
    method: str
    # The properties dialog's Integrity row, word for word.
    sentence: str


class TrustReport(TypedDict):
    """Whether the OS trust store vouches for a certificate."""

    standing: TrustStanding
    # Why not trusted, for "untrusted" and "unchecked"; None otherwise.
    why: TrustDoubt | None
    # Whose store answered; None when none did.
    store: TrustStore | None
    # The attested moment the chain was judged at; empty when judged now.
    attested_at: str
    # The properties dialog's Trust row, word for word.
    sentence: str


class RevocationReport(TypedDict):
    """What the document's own revocation data says about one certificate."""

    # "none" means the document carries nothing about it: not checked.
    standing: RevocationStatus
    # Why nothing was concluded, for "unchecked"; None otherwise.
    why: RevocationGap | None
    # Which data answered, for "good", "revoked" and "unknown"; None otherwise.
    source: RevocationSource | None
    # When that data was issued, or empty.
    issued: str
    # When its issuer promised the next, or empty.
    next: str
    # When the certificate was revoked, for "revoked"; empty otherwise.
    revoked: str
    # The reason a revocation states, or None.
    reason: RevocationReason | None
    # Whose clock `moment` is.
    basis: RevocationBasis
    # The moment judged, "YYYY-MM-DD HH:MM:SS UTC".
    moment: str
    # Revoked after an attested moment, which does not undo the signature.
    after_moment: bool
    # The properties dialog's Revocation row, word for word.
    sentence: str


class ChainCertificate(TypedDict):
    """One certificate on a chain."""

    # Whom it names.
    subject: str
    # Its serial number, uppercase hex.
    serial: str
    revocation: RevocationReport


class ChainReport(TypedDict):
    """What the revocation data says about every certificate up to the root."""

    standing: RevocationStatus
    # Revoked after an attested moment, which does not undo the signature.
    after_moment: bool
    # Index in `certificates` of the one that decides `standing`, or None.
    decided_by: int | None
    # Certificates on the chain past the ones judged.
    dropped: int
    # How the chain ended.
    end: ChainEnd
    # Every certificate judged, the leaf first.
    certificates: list[ChainCertificate]
    # The properties dialog's Chain revocation row, word for word.
    sentence: str


class TimestampReport(TypedDict):
    """The RFC 3161 timestamp a signature carries, or that a document timestamp is."""

    # The time the token states; attested only when `attested` is true.
    time: str
    # The authority, from its certificate, or empty.
    authority: str
    # Whether the token checks out and covers what it is attached to.
    attested: bool
    integrity: IntegrityReport
    # None unless `attested`.
    trust: TrustReport | None
    # None exactly when `trust` is.
    revocation: RevocationReport | None
    # None exactly when `revocation` is.
    revocation_chain: ChainReport | None


class Signature(TypedDict):
    """One signature, as the properties dialog reports it."""

    # True for a document timestamp, including an archive timestamp.
    document_timestamp: bool
    # The field's fully qualified name.
    field: str
    # Whom the signing certificate names, or empty.
    signer: str
    # Who issued that certificate, or empty.
    issuer: str
    # /M, the time the signer's own machine claimed. Not checked.
    claimed_time: str
    # Whether the signed range reaches the file's last byte.
    covers_whole_file: bool
    # Bytes written after the signed range ends.
    appended_bytes: int
    integrity: IntegrityReport
    # None unless integrity.verdict is "intact" or "weak".
    trust: TrustReport | None
    # None for a signature with no timestamp.
    timestamp: TimestampReport | None
    # None exactly when `trust` is.
    revocation: RevocationReport | None
    # None exactly when `revocation` is.
    revocation_chain: ChainReport | None
    # The PAdES level the signature has the parts of; None for a document
    # timestamp, a signature that is not CAdES, and one that does not hold.
    # Read from the parts present; conformance to the standard is not tested.
    pades_level: PadesLevel | None
    # That level as a sentence; None when the level is.
    pades: str | None


class VerifiedFile(TypedDict):
    """One document `verify` was given."""

    # The path as given on the command line.
    path: str
    # Why the signatures could not be read; None when they were.
    error: FileError | None
    # Every signed signature field, in the document's order.
    signatures: list[Signature]


class VerifyReport(TypedDict):
    """`verify --json`."""

    schema: int
    command: str
    # Whether `--strict` would pass; present whether or not it was given.
    strict_passed: bool
    # One entry per document, in the order given.
    files: list[VerifiedFile]


class DrawnLine(TypedDict):
    """One line of a visible signature."""

    # The words drawn.
    text: str
    # [x, y, w, h]: the box no ink of the line leaves.
    rect: list[float]
    # The baseline, measured down from the top of the page.
    baseline: float


class Appearance(TypedDict):
    """Where a visible signature was drawn, in points from the page's top-left."""

    # Counted from 1.
    page: int
    # [x, y, w, h], as `--rect` gave it.
    rect: list[float]
    # [x, y, w, h] of the image, or None when none was drawn.
    image: list[float] | None
    # Type size of the lines in points; 0 when no line was drawn.
    font_size: float
    # The lines, top to bottom.
    lines: list[DrawnLine]


class SignReport(TypedDict):
    """`sign --json`."""

    schema: int
    command: str
    # The document signed, as given.
    input: str
    # The signed copy, as given.
    output: str
    # The new signature's field.
    field: str
    # The certificate it was signed with.
    identity: UsableIdentity
    # Whether it has an appearance on a page.
    visible: bool
    # None for an invisible signature.
    appearance: Appearance | None
    # Every signature in the written file, read back after writing.
    signatures: list[Signature]
    # The signing panel's closing sentence, word for word.
    summary: str


# --- info ------------------------------------------------------------------


class PageSize(TypedDict):
    """Pages of one displayed size."""

    # Points, to two decimals.
    width_pt: float
    height_pt: float
    # How many pages have it.
    count: int


class MetadataField(TypedDict):
    """One entry of the /Info dictionary."""

    # The key, without its slash.
    name: str
    # The value, decoded; the two date keys are reformatted.
    value: str
    # Whether PDF defines this key, rather than the document.
    standard: bool


class Permission(TypedDict):
    """One thing a document's encryption permits or forbids."""

    what: str
    allowed: bool


class Encryption(TypedDict):
    """The document's encryption, read from the trailer."""

    # "RC4 40-bit", "AES-256".
    method: str
    # The standard security handler's revision, /R.
    revision: int
    # Whether the empty user password opened it.
    opened_without_password: bool
    # What it permits, in a fixed order.
    permissions: list[Permission]


class Conformance(TypedDict):
    """A conformance claim. Claimed, never checked."""

    # "PDF/A-3B", "PDF/UA-1", sorted.
    claimed: list[str]
    # The XMP packet was there and could not all be read.
    unread: bool


class Form(TypedDict):
    """The document's interactive form."""

    # Whether the form could be read; False with `why` when it was refused.
    readable: bool
    # Fields with at least one widget on a page; 0 when there is no form.
    fields: int
    # Widgets on pages; a field may have several.
    widgets: int
    # Whether the form is XFA, which is not read.
    xfa: bool
    # Why the form could not be read; None when it could.
    why: str | None


class DocumentLimits(TypedDict):
    """What `info` could not read, so nothing is silently partial."""

    # Encrypted, and an empty user password did not open it.
    locked: bool
    # /Info entries dropped at the count bound.
    fields_dropped: int
    # Values shortened at the length bound.
    values_clipped: int
    # Timestamp tokens present on a signature and not readable.
    timestamps_unread: int
    # Signature fields not walked.
    signatures_dropped: int
    # /Fields entries that resolved to nothing usable.
    unreadable: int
    # Signatures carrying a certificate that could not be read.
    certificates_unread: int
    # Revocation data present and not readable.
    revocation_unread: int
    # Revocation data not read at a count bound.
    revocation_dropped: int


class Document(TypedDict):
    """What the properties dialog shows, as data."""

    # "1.7": the header's, or the catalog's /Version where that is later.
    version: str
    # The file's length.
    bytes: int
    pages: int
    # Each distinct displayed page size, in the order it first occurs.
    page_sizes: list[PageSize]
    # One, plus one per incremental update.
    revisions: int
    # The /Info dictionary: PDF's own keys first, then the document's.
    metadata: list[MetadataField]
    # /Lang, or empty.
    language: str
    # None for a document that is not encrypted.
    encryption: Encryption | None
    # Whether it has a structure tree; None when that could not be asked.
    tagged: bool | None
    # What its XMP metadata claims; None when it carries none.
    conformance: Conformance | None
    # Embedded files; None when they could not be counted.
    attachments: int | None
    form: Form
    # Every signed signature field: the objects `verify` prints.
    signatures: list[Signature]
    # Signature fields waiting for a signature.
    unsigned_signature_fields: int
    limits: DocumentLimits


class DescribedFile(TypedDict):
    """One document `info` was given."""

    # The path as given on the command line.
    path: str
    # Why it could not be described; None when it was. "locked" is reported
    # here and `info` still exits 0 for it.
    error: FileError | None
    # None exactly when `error` is not.
    document: Document | None


class InfoReport(TypedDict):
    """`info --json`."""

    schema: int
    command: str
    # One entry per document, in the order given.
    files: list[DescribedFile]


# --- text ------------------------------------------------------------------


class PageText(TypedDict):
    """One page's text."""

    # Counted from 1.
    page: int
    # How the order was decided.
    order: TextOrder
    # Whether the document says what its characters mean.
    encoding: TextEncoding
    # The page's lines in reading order, joined by "\n".
    text: str


class TextReport(TypedDict):
    """`text --json`."""

    schema: int
    command: str
    # The document, as given.
    path: str
    # The pages asked for, in document order.
    pages: list[PageText]


# --- fields and fill -------------------------------------------------------


class FieldOption(TypedDict):
    """One option of a choice, or one state of a radio group."""

    # What `fill` takes and the document stores.
    export: str
    # What is shown.
    label: str
    # Whether it is chosen now.
    selected: bool


class Field(TypedDict):
    """One form field, as `fill` addresses it."""

    # The fully qualified name: the key `fill` takes.
    name: str
    kind: FieldKind
    # Its current answer, in the form `fill` takes one.
    value: FieldValue
    # A radio group's states or a choice's options; empty for other kinds.
    options: list[FieldOption]
    # A list that allows several selections.
    multiple: bool
    # A dropdown that also takes text of its own.
    custom_text: bool
    # A text field that takes several lines.
    multiline: bool
    # /MaxLen in characters; None when there is none.
    max_length: int | None
    # The pages its widgets are on, counted from 1, each once.
    pages: list[int]
    # How many widgets it has.
    widgets: int
    # Whether `fill` can answer it.
    editable: bool
    # Why not; None when it can.
    not_editable: NotEditable | None
    # The sentence for `not_editable`; None when it is editable.
    why: str | None


class FieldsReport(TypedDict):
    """`fields --json`."""

    schema: int
    command: str
    # The document, as given.
    path: str
    # Every field with a widget on a page, in the order its first widget is met.
    fields: list[Field]


class FillProblem(TypedDict):
    """One answer `fill` refused, or one that did not read back."""

    # The name as the answers give it.
    field: str
    # Which rule.
    problem: FillProblemKind
    # The sentence, as stderr prints it.
    why: str


class FilledField(TypedDict):
    """One answered field, read back from the written file."""

    name: str
    kind: FieldKind
    value: FieldValue


class FillReport(TypedDict):
    """`fill --json`, written or refused."""

    schema: int
    command: str
    # The document filled, as given. It is never written.
    input: str
    # The filled copy, as given.
    output: str
    # Whether the filled copy was written and kept.
    written: bool
    # Every reason it was not; empty when `written`.
    problems: list[FillProblem]
    # Every field answered, read back; empty unless `written`.
    fields: list[FilledField]


# --- redact ----------------------------------------------------------------


class RedactSearch(TypedDict):
    """One `--text` or `--pattern` of a redaction, and what it found."""

    kind: SearchKind
    # The query, as given.
    query: str
    # How many times it matched.
    matches: int


class RedactedPage(TypedDict):
    """One page of a redaction."""

    # Counted from 1.
    page: int
    # Each match that starts on this page, as the page spells it.
    hits: list[str]
    # Regions marked on it.
    regions: int
    # Text-showing operations in the page's content that go.
    text_removals: int
    # Text-showing operations inside forms the page draws that go.
    form_text_removals: int
    # Image draws that go.
    image_removals: int
    # Drawings that go: paths the region holds all of.
    path_removals: int
    # Drawings cut at a region's edge: a straight rule or a rectangle the
    # region crosses, which keeps the part outside it.
    path_cuts: int
    # What the removed operations draw, one string per region that took any.
    taking: list[str]
    # What the removal cannot take, one sentence each.
    left: list[str]


class RedactReport(TypedDict):
    """`redact --json`, for a written copy and for a dry run."""

    schema: int
    command: str
    # The document, as given. It is never written.
    input: str
    # The redacted copy, as given; None for a dry run given no `-o`.
    output: str | None
    # `--dry-run`: nothing was written by this run.
    dry_run: bool
    # Whether the redacted copy was written and kept.
    written: bool
    # True only when every check proved the copy clean; False with every
    # reason in `reasons`; None when nothing was written.
    verified: bool | None
    # Why the copy could not be proved clean, or for a dry run what the
    # removal will not be able to take. Empty when `verified` is True.
    reasons: list[str]
    # What a clean verdict covers less of than usual, one sentence each.
    notes: list[str]
    # The sentence shown after a redaction; None unless written.
    summary: str | None
    # How many regions were marked.
    regions: int
    # How many distinct removals they take.
    removals: int
    # Signatures the rewrite invalidates.
    signatures_invalidated: int
    # Each `--text` and `--pattern`, in the order given.
    searches: list[RedactSearch]
    # Every page that has a match or a region, in page order.
    pages: list[RedactedPage]


# --- search ----------------------------------------------------------------


class SearchQuery(TypedDict):
    """One `--text` or `--pattern` of a search."""

    kind: SearchKind
    # The query, as given.
    query: str


class HitRect(TypedDict):
    """One rectangle of a match, as edit's annotate and redact's regions take it."""

    # The page, counted from 1.
    page: int
    # [x, y, width, height] in points from the page's top-left corner.
    rect: list[float]


class SearchHit(TypedDict):
    """One match."""

    # The page it starts on, counted from 1.
    page: int
    # The page it ends on when it runs over a page break; else None.
    end_page: int | None
    # Which of the report's `queries` found it, counted from 0.
    query: int
    # The text before it, whitespace collapsed.
    before: str
    # The matched text, as the page spells it.
    hit: str
    # The text after it, whitespace collapsed.
    after: str
    # Where it is: one rectangle per run of text on a line. Empty when its
    # characters have no position; the match is real and cannot be marked.
    rects: list[HitRect]


class SearchedFile(TypedDict):
    """One document `search` was given."""

    # The path as given on the command line.
    path: str
    # Why it could not be searched; None when it was.
    error: FileError | None
    # How many pages were searched.
    pages_searched: int
    # Pages, counted from 1, that held no text at all.
    pages_without_text: list[int]
    # Every match, by page and then by where on the page it starts.
    matches: list[SearchHit]


class SearchReport(TypedDict):
    """`search --json`."""

    schema: int
    command: str
    # Every `--text`, then every `--pattern`; a hit's `query` indexes this.
    queries: list[SearchQuery]
    # The documents, in the order given.
    files: list[SearchedFile]


# --- Sample file stem -> the report it holds -------------------------------

# Keyed by the stems of `src-tauri/testdata/cli/*.json`. `help --json` has no
# sample there, so `HelpReport` is not listed.
REPORTS: dict[str, type] = {
    "command-error": ErrorReport,
    "comments": CommentsReport,
    "edit": EditReport,
    "fields": FieldsReport,
    "fill": FillReport,
    "fill-refused": FillReport,
    "identities": IdentitiesReport,
    "images": ImagesReport,
    "info": InfoReport,
    "ocr": OcrReport,
    "pages": PagesReport,
    "protect": ProtectReport,
    "redact": RedactReport,
    "redact-dry-run": RedactReport,
    "render": RenderReport,
    "search": SearchReport,
    "sign": SignReport,
    "text": TextReport,
    "text-runs": TextRunsReport,
    "verify": VerifyReport,
}
