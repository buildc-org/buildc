use crate::layout;
use std::path::PathBuf;

pub fn path() -> PathBuf {
    PathBuf::from(layout::GENERATED)
        .join(layout::LIB)
        .join("build-record.sh")
}

pub fn script() -> String {
    include_str!("../../assets/lib/build-record.sh").to_string()
}
