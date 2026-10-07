# What people want from a PDF program

What users of Acrobat, Foxit, PDF24 and the other common PDF programs complain about and
ask for, collected on 2026-10-03 and extended on 2026-10-07, and what it means for tpdf. `docs/PLAN.md` §1 states the
problem tpdf was started for; this file is the outside check on it.

## How it was collected

Two passes, and they are not the same kind of evidence.

- **Vendor and project forums, review sites.** Adobe's community forum, the SumatraPDF,
  PDF-XChange and PDF24 forums, Capterra reviews, askwoody. Read through a web search;
  the order is a judgement of how often a complaint came up, not a count.
- **Reddit.** An independent search read 30 threads started between 2024 and 2026, in
  r/software, r/sysadmin, r/Adobe, r/Acrobat, r/pdf, r/windows, r/Windows11, r/macapps,
  r/opensource and r/FuckAdobe. The counts are threads out of those 30; a theme counts
  once per thread and the categories overlap. They describe that sample, not Reddit.

Limits worth keeping in mind when quoting any of this:

- The Reddit thread links were not opened a second time by the author; the quotes are
  the search's reading.
- Recommendation threads contain vendor accounts and promotional replies, so how often a
  product is recommended shows how visible it is, not how good.
- The Foxit evidence in the forum pass is thin and partly old (the toolbar bundling is
  from the installer era). The PDF24 points come from a handful of reviews.

## The complaints, ranked on Reddit

| # | Threads of 30 | Complaint or wish | tpdf on 2026-10-03 |
|---|---|---|---|
| 1 | 21 | Subscriptions, paywalls, watermarks on what a trial writes | Free, MIT, no watermark |
| 2 | 16 | Cluttered interfaces, AI banners, simple actions hard to find | Command palette; no AI |
| 3 | 15 | Distrust: files uploaded by online tools, a free tool whose funding is unclear, bundled software | Offline, open source |
| 4 | 14 | A file renders wrong or loses formatting after saving | Every write is checked against its input. Exception: deleting a page or merging drops the bookmarks |
| 5 | 13 | Freezes and crashes; Acrobat freezing 5 to 10 s on a five-page file | The first of tpdf's three properties |
| 6 | 12 | Filling forms, **creating** forms, signing; confusion between a drawn and a certificate signature | Filling, both kinds of signature, and creating fields; list boxes are the one kind missing |
| 7 | 10 | "Edit PDF" turns out to mean annotate; they want to change the existing text | Built, and it refuses what it cannot do faithfully |
| 7 | 10 | One program instead of five (Okular, PDF Arranger, Ghostscript, Xournal++ together) | Mostly; see the gaps |
| 9 | 7 | Licence activation and deployment; administrators want Group Policy and a central switch for AI | No licence. No managed-deployment settings |
| 10 | 6 | OCR that makes a scan searchable without a paid add-on | Built: `tpdf ocr`, *Recognise text* |

The forum pass agrees on price, clutter, privacy, speed and text editing, and adds four
that Reddit's ten do not carry:

- **Sign-in prompts and upsell popups** when all the reader wants is to open a file
  (Acrobat Reader; several threads running for years).
- **An AI button that cannot be removed**, even with generative AI switched off in the
  preferences; some companies treat it as a security risk (Acrobat).
- **Advertising, bundled toolbars, update nags** (Foxit).
- **Tabs that come back after a restart**, and the last page remembered per document
  (PDF-XChange, Zotero, Vivaldi forums). Built on 2026-10-03: *At launch: reopen all
  tabs*.

Two more came up rarely: the reader fighting over the default-application setting
(Acrobat, Edge), which tpdf only ever changes when asked, and comparing two versions of
a document.

## What nobody offers for free

The threads do not support "no free tool has X" for any single operation. The gaps are
whole workflows:

- **Editing existing text reliably** on complicated documents: fonts, spacing and the
  rest of the page kept. Free editors are suggested and then fail on the asker's file.
- **One free program for technical drawings**: OCR, merging, flattening, annotations,
  signatures and a calibrated area measurement on large construction drawings.
- **Print production**: colour separations, spot colours, ink control. Designers say
  nothing replaces Acrobat there.
