pub(crate) fn asset<'a>(
    template_path: &'a Path,
    output_path: &'a Path,
    templater: impl Asset<Output = Templater<'a>> + 'a,
) -> impl Asset<Output = ()> + 'a {
    let template = asset::TextFile::new(template_path)
        .map(|src| Template::compile(&src?).context("failed to compile 404 template"))
        .map(Rc::new)
        .cache();

    asset::all((templater, template))
        .map(|(templater, template)| {
            let template = match &*template {
                Ok(template) => template,
                Err(e) => return error_page([e]),
            };

            match templater.render(template, ()) {
                Ok(rendered) => rendered,
                Err(e) => error_page([&e]),
            }
        })
        .map(move |html| {
            write_file(output_path, html)?;
            log::info!("successfully emitted 404 file");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(output_path)
}

use crate::templater::Templater;
use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::error_page;
use crate::util::log_errors;
use crate::util::write_file;
use anyhow::Context as _;
use handlebars::Template;
use std::path::Path;
use std::rc::Rc;
