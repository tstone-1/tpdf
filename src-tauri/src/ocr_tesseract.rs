//! Tesseract as an [`crate::ocr::Recogniser`], the engine on Linux.
//!
//! Linux has no text recogniser in the system the way macOS has Vision and
//! Windows has `Windows.Media.Ocr`, so tpdf brings its own choice: Tesseract
//! (Apache-2.0), decided on 2026-10-14 (`docs/PLAN.md` §1).
//!
//! ## What is the system's and what is tpdf's
//!
//! * **The library is the system's**: `libtesseract.so.5`, Ubuntu's package
//!   `libtesseract5`. It is opened at run time, in the OCR worker only and
//!   before that worker's boundary, the way PDFium is opened in the parser
//!   worker. So the application itself never maps it, and a machine without it
//!   starts tpdf normally and is told what is missing when it asks for OCR.
//! * **The language data is tpdf's**: `eng.traineddata` and `deu.traineddata`
//!   from tessdata_fast 4.1.0, pinned by digest in `scripts/fetch_tessdata.py`
//!   and shipped as bundle resources. A distribution's data package differs in
//!   version and in which languages are installed, and the redaction gate's
//!   verdict names the engine that read; data tpdf ships is data whose version
//!   it can name.
//!
//! ## The worker hands over pixels, never a picture file
//!
//! Tesseract links Leptonica and through it decoders for PNG, JPEG, TIFF, GIF,
//! WebP and JPEG 2000, and it links curl. None of them runs here: the image
//! arrives as a grey buffer this module made from the RGBA tpdf rendered
//! ([`to_grey`]), passed with `TessBaseAPISetImage`, which reads raw bytes. The
//! worker's seccomp filter refuses every socket, so curl could not connect if
//! something did call it.
//!
//! ## Coordinates
//!
//! `TessPageIteratorBoundingBox` reports pixels with the origin top-left, y
//! down, which is this codebase's convention, so the conversion is a division
//! by `scale` and no flip, as on Windows (`ocr_windows.rs` explains why the
//! Vision engine is the one that needs a flip).

use std::ffi::{c_char, c_float, c_int, c_void, CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::ocr::{EngineId, Options, Pixels, RecogniseError, RecognisedItem, Recogniser};

/// The engine name, one of [`crate::ocr_worker::KNOWN_ENGINES`].
pub const NAME: &str = "tesseract";

/// The library's soname, which is what Ubuntu 24.04's `libtesseract5` installs.
pub const LIBRARY: &str = "libtesseract.so.5";

/// The release the language data comes from, part of the engine's identity.
pub const DATA_RELEASE: &str = "tessdata_fast 4.1.0";

/// The languages tpdf ships data for: the BCP-47 tag a reader picks, and the
/// name of Tesseract's data file. English first, since it is what an
/// unspecified reading leads with.
pub const LANGUAGES: [(&str, &str); 2] = [("en", "eng"), ("de", "deu")];

/// Where the language data is, if anywhere.
///
/// Three candidates, the first that holds `eng.traineddata`:
///
/// 1. In a debug build only, `vendor/tessdata` in the source tree, where
///    `scripts/fetch_tessdata.py` puts it. Debug only for the reason
///    `lib.rs`'s `library_dir_among` gives for PDFium: the path is baked in at
///    compile time and a release must not look in the build machine's tree.
/// 2. `tessdata/` beside the executable: a checks build in
///    `target/release/`, and the command-line tool's `cli::resource_dir` rule.
/// 3. `<prefix>/lib/tpdf/tessdata` for an executable in `<prefix>/bin`, which is
///    where Tauri's Linux packages put bundle resources.
#[must_use]
pub fn data_dir() -> Option<PathBuf> {
    data_dir_among(&candidates())
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(debug_assertions)]
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/tessdata"));
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.canonicalize().ok())
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        out.push(dir.join("tessdata"));
        if let Some(prefix) = dir.parent() {
            out.push(prefix.join("lib").join("tpdf").join("tessdata"));
        }
    }
    out
}

