pub(crate) fn asset<'a>(src: &'a Path, dest: &'a Path) -> impl Asset<Output = ()> + 'a {
    asset::FsPath::new(src)
        .map(move |()| log_errors(fs::copy(src, dest).context("failed to copy CNAME")))
        .modifies_path(dest)
}

use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use anyhow::Context as _;
use std::fs;
use std::path::Path;
