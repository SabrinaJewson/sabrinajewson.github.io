/// Global config shared by the entire program.
pub(crate) struct Config {
    /// Whether to build drafts.
    pub drafts: bool,

    /// Whether we minify the result.
    pub minify: bool,

    /// Whether to build icons.
    pub icons: bool,

    /// Whether we are live reloading.
    pub live_reload: bool,
}

pub(crate) fn copy_minify<'a>(
    config: impl Asset<Output = &'a Config> + 'a,
    file_type: minify::FileType,
    in_: impl AsRef<Path> + 'a,
    out: impl AsRef<Path> + Clone + 'a,
) -> impl Asset<Output = ()> + 'a {
    let out_1 = out.clone();
    asset::all((asset::TextFile::new(in_), config))
        .map(move |(res, config)| -> anyhow::Result<_> {
            let mut text = res?;
            if config.minify {
                minify(file_type, &mut text);
            }
            write_file(&out_1, text)?;
            log::info!("successfully emitted {}", out_1.as_ref().display());
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out)
}

use crate::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use crate::util::minify;
use crate::util::minify::minify;
use crate::util::write_file;
use std::path::Path;
