//! The files a document carries, listed and taken out (*Save an attachment*).
//!
//! The document-level list, `/Names /EmbeddedFiles` (PDF 32000-1 §7.7.4), a
//! name tree whose values are file specifications with the bytes in `/EF`
//! (§7.11.4). A `/FileAttachment` annotation is a comment and is listed in the
//! comments panel, which is why `docinfo` counts only this list.
//!
//! Read in the worker, as every parse is. **tpdf never opens an embedded
//! file**: the bytes are decoded within [`MAX_DECODE`] and written where the
//! reader chose, and the name says what kind of file it claims to be, which
//! nothing here checks.

use std::collections::HashSet;

use lopdf::{Document, Object, ObjectId};
use serde::{Deserialize, Serialize};

use crate::annots::decode_text_string;
use crate::encoding::{resolve, MAX_DECODE};

/// Most files listed. A real document carries a handful; a name tree that
/// claims more is listed this far and no further.
pub const MAX_FILES: usize = 1000;

/// Deepest name tree walked. A tree branches past several hundred entries, so
/// a real one is two or three levels deep.
const MAX_DEPTH: usize = 32;

/// Longest name kept, in characters.
const MAX_NAME_CHARS: usize = 255;

/// One embedded file as the properties dialog lists it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Embedded {
    /// The file's name, without any folder the document puts in front of it.
    pub name: String,
    /// The document's description of it, or empty.
    pub description: String,
    /// The size the document states for it (`/Params /Size`), if it states one.
    /// What the bytes decode to is only known when they are saved.
    pub size: Option<u64>,
}

/// Every embedded file, in the name tree's order, at most [`MAX_FILES`].
#[must_use]
pub fn list(document: &Document) -> Vec<Embedded> {
    entries(document)
        .into_iter()
        .map(|(key, spec)| describe(document, &key, spec))
        .collect()
}

/// The bytes of the embedded file at `index` in [`list`]'s order, decoded.
///
/// # Errors
///
/// No file at that place, a file specification with no bytes, or bytes that
/// do not decode within [`MAX_DECODE`].
pub fn content(document: &Document, index: usize) -> Result<Vec<u8>, String> {
    let found = entries(document);
    let Some((key, spec)) = found.get(index) else {
        return Err(said!(
            "The document has no attachment {number} (it has {count}).",
            number = index + 1,
            count = found.len()
        ));
    };
    let name = describe(document, key, *spec).name;
    let stream = spec
        .and_then(|spec| resolve(document, spec).as_dict().ok())
        .and_then(|spec| spec.get(b"EF").ok())
        .and_then(|ef| resolve(document, ef).as_dict().ok())
        .and_then(|ef| ef.get(b"UF").or_else(|_| ef.get(b"F")).ok())
        .and_then(|file| resolve(document, file).as_stream().ok())
        .ok_or_else(|| said!("{name} holds no file to save."))?;
    if stream.dict.has(b"Filter") {
        stream
            .decompressed_content_with_limit(MAX_DECODE)
            .map_err(|why| {
                said!(
                    "{name} could not be decoded within {mib} MiB: {why}",
                    mib = MAX_DECODE >> 20
                )
            })
    } else {
        Ok(stream.content.clone())
    }
}