/// [`data_dir`]'s choice among `candidates`, split out so it is tested
/// without an executable in a particular place.
#[must_use]
pub fn data_dir_among(candidates: &[PathBuf]) -> Option<PathBuf> {
    let required = format!("{}.traineddata", LANGUAGES[0].1);
    candidates
        .iter()
        .find(|dir| dir.join(&required).is_file())
        .cloned()
}

/// The tags of [`LANGUAGES`] whose data is in `dir`.
#[must_use]
pub fn offered_in(dir: &Path) -> Vec<String> {
    LANGUAGES
        .iter()
        .filter(|(_, file)| dir.join(format!("{file}.traineddata")).is_file())
        .map(|(tag, _)| (*tag).to_string())
        .collect()
}

/// The languages a reader can choose between: [`offered_in`] the data that
/// [`data_dir`] finds, or none.
///
/// Asked in the app process, which never maps the library: this reads only
/// whether files exist.
#[must_use]
pub fn languages() -> Vec<String> {
    data_dir().map_or_else(Vec::new, |dir| offered_in(&dir))
}

/// The `+`-joined data names to start Tesseract with.
///
/// Every offered language when none is asked, so an unspecified reading uses
/// all the data there is. Otherwise the first asked tag that is offered,
/// compared the way [`crate::ocr::first_offered`] compares; asking only for
/// languages with no data is refused, not read with another language's data.
///
/// # Errors
///
/// [`RecogniseError::Unavailable`] when nothing asked is offered, or nothing is
/// offered at all.
pub fn codes_for(asked: &[String], offered: &[String]) -> Result<String, RecogniseError> {
    let code = |tag: &str| {
        LANGUAGES
            .iter()
            .find(|(known, _)| *known == tag)
            .map(|(_, code)| *code)
    };
    if asked.is_empty() {
        let all: Vec<&str> = offered.iter().filter_map(|tag| code(tag)).collect();
        if all.is_empty() {
            return Err(RecogniseError::Unavailable(
                "no text-recognition language data is installed".into(),
            ));
        }
        return Ok(all.join("+"));
    }
    crate::ocr::first_offered(asked, offered)
        .and_then(|tag| code(&tag))
        .map(str::to_string)
        .ok_or_else(|| {
            RecogniseError::Unavailable(format!(
                "tpdf has no text-recognition data for {}; it reads {}",
                asked.join(", "),
                offered.join(", ")
            ))
        })
}

/// RGBA to one grey byte per pixel, laid over white.
///
/// Grey because that is what Tesseract binarises from anyway, and it removes
/// the question of which byte order a four-byte pixel is read in. Over white
/// because a transparent pixel is paper: tpdf renders pages opaque, and a
/// buffer that was not would otherwise read as black where nothing is.
///
/// The weights are ITU-R BT.601's luma. **A test with black text cannot see a
/// mistake here**, as `ocr_windows.rs` says of its channel swap: black and white
/// come out the same whichever weight goes on which channel, so the tests use
/// pure red, green and blue.
///
/// `None` for a buffer that is not whole pixels.
#[must_use]
pub fn to_grey(rgba: &[u8]) -> Option<Vec<u8>> {
    if rgba.len() % 4 != 0 {
        return None;
    }
    Some(
        rgba.chunks_exact(4)
            .map(|p| {
                let alpha = u32::from(p[3]);
                let over = |c: u8| (u32::from(c) * alpha + 255 * (255 - alpha) + 127) / 255;
                let luma = (299 * over(p[0]) + 587 * over(p[1]) + 114 * over(p[2]) + 500) / 1000;
                // At most 255: the weights sum to 1000 and each input is at most 255.
                u8::try_from(luma).unwrap_or(u8::MAX)
            })
            .collect(),
    )
}

