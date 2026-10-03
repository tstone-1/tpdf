# What people want from a PDF program

What users of Acrobat, Foxit, PDF24 and the other common PDF programs complain about and
ask for, collected on 2026-10-03, and what it means for tpdf. `docs/PLAN.md` §1 states the
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
| 4 | 14 | A file renders wrong or loses formatting after saving | Every write is checked against its input |
| 5 | 13 | Freezes and crashes; Acrobat freezing 5 to 10 s on a five-page file | The first of tpdf's three properties |
| 6 | 12 | Filling forms, **creating** forms, signing; confusion between a drawn and a certificate signature | Filling and both kinds of signature. **No way to add a form field** |
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

1. **Creating form fields.** Asked for in the same breath as text editing and OCR by
   people replacing Acrobat Pro. Begun on 2026-10-03: text fields and checkboxes can be
   placed in the window by a drag and added from the command line with `tpdf form`.
   Still to do, in this order: resizing a placed field, dropdowns, radio buttons and
   list boxes, then turned pages.
2. **Comparing two documents.** Moderate demand in the forum pass, none in the Reddit
   sample. The free tools that do it are websites, which collides with complaint 3, so
   an offline one is worth having. `tpdf compare a.pdf b.pdf` first, since it reuses the
   text extraction, then a view in the window.
3. **A calibrated measuring tool**, distance and area against a scale. A real need on
   technical drawings and a small audience.
4. **Split view and exporting annotations**, asked for by Mac readers.
5. **Settings an administrator can deploy.** Seven threads; nothing to do until someone
   deploys tpdf in an organisation.

Left out on purpose: print production. It is a different product.

## What to say about tpdf

The top complaints are about behaviour, not missing features: price, nagging, clutter,
uploads, speed. tpdf's description should lead with those. Fifteen threads worry about
who pays for a free PDF tool and where the file goes, so the description should answer
both outright: nobody pays, it is MIT-licensed, and nothing leaves the machine except the
update check and a timestamp request when a signature asks for one.

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
