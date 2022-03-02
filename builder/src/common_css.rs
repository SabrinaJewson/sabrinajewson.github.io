use crate::{
    asset::{self, Asset},
    minify,
};
use ::std::path::Path;

pub(crate) fn asset<'a>(in_path: &'a Path, out_path: &'a Path) -> impl Asset<Output = ()> + 'a {
    asset::TextFile::new(in_path)
        .map(|res| {
            let css = match res {
                Ok(css) => css,
                Err(e) => {
                    log::error!("{e:?}");
                    return String::new();
                }
            };

            let minified = match minify::css(&css) {
                Ok(minified) => minified,
                Err(e) => {
                    log::error!("{:?}", e.context("failed to minify common CSS"));
                    css
                }
            };

            log::info!("successfully emitted common CSS file");

            minified
        })
        .to_file(out_path)
        .map(|res| {
            if let Err(e) = res {
                log::error!("{e:?}");
            }
        })
}
