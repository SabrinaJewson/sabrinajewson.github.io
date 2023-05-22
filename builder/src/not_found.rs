pub(crate) fn asset<'a>(
    template_path: &'a Path,
    output_path: &'a Path,
    templater: impl Asset<Output = Templater> + 'a,
) -> impl Asset<Output = ()> + 'a {
    let template = asset::TextFile::new(template_path)
        .map(|src| Template::compile(&src?).context("failed to compile 404 template"))
        .map(Rc::new)
        .cache();

    asset::all((templater, template))
        .map(|(templater, template)| -> Result<String, ErrorPage> {
            Ok(templater.render((*template).as_ref()?, ())?)
        })
        .map(move |html| {
            write_file(output_path, html.unwrap_or_else(ErrorPage::into_html))?;
            log::info!("successfully emitted 404 file");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(output_path)
}

use crate::templater::Templater;
use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use crate::util::write_file;
use crate::util::ErrorPage;
use anyhow::Context as _;
use handlebars::Template;
use std::path::Path;
use std::rc::Rc;
