//! Hostile content discovery plus validated replacement through the production writer.
//! Run with `src-tauri/fuzz/run.py --target textedit_scan --seconds 3600`.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(mut document) = tpdf_lib::encoding::load(data, None) else {
        return;
    };
    for page in 0..4 {
        let Ok(runs) = tpdf_lib::textedit::scan(&document, page) else {
            continue;
        };
        let Some(run) = runs.runs.first() else {
            continue;
        };
        // Deletion is always within the original advance, so a supported seed
        // reaches the writer as well as all malformed-input refusal branches.
        let change = tpdf_lib::textedit::Change {
            page,
            revision: runs.revision,
            operator: run.operator,
            original: run.text.clone(),
            replacement: String::new(),
        };
        tpdf_lib::textedit::write(&mut document, &[change])
            .expect("a discovered run can be deleted");
        let after =
            tpdf_lib::textedit::scan(&document, page).expect("the edited stream remains supported");
        assert!(after.runs[0].text.is_empty());
    }
});
