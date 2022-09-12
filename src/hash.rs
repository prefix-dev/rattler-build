use std::path::Path;

use sha2::{Digest, Sha256};

pub fn sha256_digest(path: &Path) -> String {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path).expect("Give file");
    std::io::copy(&mut file, &mut hasher).expect("Could not compute hash");
    hex::encode(hasher.finalize())
}
