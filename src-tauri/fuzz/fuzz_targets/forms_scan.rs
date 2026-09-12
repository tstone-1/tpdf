//! Hostile AcroForm trees through the same bounded loader and scanner as the worker.
//! Run with `src-tauri/fuzz/run.py --target forms_scan --seconds 3600`.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(document) = tpdf_lib::encoding::load(data, None) else {
        return;
    };
    if let Ok(form) = tpdf_lib::forms::scan(&document) {
        for widget in form.widgets {
            std::hint::black_box((
                widget.object,
                widget.page,
                widget.rect,
                widget.value,
                widget.control,
            ));
        }
    }
});
