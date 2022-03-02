//! This module contains many small independent components.

use ::{
    anyhow::Context as _,
    std::{fs, path::Path},
};

pub(crate) mod asset;
pub(crate) mod markdown;
pub(crate) mod minify;
pub(crate) mod push_str;
pub(crate) mod template;

pub(crate) fn log_errors(res: anyhow::Result<()>) -> Result<(), ()> {
    if let Err(e) = &res {
        log::error!("{:?}", e);
    }
    res.map_err(drop)
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