/// A box in pixels of the image to `[left, top, right, bottom]` in points.
#[must_use]
pub fn pixels_to_points(
    left: c_int,
    top: c_int,
    right: c_int,
    bottom: c_int,
    scale: f32,
) -> [f32; 4] {
    #[allow(clippy::cast_precision_loss)] // pixel coordinates are far below 2^24
    let to = |v: c_int| v as f32 / scale;
    [to(left), to(top), to(right), to(bottom)]
}

// `TessOcrEngineMode::OEM_LSTM_ONLY`: tessdata_fast holds only LSTM models.
const OEM_LSTM_ONLY: c_int = 1;
// `TessPageSegMode::PSM_SPARSE_TEXT`: find as much text as possible, in no
// particular order. Measured 2026-10-14 with Tesseract 5.3.4 and this data, word
// lists off, on pages rendered at 144 dpi. `text-base14.pdf`: modes 3 (automatic,
// the command-line default) and 4 dropped the line under a rule, 24 of 30 words;
// 6 and 11 read all 30. `columns.pdf`: mode 3 misread most words ("pha one" for
// "alpha one"); 11 read every word and interleaved the two columns line by line.
// A missed word is what the redaction gate cannot afford, so 11, and a text layer
// over a multi-column scan copies out in that interleaved order.
const PSM_SPARSE_TEXT: c_int = 11;
// `TessPageIteratorLevel`.
const RIL_TEXTLINE: c_int = 2;
const RIL_WORD: c_int = 3;

type Handle = *mut c_void;

/// The functions of Tesseract's C interface (`tesseract/capi.h`) this uses.
struct Api {
    create: unsafe extern "C" fn() -> Handle,
    delete: unsafe extern "C" fn(Handle),
    #[allow(clippy::type_complexity)]
    init4: unsafe extern "C" fn(
        Handle,
        *const c_char,
        *const c_char,
        c_int,
        *mut *mut c_char,
        c_int,
        *mut *mut c_char,
        *mut *mut c_char,
        usize,
        c_int,
    ) -> c_int,
    set_page_seg_mode: unsafe extern "C" fn(Handle, c_int),
    set_image: unsafe extern "C" fn(Handle, *const u8, c_int, c_int, c_int, c_int),
    set_source_resolution: unsafe extern "C" fn(Handle, c_int),
    recognize: unsafe extern "C" fn(Handle, *mut c_void) -> c_int,
    get_iterator: unsafe extern "C" fn(Handle) -> Handle,
    iterator_delete: unsafe extern "C" fn(Handle),
    iterator_next: unsafe extern "C" fn(Handle, c_int) -> c_int,
    iterator_text: unsafe extern "C" fn(Handle, c_int) -> *mut c_char,
    iterator_confidence: unsafe extern "C" fn(Handle, c_int) -> c_float,
    iterator_page: unsafe extern "C" fn(Handle) -> Handle,
    page_bounding_box: unsafe extern "C" fn(
        Handle,
        c_int,
        *mut c_int,
        *mut c_int,
        *mut c_int,
        *mut c_int,
    ) -> c_int,
    delete_text: unsafe extern "C" fn(*const c_char),
    version: unsafe extern "C" fn() -> *const c_char,
    // Last, so the functions above are never used after it is closed: fields
    // drop in declaration order, and these are plain pointers into it.
    _library: libloading::Library,
}

