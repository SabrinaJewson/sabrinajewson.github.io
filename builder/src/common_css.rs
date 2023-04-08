use crate::util::{
    asset::{self, Asset},
    log_errors, minify, write_file,
};
use ::std::path::Path;

// TODO: Make this path a variable rather than a constant
pub(crate) const PATH: &str = "common.css";

pub(crate) fn asset<'a>(in_path: &'a Path, out_path: &'a Path) -> impl Asset<Output = ()> + 'a {
    asset::TextFile::new(in_path)
        .map(move |res| -> anyhow::Result<_> {
            let css = minify::css(&res?);
            write_file(out_path.join(PATH), css)?;
            log::info!("successfully emitted common CSS file");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out_path.join(PATH))
}
