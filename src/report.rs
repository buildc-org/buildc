use crate::layout;
use common::ui::tree::Change;
use std::path::{Path, PathBuf};

pub type Say<'a> = &'a mut dyn FnMut(&str);

pub fn report(root: &Path, wrote: &[(PathBuf, Change)]) {
    let declared = crate::model::image::List::load(root).0.id;
    let id = match declared.is_empty() {
        true => layout::named_after_root(root).unwrap_or_default(),
        false => declared,
    };
    common::ui::tree::print(&id, wrote, describe);
}

fn describe(_path: &Path, change: Option<&Change>) -> String {
    match change {
        Some(Change::Updated(edit)) => edit.clone(),
        _ => String::new(),
    }
}
