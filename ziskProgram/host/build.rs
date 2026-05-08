use std::path::PathBuf;
use common::sample_inputs;
use zisk_sdk::{ZiskStdin, build_program};

fn main() {
    build_program("../guest");

    let stdin_save = ZiskStdin::new();
    stdin_save.write(&sample_inputs());

    let path = PathBuf::from("tmp/input.bin");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    stdin_save.save(&path).unwrap();
}
