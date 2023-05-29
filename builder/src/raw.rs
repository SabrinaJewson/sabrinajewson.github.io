pub(crate) fn asset<'a>(src_dir: &'a Path, out_dir: &'a Path) -> impl Asset<Output = ()> + 'a {
    asset::Volatile
        .map(move |()| -> anyhow::Result<_> {
            let mut assets = Vec::new();

            for entry in WalkDir::new(src_dir).follow_links(true) {
                let entry = entry?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let src = entry.into_path();
                let relative = src.strip_prefix(src_dir).with_context(|| {
                    format!(
                        "failed to strip prefix {} from {}",
                        src_dir.display(),
                        src.display()
                    )
                })?;
                let dest_0 = out_dir.join(relative);
                let dest_1 = dest_0.clone();

                let asset = asset::FsPath::new(src.clone())
                    .map(move |()| {
                        make_parents(&dest_0)?;
                        fs::copy(&*src, &dest_0).with_context(|| {
                            format!("failed to copy {} to {}", src.display(), dest_0.display())
                        })?;
                        log::info!("Copied {} to {}", src.display(), dest_0.display());
                        Ok(())
                    })
                    .map(log_errors)
                    .modifies_path(dest_1);
                assets.push(asset);
            }

            Ok(asset::all(assets).map(|_| {}))
        })
        .map(|res| -> Rc<dyn Asset<Output = _>> {
            match res {
                Ok(asset) => Rc::new(asset),
                Err(e) => {
                    log::error!("{:?}", e);
                    Rc::new(asset::Constant::new(()))
                }
            }
        })
        .cache()
        .flatten()
}

use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use crate::util::make_parents;
use anyhow::Context;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use walkdir::WalkDir;