- **A small Mac reader** with correct rendering, OCR and the system's Look Up and
  Translate.

## What gets recommended

Threads of 30 in which each was recommended, and the reason given:

| Program | Threads | Why |
|---|---|---|
| PDF-XChange Editor | 17 | Broad editing, fast, a one-time licence; the favourite of Windows administrators |
| Foxit | 16 | Familiar to Acrobat users; alongside complaints about price, administration and stability |
| PDFgear | 15 | Free and broad; others question how it is paid for |
| PDF24 | 11 | Free, local, good at merge, split and compress; the interface is tolerated |
| Okular | 11 | Open source, annotation and signing; wrongly recommended for editing text |
| LibreOffice Draw | 10 | Free changes to existing content on simple files; import fidelity is the caveat |
| Nitro | 9 | Capable and cheaper than Acrobat; activation trouble |
| SumatraPDF | 8 | Fast and small; recommended as the reader beside a separate editor |

On Windows the program to be measured against is PDF-XChange Editor, not Acrobat: it is
the one people already call fast.

## What tpdf lacks

In the order they should be taken:

1. **Keeping bookmarks through a page deletion and a merge.** Not a wish from the threads
   but a defect against complaint 4: deleting a page drops the document's bookmarks, and
   a merge carries over neither bookmarks nor named destinations nor form fields.
   Editing the outline is the step after it.
2. **Comparing two documents.** Moderate demand in the forum pass, none in the Reddit
   sample. The free tools that do it are websites, which collides with complaint 3, so
   an offline one is worth having. `tpdf compare a.pdf b.pdf` first, since it reuses the
   text extraction, then a view in the window.
3. **Split view and exporting annotations**, asked for by Mac readers.
4. **A calibrated measuring tool**, distance and area against a scale. A real need on
   technical drawings and a small audience. Moved down from third place on 2026-10-07:
   Open PDF Studio, an LGPL editor for construction drawings with measuring, reached 887
   stars in nine months, and large drawings are where tpdf is slowest.
5. **Settings an administrator can deploy.** Seven threads; nothing to do until the
   Windows installer is signed and someone deploys tpdf in an organisation.

Creating form fields led this list on 2026-10-03 and is built: text, multi-line,
checkbox, dropdown, radio button and signature fields. List boxes remain.

Left out on purpose: print production. It is a different product.

## What to say about tpdf

The top complaints are about behaviour, not missing features: price, nagging, clutter,
uploads, speed. tpdf's description should lead with those. Fifteen threads worry about
who pays for a free PDF tool and where the file goes, so the description should answer
both outright: nobody pays, it is MIT-licensed, and nothing leaves the machine except the
update check and a timestamp request when a signature asks for one.

## Second pass, 2026-10-07

A wider pass over the market: the competing programs, Hacker News and the issue trackers of
SumatraPDF, Stirling-PDF, sioyek, PDF Arranger and Zotero, the routes by which open-source
desktop programs found their first users, and the libraries and command-line tools that
scripts use. Reddit refused every request in this pass, so the sample above remains the only
Reddit evidence. Nobody from a law firm, an accounting office or an IT department was heard
in their own words.

**The finding.** Every job people name most often is already built: fill and sign a form,
annotate, reorder pages, compress, recognise text, redact. What limits adoption is that
nobody can find tpdf and that Windows warns against it. Distribution comes before features.

What was measured, each on its own page or API on 2026-10-07:

- **No comparable program exists.** No MIT-licensed native editor for both macOS and
  Windows with redaction and text editing has users. PDF4QT (MIT, 1,497 stars) has no
  macOS build; Stirling-PDF (93,695 stars) is a web application in a desktop wrapper with
  installers of 310 to 458 MB and proprietary parts.
- **Free text editing is taken.** PDFgear and PDF24 give away text editing, text
  recognition and conversion, both closed source. Acrobat sells redaction, text
  recognition and comparison only in Pro, at USD 239.88 a year.
- **Failed redaction is recurring news.** On Hacker News the Epstein files reached 1,029
  points (December 2025), a library that finds bad redactions 709, and a city's failed
  redaction 526 (2026-10-04). No permissively licensed tool was found that removes
  content and then checks the removal: qpdf closed its request, pypdf and pdfcpu have
  none, and the one checker depends on an AGPL library.
