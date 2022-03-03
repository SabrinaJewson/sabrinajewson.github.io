use crate::{
    minify,
    templater::Templater,
    util::{
        asset::{self, Asset},
        error_page, log_errors, write_file,
    },
};
use ::{
    anyhow::Context as _,
    handlebars::Template,
    std::{path::Path, rc::Rc},
};

pub(crate) fn asset<'a>(
    template_path: &'a Path,
    output_path: &'a Path,
    templater: impl Asset<Output = Templater> + 'a,
) -> impl Asset<Output = ()> + 'a {
    let template = asset::TextFile::new(template_path)
        .map(|src| Template::compile(&*src?).context("failed to compile 404 template"))
        .map(Rc::new)
        .cache();

    asset::all((templater, template))
        .map(|(templater, template)| {
            let template = match &*template {
                Ok(template) => template,
                Err(e) => return error_page([e]),
            };

            let rendered = match templater.render(template, ()) {
                Ok(rendered) => rendered,
                Err(e) => return error_page([&e]),
            };

            minify::html(&rendered)
        })
        .map(move |html| log_errors(write_file(output_path, html)))
        .modifies_path(output_path)
        .map(|res| {
            if res.is_ok() {
                log::info!("successfully emitted 404 file");
            }
        })
}
