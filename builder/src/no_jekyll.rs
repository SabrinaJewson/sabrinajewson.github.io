use crate::util::{
    asset::{self, Asset},
    log_errors,
};
use ::{
    anyhow::Context as _,
    std::{fs::File, path::Path},
};

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
