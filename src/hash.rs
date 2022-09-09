use std::path::PathBuf;

use sha2::{Digest, Sha256};

use hex;

pub fn sha256_digest(path: &PathBuf) -> String {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(&path).expect("Give file");
    std::io::copy(&mut file, &mut hasher).expect("Could not compute hash");
    return hex::encode(hasher.finalize());
}
