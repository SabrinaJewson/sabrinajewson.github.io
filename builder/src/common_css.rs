use crate::util::{
    asset::{self, Asset},
    minify, write_file,
};
use ::std::path::Path;

pub(crate) fn asset<'a>(in_path: &'a Path, out_path: &'a Path) -> impl Asset<Output = ()> + 'a {
    asset::TextFile::new(in_path).map(move |res| {
        let css = match res {
            Ok(css) => css,
            Err(e) => {
                log::error!("{e:?}");
                return;
            }
        };

        let minified = match minify::css(&css) {
            Ok(minified) => minified,
            Err(e) => {
                log::error!("{:?}", e.context("failed to minify common CSS"));
                css
            }
        };

        if let Err(e) = write_file(out_path, &minified) {
            log::error!("{e:?}");
            return;
        }

        log::info!("successfully emitted common CSS file");
    })
}