/// Saves the embedded file at `index` of the document at `source` to `dest`,
/// and answers how many bytes it holds.
///
/// `outside` decodes it, in a worker wherever there is one, into a file beside
/// `dest`; that file's size is held to the count the worker gives, then it is
/// renamed over `dest`, so a save that fails leaves whatever was there. A
/// destination that is the document itself is refused: the rename would put an
/// attachment where the open document is.
///
/// # Errors
///
/// The destination, the source, what `outside` refuses, or the write.
pub fn save(
    source: &std::path::Path,
    index: usize,
    dest: &std::path::Path,
    password: Option<&str>,
    outside: &dyn crate::save::Rewriter,
) -> Result<usize, String> {
    let same = |a: &std::path::Path, b: &std::path::Path| match (a.canonicalize(), b.canonicalize())
    {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same(source, dest) {
        return Err(said!(
            "The attachment was not saved: that is the document it comes from."
        ));
    }
    let name = dest.file_name().ok_or_else(|| {
        said!(
            "The attachment was not saved: {path} names no file.",
            path = dest.display()
        )
    })?;
    let failed = |why: String| {
        said!(
            "The attachment was not saved to {path}: {why}",
            path = dest.display()
        )
    };
    let (staged, mut out) =
        crate::save::claim_beside(dest, name).map_err(|e| failed(e.to_string()))?;
    let written = (|| -> Result<usize, String> {
        let mut from =
            std::fs::File::open(source).map_err(|e| format!("{}: {e}", source.display()))?;
        let len = usize::try_from(from.metadata().map_err(|e| e.to_string())?.len())
            .map_err(|_| said!("the document is too large to read"))?;
        let count = outside
            .attachment(&mut from, len, &mut out, index, password)
            .map_err(|why| why.message)?;
        let held = out.metadata().map_err(|e| e.to_string())?.len();
        if held != count as u64 {
            return Err(said!(
                "the worker said {count} bytes and the file holds {held}"
            ));
        }
        out.sync_all().map_err(|e| e.to_string())?;
        drop(out);
        std::fs::rename(&staged, dest).map_err(|e| e.to_string())?;
        Ok(count)
    })();
    written.map_err(|why| {
        let _ = std::fs::remove_file(&staged);
        failed(why)
    })
}

/// The name tree's leaves in order: each key and its value, the value
/// unresolved.
fn entries(document: &Document) -> Vec<(Vec<u8>, Option<&Object>)> {
    let mut out = Vec::new();
    let root = document
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"Names").ok())
        .and_then(|names| resolve(document, names).as_dict().ok())
        .and_then(|names| names.get(b"EmbeddedFiles").ok());
    if let Some(root) = root {
        let mut seen = HashSet::new();
        walk(document, root, 0, &mut seen, &mut out);
    }
    out
}

/// One node of the name tree: its `/Names` pairs, then its `/Kids`. Bounded
/// in depth and in count, and a node reached twice is walked once, so a tree
/// whose kids name their parent ends.
fn walk<'a>(
    document: &'a Document,
    node: &'a Object,
    depth: usize,
    seen: &mut HashSet<ObjectId>,
    out: &mut Vec<(Vec<u8>, Option<&'a Object>)>,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    if let Object::Reference(id) = node {
        if !seen.insert(*id) {
            return;
        }
    }
    let Ok(dict) = resolve(document, node).as_dict() else {
        return;
    };
    if let Some(pairs) = dict
        .get(b"Names")
        .ok()
        .and_then(|names| resolve(document, names).as_array().ok())
    {
        for pair in pairs.chunks(2) {
            if out.len() >= MAX_FILES {
                return;
            }
            let key = match pair.first().map(|key| resolve(document, key)) {
                Some(Object::String(bytes, _)) => bytes.clone(),
                _ => Vec::new(),
            };
            out.push((key, pair.get(1)));
        }
    }
    if let Some(kids) = dict
        .get(b"Kids")
        .ok()
        .and_then(|kids| resolve(document, kids).as_array().ok())
    {
        for kid in kids {
            walk(document, kid, depth + 1, seen, out);
        }
    }
}

/// What the list says of one entry: `/UF`, then `/F`, then the tree's key for
/// the name.
fn describe(document: &Document, key: &[u8], spec: Option<&Object>) -> Embedded {
    let spec = spec.and_then(|spec| resolve(document, spec).as_dict().ok());
    let text = |name: &[u8]| -> Option<String> {
        match spec?.get(name).ok().map(|value| resolve(document, value)) {
            Some(Object::String(bytes, _)) => Some(decode_text_string(bytes)),
            _ => None,
        }
    };
    let raw = text(b"UF")
        .or_else(|| text(b"F"))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| decode_text_string(key));
    let size = spec
        .and_then(|spec| spec.get(b"EF").ok())
        .and_then(|ef| resolve(document, ef).as_dict().ok())
        .and_then(|ef| ef.get(b"UF").or_else(|_| ef.get(b"F")).ok())
        .and_then(|file| resolve(document, file).as_stream().ok())
        .and_then(|file| file.dict.get(b"Params").ok())
        .and_then(|params| resolve(document, params).as_dict().ok())
        .and_then(|params| params.get(b"Size").ok())
        .and_then(|size| resolve(document, size).as_i64().ok())
        .and_then(|size| u64::try_from(size).ok());
    Embedded {
        name: file_name(&raw),
        description: text(b"Desc").unwrap_or_default(),
        size,
    }
}

