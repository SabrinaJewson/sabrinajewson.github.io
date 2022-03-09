use crate::util::{
    asset::{self, Asset},
    log_errors,
};
use ::{
    anyhow::Context as _,
    std::{fs, path::Path},
};

pub(crate) fn asset<'a>(src: &'a Path, dest: &'a Path) -> impl Asset<Output = ()> + 'a {
    asset::FsPath::new(src)
        .map(move |()| log_errors(fs::copy(src, dest).context("failed to copy CNAME")))
        .modifies_path(dest)
}
