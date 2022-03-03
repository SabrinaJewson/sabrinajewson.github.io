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
                Ok(())
            }
        })
        .modifies_path(path)
        .map(|res| {
            if log_errors(res).is_ok() {
                log::info!("successfully emitted .nojekyll file");
            }
        })
}
