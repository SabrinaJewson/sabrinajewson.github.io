use crate::util::{
    asset::{self, Asset},
    minify, write_file,
};
use ::std::path::Path;

pub(crate) const PATH: &str = "common.css";

pub(crate) fn asset<'a>(in_path: &'a Path, out_path: &'a Path) -> impl Asset<Output = ()> + 'a {
    asset::TextFile::new(in_path)
        .map(move |res| -> anyhow::Result<_> {
            let css = res?;
            let minified = match minify::css(&*css) {
                Ok(minified) => minified,
                Err(e) => {
                    log::error!("{:?}", e.context("failed to minify common CSS"));
                    css
                }
            };

            write_file(out_path.join(PATH), &minified)?;

            log::info!("successfully emitted common CSS file");

            Ok(())
        })
        .modifies_path(out_path.join(PATH))
        .map(|res| match res {
            Ok(()) => {}
            Err(e) => log::error!("{e:?}"),
        })
}