impl Api {
    fn load() -> Result<Self, RecogniseError> {
        let unavailable = |e: libloading::Error| {
            RecogniseError::Unavailable(format!(
                "text recognition needs Tesseract, and {LIBRARY} could not be loaded ({e}); \
                 on Ubuntu and Debian it is the package libtesseract5"
            ))
        };
        // SAFETY: opening a shared library runs its initialisers. This is
        // Tesseract's own library, opened in the OCR worker before its boundary,
        // which is what the worker exists to hold.
        let library = unsafe { libloading::Library::new(LIBRARY) }.map_err(unavailable)?;
        macro_rules! symbol {
            ($name:literal) => {
                // SAFETY: the type each field declares is the one `capi.h`
                // declares for this symbol, and the pointer is used only while
                // `_library` keeps the library open (see the field order).
                *unsafe { library.get($name) }.map_err(unavailable)?
            };
        }
        Ok(Self {
            create: symbol!(b"TessBaseAPICreate"),
            delete: symbol!(b"TessBaseAPIDelete"),
            init4: symbol!(b"TessBaseAPIInit4"),
            set_page_seg_mode: symbol!(b"TessBaseAPISetPageSegMode"),
            set_image: symbol!(b"TessBaseAPISetImage"),
            set_source_resolution: symbol!(b"TessBaseAPISetSourceResolution"),
            recognize: symbol!(b"TessBaseAPIRecognize"),
            get_iterator: symbol!(b"TessBaseAPIGetIterator"),
            iterator_delete: symbol!(b"TessResultIteratorDelete"),
            iterator_next: symbol!(b"TessResultIteratorNext"),
            iterator_text: symbol!(b"TessResultIteratorGetUTF8Text"),
            iterator_confidence: symbol!(b"TessResultIteratorConfidence"),
            iterator_page: symbol!(b"TessResultIteratorGetPageIteratorConst"),
            page_bounding_box: symbol!(b"TessPageIteratorBoundingBox"),
            delete_text: symbol!(b"TessDeleteText"),
            version: symbol!(b"TessVersion"),
            _library: library,
        })
    }
}

/// One `TessBaseAPI`, deleted when this goes out of scope.
struct Base<'a> {
    api: &'a Api,
    handle: Handle,
}

impl Drop for Base<'_> {
    fn drop(&mut self) {
        // SAFETY: a handle from `TessBaseAPICreate`, deleted once.
        unsafe { (self.api.delete)(self.handle) };
    }
}

/// One `TessResultIterator`, deleted when this goes out of scope.
struct Results<'a> {
    api: &'a Api,
    handle: Handle,
}

impl Drop for Results<'_> {
    fn drop(&mut self) {
        // SAFETY: a non-null iterator from `TessBaseAPIGetIterator`, deleted once.
        unsafe { (self.api.iterator_delete)(self.handle) };
    }
}

/// Tesseract, opened, with the language data it will read.
pub struct Tesseract {
    api: Api,
    dir: PathBuf,
    offered: Vec<String>,
    version: String,
}

impl Tesseract {
    /// Finds the language data and opens the library.
    ///
    /// # Errors
    ///
    /// [`RecogniseError::Unavailable`] when either is missing, saying which.
    pub fn load() -> Result<Self, RecogniseError> {
        let dir = data_dir().ok_or_else(|| {
            RecogniseError::Unavailable(
                "tpdf's text-recognition language data is not where tpdf looks for it; \
                 reinstalling should fix it"
                    .into(),
            )
        })?;
        let offered = offered_in(&dir);
        let api = Api::load()?;
        // SAFETY: `TessVersion` returns a static NUL-terminated string.
        let version = unsafe { CStr::from_ptr((api.version)()) }
            .to_string_lossy()
            .into_owned();
        Ok(Self {
            api,
            dir,
            offered,
            version,
        })
    }

    /// The folder the language data is read from, which the worker's sandbox
    /// must let it read.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.dir
    }
}

impl Recogniser for Tesseract {
    fn id(&self) -> EngineId {
        EngineId {
            name: NAME,
            build: format!("{}, {DATA_RELEASE}", self.version),
        }
    }

