pub(crate) fn asset(output_path: &Path) -> impl Asset<Output = ()> {
    let path = output_path.join(".nojekyll");
    asset::Constant::new(())
        .map({
            let path = path.clone();
            move |()| {
                File::create(&path).context("failed to create .nojekyll file")?;
                log::info!("successfully emitted .nojekyll file");
                Ok(())
            }
        })
        .map(log_errors)
        .modifies_path(path)
}

use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use anyhow::Context as _;
use std::fs::File;
use std::path::Path;
