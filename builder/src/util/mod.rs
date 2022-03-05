//! This module contains many small independent components.

use self::push_str::push;
use ::{
    anyhow::Context as _,
    std::{fs, path::Path},
};

pub(crate) mod asset;
pub(crate) mod markdown;
pub(crate) mod minify;
pub(crate) mod push_str;

pub(crate) fn log_errors<T>(res: anyhow::Result<T>) {
    if let Err(e) = res {
        log::error!("{e:?}");
    }
}

pub(crate) fn error_page<'a, I: IntoIterator<Item = &'a anyhow::Error>>(errors: I) -> String {
    let mut res = String::new();
    for error in errors {
        log::error!("{error:?}");
        push!(res, "<p style='color:red'>Error: {error:?}</p>");
    }
    res
}

pub(crate) fn write_file<P: AsRef<Path>, D: AsRef<[u8]>>(path: P, data: D) -> anyhow::Result<()> {
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create dir `{}`", parent.display()))?;
    }

    fs::write(path, data)
        .with_context(|| format!("couldn't write asset to `{}`", path.display()))?;

    Ok(())
}
