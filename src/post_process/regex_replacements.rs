//! A post process step that runs a regex replacement over the new files

use crate::{metadata::Output, packaging::TempFiles};
use regex;

pub fn regex_post_process(temp_files: &TempFiles, output: &Output) -> Result<(), std::io::Error> {
    for post_process_step in output.recipe.build().post_process().iter() {
        let rx = regex::Regex::new(&post_process_step.regex).expect("Could not parse regex!");
        for file in temp_files.files.iter() {
            if post_process_step.files.is_match(&file) {
                let file_contents = std::fs::read_to_string(&file)?;
                let new_contents = rx.replace_all(&file_contents, &post_process_step.replacement);
                std::fs::write(&file, new_contents.as_bytes())?;
            }
        }
    }

    Ok(())
}