/// An invisible character that changes what a name shows: the direction marks
/// and overrides, by which `fdp.exe` reads as `exe.pdf`, and the zero-width ones.
fn reorders(c: char) -> bool {
    matches!(
        c,
        '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}'
            | '\u{feff}'
    )
}

/// The last part of a name a document wrote with folders in front of it, with
/// no character a file system refuses: what the save panel offers.
fn file_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\', ':']).next().unwrap_or(raw);
    let kept: String = last
        .chars()
        .filter(|c| {
            !c.is_control() && !reorders(*c) && !matches!(c, '<' | '>' | '"' | '|' | '?' | '*')
        })
        .take(MAX_NAME_CHARS)
        .collect();
    // Trailing dots and spaces too: Windows drops them from a path, so
    // `report.exe.` has no ending here and is `report.exe` on disk.
    let kept = kept
        .trim_matches(|c: char| c.is_whitespace() || c == '.')
        .to_string();
    if kept.is_empty() {
        "attachment".to_string()
    } else {
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream, StringFormat};

    /// A document carrying `files` as `(key, name, bytes, compressed)`, in a
    /// name tree of one leaf, or of one kid per file when `kids`.
    fn carrying(files: &[(&str, &str, &[u8], bool)], kids: bool) -> Document {
        let mut doc = Document::with_version("1.7");
        let mut leaves = Vec::new();
        for (key, name, bytes, compressed) in files {
            let mut stream = Stream::new(
                dictionary! { "Type" => "EmbeddedFile", "Params" => dictionary! { "Size" => bytes.len() as i64 } },
                bytes.to_vec(),
            );
            if *compressed {
                stream.compress().expect("compress");
            }
            let file = doc.add_object(stream);
            let spec = doc.add_object(dictionary! {
                "Type" => "Filespec",
                "F" => Object::String(name.as_bytes().to_vec(), StringFormat::Literal),
                "Desc" => Object::String(format!("about {key}").into_bytes(), StringFormat::Literal),
                "EF" => dictionary! { "F" => file },
            });
            leaves.push(vec![
                Object::String(key.as_bytes().to_vec(), StringFormat::Literal),
                Object::Reference(spec),
            ]);
        }
        let tree = if kids {
            let kids: Vec<Object> = leaves
                .into_iter()
                .map(|pair| Object::Reference(doc.add_object(dictionary! { "Names" => pair })))
                .collect();
            dictionary! { "Kids" => kids }
        } else {
            dictionary! { "Names" => leaves.concat() }
        };
        let tree = doc.add_object(tree);
        let names = doc.add_object(dictionary! { "EmbeddedFiles" => tree });
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Names" => names });
        doc.trailer.set("Root", catalog);
        doc
    }

    #[test]
    fn lists_each_file_with_its_name_description_and_stated_size() {
        let doc = carrying(
            &[
                ("a", "invoice.xml", b"<i/>", false),
                ("b", "data.csv", b"1,2,3\n", true),
            ],
            false,
        );
        assert_eq!(
            list(&doc),
            vec![
                Embedded {
                    name: "invoice.xml".into(),
                    description: "about a".into(),
                    size: Some(4)
                },
                Embedded {
                    name: "data.csv".into(),
                    description: "about b".into(),
                    size: Some(6)
                },
            ]
        );
    }

    #[test]
    fn takes_out_the_bytes_as_they_were_put_in_compressed_or_not() {
        // Long enough to compress: `Stream::compress` keeps a stream that would
        // not get smaller as it is, and a short one let this test pass with the
        // decoding removed.
        let packed = b"packed ".repeat(50);
        let doc = carrying(
            &[
                ("a", "plain.txt", b"plain", false),
                ("b", "packed.txt", &packed, true),
            ],
            false,
        );
        let filtered = doc
            .objects
            .values()
            .filter_map(|object| object.as_stream().ok())
            .filter(|stream| stream.dict.has(b"Filter"))
            .count();
        assert_eq!(filtered, 1, "the second file is stored compressed");
        assert_eq!(content(&doc, 0).expect("plain"), b"plain");
        assert_eq!(content(&doc, 1).expect("packed"), packed);
        let missing = content(&doc, 2).expect_err("no third");
        assert!(missing.contains("no attachment 3 (it has 2)"), "{missing}");
    }

    #[test]
    fn walks_the_kids_of_a_tree_that_branches() {
        let doc = carrying(
            &[("a", "one.txt", b"1", false), ("b", "two.txt", b"2", false)],
            true,
        );
        let names: Vec<String> = list(&doc).into_iter().map(|file| file.name).collect();
        assert_eq!(names, ["one.txt", "two.txt"]);
        assert_eq!(content(&doc, 1).expect("second"), b"2");
    }

    #[test]
    fn a_tree_whose_kid_names_its_parent_ends() {
        let mut doc = carrying(&[("a", "one.txt", b"1", false)], true);
        let names = doc
            .catalog()
            .unwrap()
            .get(b"Names")
            .unwrap()
            .as_reference()
            .unwrap();
        let tree = doc
            .get_dictionary(names)
            .unwrap()
            .get(b"EmbeddedFiles")
            .unwrap()
            .as_reference()
            .unwrap();
        let kids = doc
            .get_dictionary(tree)
            .unwrap()
            .get(b"Kids")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let kid = kids[0].as_reference().unwrap();
        doc.get_dictionary_mut(kid)
            .unwrap()
            .set("Kids", vec![Object::Reference(tree)]);
        assert_eq!(list(&doc).len(), 1);
    }

    #[test]
    fn a_document_with_no_list_lists_nothing() {
        let mut doc = Document::with_version("1.7");
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog" });
        doc.trailer.set("Root", catalog);
        assert!(list(&doc).is_empty());
        assert!(content(&doc, 0).is_err());
    }

    /// `carrying`'s document written to a file in `dir`.
    fn written(dir: &crate::testutil::TempDir, doc: &mut Document) -> std::path::PathBuf {
        let path = dir.join("carrier.pdf");
        doc.save(&path).expect("save");
        path
    }

    #[test]
    fn saves_a_file_beside_nothing_and_says_how_many_bytes() {
        let dir = crate::testutil::TempDir::new("attachments-save");
        let mut doc = carrying(
            &[
                ("a", "a.txt", b"first", false),
                ("b", "b.bin", b"second file", true),
            ],
            false,
        );
        let source = written(&dir, &mut doc);
        let dest = dir.join("out.bin");
        assert_eq!(save(&source, 1, &dest, None, &crate::save::Here), Ok(11));
        assert_eq!(std::fs::read(&dest).expect("read"), b"second file");
        assert_eq!(dir.names(), ["carrier.pdf", "out.bin"]);
    }

    #[test]
    fn refuses_to_save_over_the_document_it_comes_from() {
        let dir = crate::testutil::TempDir::new("attachments-self");
        let mut doc = carrying(&[("a", "a.txt", b"first", false)], false);
        let source = written(&dir, &mut doc);
        let before = std::fs::read(&source).expect("read");
        let refused = save(&source, 0, &source, None, &crate::save::Here).expect_err("over itself");
        assert!(refused.contains("the document it comes from"), "{refused}");
        assert_eq!(std::fs::read(&source).expect("read"), before);
    }

    #[test]
    fn a_save_that_fails_leaves_the_old_file_and_no_staged_one() {
        let dir = crate::testutil::TempDir::new("attachments-fail");
        let mut doc = carrying(&[("a", "a.txt", b"first", false)], false);
        let source = written(&dir, &mut doc);
        let dest = dir.join("kept.txt");
        std::fs::write(&dest, b"kept").expect("seed");
        let refused = save(&source, 5, &dest, None, &crate::save::Here).expect_err("no sixth");
        assert!(refused.contains("no attachment 6"), "{refused}");
        assert_eq!(std::fs::read(&dest).expect("read"), b"kept");
        assert_eq!(dir.names(), ["carrier.pdf", "kept.txt"]);
    }

    /// The name a save to `dest` would stage at, asked of the code that names
    /// it, so that a planted file cannot end up beside a name nothing uses.
    fn staged_name(dest: &std::path::Path) -> std::path::PathBuf {
        let name = dest.file_name().expect("a file name");
        let (staged, file) = crate::save::claim_beside(dest, name).expect("a free name");
        drop(file);
        std::fs::remove_file(&staged).expect("give the name back");
        staged
    }

    #[cfg(unix)]
    #[test]
    fn a_link_planted_at_the_staged_name_is_not_written_through() {
        let dir = crate::testutil::TempDir::new("attachments-link");
        let mut doc = carrying(&[("a", "a.txt", b"first", false)], false);
        let source = written(&dir, &mut doc);
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"do not overwrite me").expect("seed");
        let dest = dir.join("invoice.xml");
        let planted = staged_name(&dest);
        std::os::unix::fs::symlink(&victim, &planted).expect("plant the link");

        assert_eq!(save(&source, 0, &dest, None, &crate::save::Here), Ok(5));

        assert_eq!(
            std::fs::read(&victim).expect("read"),
            b"do not overwrite me"
        );
        assert_eq!(std::fs::read(&dest).expect("read"), b"first");
        let link = std::fs::symlink_metadata(&planted).expect("the link is left alone");
        assert!(link.file_type().is_symlink());
    }

    #[test]
    fn a_file_left_at_the_staged_name_survives_a_save_that_fails() {
        let dir = crate::testutil::TempDir::new("attachments-left");
        let mut doc = carrying(&[("a", "a.txt", b"first", false)], false);
        let source = written(&dir, &mut doc);
        let dest = dir.join("kept.txt");
        let left = staged_name(&dest);
        std::fs::write(&left, b"somebody else's").expect("leave it");

        let refused = save(&source, 5, &dest, None, &crate::save::Here).expect_err("no sixth");

        assert!(refused.contains("no attachment 6"), "{refused}");
        assert_eq!(std::fs::read(&left).expect("read"), b"somebody else's");
        assert_eq!(dir.names().len(), 2, "{:?}", dir.names());
    }

    /// The reason this does not stage through `save::stage`, which copies the
    /// mode of the file it replaces: the bytes are the document author's, and
    /// the file they replace may be too.
    #[cfg(unix)]
    #[test]
    fn saving_over_a_program_does_not_make_the_attachment_one() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = crate::testutil::TempDir::new("attachments-mode");
        let mut doc = carrying(&[("a", "a.txt", b"first", false)], false);
        let source = written(&dir, &mut doc);
        let dest = dir.join("tool");
        std::fs::write(&dest, b"old").expect("seed");
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        assert_eq!(save(&source, 0, &dest, None, &crate::save::Here), Ok(5));

        assert_eq!(std::fs::read(&dest).expect("read"), b"first");
        let mode = std::fs::metadata(&dest).expect("stat").permissions().mode();
        assert_eq!(mode & 0o111, 0, "{mode:o}");
    }

    #[test]
    fn a_name_keeps_its_last_part_and_nothing_a_file_system_refuses() {
        assert_eq!(file_name("C:\\Users\\x\\report.xlsx"), "report.xlsx");
        assert_eq!(file_name("../../etc/passwd"), "passwd");
        assert_eq!(file_name("a\u{0}b?.txt"), "ab.txt");
        assert_eq!(file_name(".hidden"), "hidden");
        assert_eq!(file_name("  "), "attachment");
        assert_eq!(file_name("report.exe."), "report.exe");
        assert_eq!(file_name("report.exe . ."), "report.exe");
        assert_eq!(file_name("Rechnung\u{202e}fdp.exe"), "Rechnungfdp.exe");
        assert_eq!(
            file_name("a\u{061c}b\u{200b}c\u{2066}d\u{feff}.pdf"),
            "abcd.pdf"
        );
        assert_eq!(file_name("\u{202e}..."), "attachment");
    }
}