- **Scripts run on Linux.** 93.0% of pypdf's downloads come from Linux, 4.6% from Windows
  and 2.2% from macOS. PyMuPDF has 83 million downloads a month under the AGPL, and a
  GitHub search for "pymupdf AGPL license" returns 884 issues.
- **An unsigned Windows installer starts again with every release.** Microsoft: "Unsigned
  files must build reputation anew with every update", which "can take several weeks and
  hundreds of clean installs". Smart App Control blocks unsigned programs outright.
- **People praise behaviour.** In ten Hacker News threads with 1,514 comments, "nothing
  leaves my machine" appears in 116, speed in 105, signing in 75. The words used for a
  loved reader are instant, does not lock the file, remembers the place, reloads on change.
- **E-invoices.** Quba, the free viewer for ZUGFeRD and Factur-X, has 785,918 release
  downloads on 202 stars. An ordinary viewer shows the picture and not the XML that binds.

The order of work that follows from it:

1. The first screen of the README: which file to download, the Windows warning, who makes
   tpdf and what leaves the computer, a comparison. Done on 2026-10-07.
2. Fewer Windows releases until the installer is signed; `docs/DETAIL.md`, *Versioning*.
3. A code-signing certificate for Windows. Azure's service takes individuals only in the
   USA and Canada; Certum issues an open-source certificate to individuals. The Microsoft
   Store signs for free but takes MSIX, which Tauri does not build, and whether the worker
   runs inside an MSIX container is not known.
4. One announcement, led by redaction that is checked.
5. The command-line tool on Linux with a worker sandbox, then Python wheels that carry it.
6. A command that checks a document redacted in another program for content that is
   still there. `tpdf redact` checks its own output only.
7. The list under *What tpdf lacks*, in its order, and after it a view of the e-invoice
   embedded in a document.

Not worth building: conversion to Word, which the free closed programs give away;
anything with a language model in the window, which two of the nine most-voted requests
on Adobe's own board ask to have removed; creating e-invoices, converting to PDF/A and
repairing accessibility tags, each of which is a product of its own.

