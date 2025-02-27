//! Check that we can parse any `Menu/*.json` files as valid `menuinst` files.

use std::ffi::OsStr;

use crate::packaging::{PackagingError, TempFiles};
use rattler_menuinst::schema::MenuInstSchema;

/// Check that all `Menu/*.json` files are valid `menuinst` files.
pub fn menuinst(temp_files: &TempFiles) -> Result<(), PackagingError> {
    // find all new files `Menu/*.json`
    for p in temp_files.files.iter() {
        let prefix_path = p.strip_prefix(temp_files.temp_dir.path()).unwrap();
        if let Some(first) = prefix_path.components().next() {
            if first.as_os_str() == "Menu" && prefix_path.extension() == Some(OsStr::new("json")) {
                let content = fs_err::read_to_string(p)?;
                let _menu: MenuInstSchema = serde_json::from_str(&content)
                    .map_err(|e| PackagingError::InvalidMenuInstSchema(p.to_path_buf(), e))?;
            }
        }
    }

    Ok(())
}
