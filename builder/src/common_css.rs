// TODO: Make this path a variable rather than a constant
pub(crate) const PATH: &str = "common.css";

pub(crate) fn asset<'a>(
    in_path: &'a Path,
    out_path: &'a Path,
    config: impl Asset<Output = &'a Config> + 'a,
) -> impl Asset<Output = ()> + 'a {
    copy_minify(config, minify::FileType::Css, in_path, out_path.join(PATH))
}

use crate::config::copy_minify;
use crate::config::Config;
use crate::util::asset::Asset;
use crate::util::minify;
use std::path::Path;