Open questions this pass could not answer: how many installs the Windows warning costs,
and whether removing whole text runs closes the leak a 2023 study found in 9 of 11
redaction tools, where the width of the removed text gives the words away
(<https://petsymposium.org/popets/2023/popets-2023-0069.php>).

## Sources

Reddit:

- <https://www.reddit.com/r/software/comments/1mvgky9/pdf_app_to_replace_adobe/>
- <https://www.reddit.com/r/software/comments/1o5labz/looking_for_a_free_pdf_editor_that_actually_works/>
- <https://www.reddit.com/r/Adobe/comments/1nholgp/a_way_to_turn_off_ai_suggestions/>
- <https://www.reddit.com/r/sysadmin/comments/1j6jibm/adobe_acrobat_alternatives/>
- <https://www.reddit.com/r/software/comments/1nbghqc/software_to_viewedit_pdf_files_besides_acrobat/>
- <https://www.reddit.com/r/pdf/comments/1ohnx5g/could_you_please_recommend_me_a_pdf_reader_and/>
- <https://www.reddit.com/r/macapps/comments/1b1ydhi/a_pdf_reader_alternative_to_preview/>
- <https://www.reddit.com/r/software/comments/1s9wsg1/what_is_the_best_pdf_editor/>
- <https://www.reddit.com/r/sysadmin/comments/1m7hpjw/at_my_breaking_point_with_adobe_acrobat_what_are/>
- <https://www.reddit.com/r/Adobe/comments/1rfbsn2/does_anyone_else_think_the_newest_adobe_acrobat/>
- <https://www.reddit.com/r/pdf/comments/1k8sgp7/adonde_acrobat_pro_alternative_for_editing_files/>
- <https://www.reddit.com/r/Windows11/comments/1pzvz92/whats_a_good_pdf_writerreader_thats_free_i_want/>
- <https://www.reddit.com/r/opensource/comments/1aw7409/windows_app_to_edit_text_in_pdf/>
- <https://www.reddit.com/r/software/comments/1saq9pp/looking_for_a_free_alternative_to_foxit_pdf_reader/>
- <https://www.reddit.com/r/opensource/comments/1bu1gdi/adobe_acrobat_foss_alternative_to_end_all/>
- <https://www.reddit.com/r/macapps/comments/1hqclcb/whats_your_favorite_pdf_reader_for_macos_looking/>
- <https://www.reddit.com/r/sysadmin/comments/1foeprp/alternative_to_adobe_and_foxit/>
- <https://www.reddit.com/r/software/comments/1ifhg9c/adobe_acrobat_pro_alternatives/>
- <https://www.reddit.com/r/opensource/comments/19b9kzd/does_anybody_know_the_best_open_source_free_pdf/>
- <https://www.reddit.com/r/FuckAdobe/comments/1vn0wk7/dear_graphic_designers_free_acrobat_alternatives/>
- <https://www.reddit.com/r/pdf/comments/1mqhk7u/adobe_alternatives/>
- <https://www.reddit.com/r/pdf/comments/1ugyrdh/whats_the_best_alternative_for_adobe_acrobat/>
- <https://www.reddit.com/r/sysadmin/comments/1qx5922/psa_foxit_working_well_for_us_to_replace_acrobat/>
- <https://www.reddit.com/r/windows/comments/1fdlr31/best_free_pdf_reader/>

Forums and reviews:

- <https://community.adobe.com/questions-12/dc-complaint-every-upgrade-is-a-downgrade-1515118>
- <https://community.adobe.com/t5/acrobat-reader-discussions/sign-in-prompt-when-opening-pdf-file/m-p/11668218>
- <https://community.adobe.com/t5/acrobat-discussions/remove-ai-assistant-button/m-p/14571826/highlight/true>
- <https://community.adobe.com/t5/acrobat-discussions/acrobat-becomes-very-slow-when-i-open-large-pdf-files-200-pages/td-p/15579541>
- <https://community.adobe.com/questions-9/adobe-freezes-while-scrolling-in-a-newly-opened-pdf-1636051>
- <https://community.adobe.com/t5/acrobat-discussions/issue-with-editing-text-in-adobe-acrobat-text-shifts-or-changes-font-automatically/td-p/15604009>
- <https://community.adobe.com/questions-91/default-viewer-1499905>
- <https://majeris.substack.com/p/screw-adobe-acrobat-tip-rant>
- <https://www.capterra.com/p/175376/Foxit-PDF-Reader/reviews/>
- <https://www.askwoody.com/forums/topic/new-foxit-reader-users-beware>
- <https://capterra.com/p/182070/PDF24-Creator/reviews/>
- <https://sumatrapdf.userjot.com/board/p/filling-of-pdf-forms>
- <https://sumatrapdf.userjot.com/board/p/better-annotation-support>
- <https://forum.pdf-xchange.com/viewtopic.php?p=30524>
- <https://forums.zotero.org/discussion/comment/380173>
- <https://forum.vivaldi.net/post/632082>

Second pass, 2026-10-07:

- <https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation>
- <https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options>
- <https://shop.certum.eu/open-source-code-signing-on-simplysign.html>
- <https://docs.brew.sh/Package-Acceptance-Policy>
- <https://www.adobe.com/acrobat/pricing.html>
- <https://www.sumatrapdfreader.org/download-free-pdf-viewer>
- <https://github.com/Stirling-Tools/Stirling-PDF/blob/main/LICENSE>
- <https://github.com/JakubMelka/PDF4QT>
- <https://github.com/OpenAEC-Foundation/open-pdf-studio>
- <https://pypistats.org/packages/pypdf>
- <https://pypistats.org/packages/pymupdf>
- <https://github.com/search?q=pymupdf+AGPL+license&type=issues>
- <https://github.com/qpdf/qpdf/issues/1139>
- <https://news.ycombinator.com/item?id=46368946>
- <https://news.ycombinator.com/item?id=46369923>
- <https://news.ycombinator.com/item?id=49957068>
- <https://news.ycombinator.com/item?id=37993575>
- <https://github.com/ZUGFeRD/quba-viewer/releases>
- <https://blog.kowalczyk.info/article/2f72237a4230410a888acbfce3dc0864/lessons-learned-from-15-years-of-sumatrapdf-an-open-source-windows-app.html>