    /// Reads with the asked language's data, or with all of it.
    ///
    /// [`Options::language_correction`] off, which is how the redaction gate
    /// always asks, starts Tesseract without its word lists
    /// (`load_system_dawg` and `load_freq_dawg` false), so it cannot turn a
    /// mark it half read into a dictionary word. [`Options::deadline_ms`] is
    /// not passed on: the parent kills a worker that does not answer in time
    /// (`ocr_worker::REPLY_DEADLINE`), which is the only bound that holds for
    /// an engine that wedges.
    fn recognise(
        &self,
        pixels: Pixels<'_>,
        options: &Options,
    ) -> Result<Vec<RecognisedItem>, RecogniseError> {
        if !pixels.is_consistent() {
            return Err(RecogniseError::MalformedInput(format!(
                "{} byte(s) is not {}x{} of RGBA",
                pixels.rgba.len(),
                pixels.width,
                pixels.height
            )));
        }
        let too_large = |what: &str| {
            RecogniseError::Rejected(format!("the image's {what} does not fit an int"))
        };
        let width = c_int::try_from(pixels.width).map_err(|_| too_large("width"))?;
        let height = c_int::try_from(pixels.height).map_err(|_| too_large("height"))?;
        let grey = to_grey(pixels.rgba)
            .ok_or_else(|| RecogniseError::MalformedInput("not whole pixels".into()))?;
        let codes = codes_for(&options.languages, &self.offered)?;

        let text = |s: &str| {
            CString::new(s).map_err(|_| RecogniseError::MalformedInput(format!("{s:?} has a NUL")))
        };
        let datapath = CString::new(self.dir.as_os_str().as_bytes()).map_err(|_| {
            RecogniseError::Unavailable("the language data's folder has a NUL in its name".into())
        })?;
        let language = text(&codes)?;
        let (names, values): (Vec<CString>, Vec<CString>) = if options.language_correction {
            (Vec::new(), Vec::new())
        } else {
            (
                vec![text("load_system_dawg")?, text("load_freq_dawg")?],
                vec![text("0")?, text("0")?],
            )
        };
        let mut name_ptrs: Vec<*mut c_char> = names.iter().map(|s| s.as_ptr().cast_mut()).collect();
        let mut value_ptrs: Vec<*mut c_char> =
            values.iter().map(|s| s.as_ptr().cast_mut()).collect();

        // SAFETY: plain calls into the library with the arguments `capi.h`
        // declares. Every pointer passed lives until after the call that takes
        // it: the strings and arrays above, and `grey`, which Tesseract reads
        // during `SetImage` and `Recognize`, both below while it is in scope.
        // The handles are owned by `Base` and `Results` and deleted once.
        unsafe {
            let created = (self.api.create)();
            if created.is_null() {
                return Err(RecogniseError::Crashed(
                    "TessBaseAPICreate returned null".into(),
                ));
            }
            let base = Base {
                api: &self.api,
                handle: created,
            };
            let started = (self.api.init4)(
                base.handle,
                datapath.as_ptr(),
                language.as_ptr(),
                OEM_LSTM_ONLY,
                std::ptr::null_mut(),
                0,
                name_ptrs.as_mut_ptr(),
                value_ptrs.as_mut_ptr(),
                name_ptrs.len(),
                0,
            );
            if started != 0 {
                return Err(RecogniseError::Unavailable(format!(
                    "Tesseract could not start with {codes} from {}",
                    self.dir.display()
                )));
            }
            (self.api.set_page_seg_mode)(base.handle, PSM_SPARSE_TEXT);
            (self.api.set_image)(base.handle, grey.as_ptr(), width, height, 1, width);
            // Pixels per point times 72 points to the inch.
            #[allow(clippy::cast_possible_truncation)] // a page's resolution fits an int
            let ppi = (pixels.scale * 72.0).round() as c_int;
            (self.api.set_source_resolution)(base.handle, ppi.max(70));
            if (self.api.recognize)(base.handle, std::ptr::null_mut()) != 0 {
                return Err(RecogniseError::Rejected(
                    "Tesseract could not read the image".into(),
                ));
            }
            let iterator = (self.api.get_iterator)(base.handle);
            if iterator.is_null() {
                return Err(RecogniseError::Crashed(
                    "Tesseract recognised the image and returned no results".into(),
                ));
            }
            let results = Results {
                api: &self.api,
                handle: iterator,
            };
            let level = if options.words {
                RIL_WORD
            } else {
                RIL_TEXTLINE
            };
            let page = (self.api.iterator_page)(results.handle);
            let mut items = Vec::new();
            loop {
                let read = (self.api.iterator_text)(results.handle, level);
                if !read.is_null() {
                    let said = CStr::from_ptr(read).to_string_lossy().trim().to_string();
                    (self.api.delete_text)(read);
                    let (mut left, mut top, mut right, mut bottom) = (0, 0, 0, 0);
                    let boxed = (self.api.page_bounding_box)(
                        page,
                        level,
                        &mut left,
                        &mut top,
                        &mut right,
                        &mut bottom,
                    ) != 0;
                    if boxed && !said.is_empty() {
                        items.push(RecognisedItem {
                            text: said,
                            rect: pixels_to_points(left, top, right, bottom, pixels.scale),
                            confidence: Some(
                                (self.api.iterator_confidence)(results.handle, level) / 100.0,
                            ),
                        });
                    }
                }
                if (self.api.iterator_next)(results.handle, level) == 0 {
                    break;
                }
            }
            drop(results);
            drop(base);
            Ok(items)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_channel_weighs_what_luma_gives_it() {
        assert_eq!(to_grey(&[255, 0, 0, 255]), Some(vec![76]));
        assert_eq!(to_grey(&[0, 255, 0, 255]), Some(vec![150]));
        assert_eq!(to_grey(&[0, 0, 255, 255]), Some(vec![29]));
    }

    #[test]
    fn black_and_white_cannot_tell_the_weights_apart() {
        assert_eq!(
            to_grey(&[0, 0, 0, 255, 255, 255, 255, 255]),
            Some(vec![0, 255])
        );
    }

    #[test]
    fn a_transparent_pixel_is_paper() {
        assert_eq!(to_grey(&[0, 0, 0, 0]), Some(vec![255]));
        assert_eq!(to_grey(&[0, 0, 0, 128]), Some(vec![127]));
    }

    #[test]
    fn a_buffer_that_is_not_whole_pixels_is_refused() {
        assert_eq!(to_grey(&[0, 0, 0]), None);
    }

    #[test]
    fn pixels_divide_by_scale_with_no_flip() {
        assert_eq!(
            pixels_to_points(10, 20, 30, 40, 2.0),
            [5.0, 10.0, 15.0, 20.0]
        );
    }

    #[test]
    fn an_unspecified_reading_uses_every_language_offered() {
        let offered = vec!["en".to_string(), "de".to_string()];
        assert_eq!(codes_for(&[], &offered), Ok("eng+deu".into()));
        assert_eq!(codes_for(&[], &offered[1..]), Ok("deu".into()));
    }

    #[test]
    fn an_asked_language_is_read_alone() {
        let offered = vec!["en".to_string(), "de".to_string()];
        assert_eq!(codes_for(&["DE".into()], &offered), Ok("deu".into()));
        assert_eq!(
            codes_for(&["fr".into(), "en".into()], &offered),
            Ok("eng".into())
        );
    }

    #[test]
    fn a_language_with_no_data_is_refused_rather_than_read_with_another() {
        let offered = vec!["en".to_string()];
        assert!(matches!(
            codes_for(&["de".into()], &offered),
            Err(RecogniseError::Unavailable(_))
        ));
        assert!(matches!(
            codes_for(&[], &[]),
            Err(RecogniseError::Unavailable(_))
        ));
    }

    #[test]
    fn the_first_folder_holding_english_data_is_the_one() {
        let root = std::env::temp_dir().join(format!("tpdf-tessdata-{}", std::process::id()));
        let (empty, german, both) = (root.join("a"), root.join("b"), root.join("c"));
        for dir in [&empty, &german, &both] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(german.join("deu.traineddata"), b"x").unwrap();
        std::fs::write(both.join("deu.traineddata"), b"x").unwrap();
        std::fs::write(both.join("eng.traineddata"), b"x").unwrap();
        let found = data_dir_among(&[empty.clone(), german.clone(), both.clone()]);
        assert_eq!(found.as_deref(), Some(both.as_path()));
        assert_eq!(offered_in(&both), ["en", "de"]);
        assert_eq!(offered_in(&german), ["de"]);
        assert_eq!(data_dir_among(&[empty, german]), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
