pub(crate) fn asset<'a>(
    template_path: &'a Path,
    src_path: &'a Path,
    out_path: &'a Path,
    templater: impl Asset<Output = Templater> + Clone + 'a,
) -> impl Asset<Output = ()> + 'a {
    let template = asset::TextFile::new(template_path)
        .map(|src| Template::compile(&src?).context("failed to compile index template"))
        .map(Rc::new)
        .cache();

    let markdown = asset::TextFile::new(src_path)
        .map(|src| Rc::new(src.map(|src| markdown::parse(&src))))
        .cache();

    asset::all((markdown, templater, template))
        .map(|(markdown, templater, template)| {
            let (markdown, template) = match (&*markdown, &*template) {
                (Ok(markdown), Ok(template)) => (markdown, template),
                (Ok(_), Err(e)) | (Err(e), Ok(_)) => return error_page([e]),
                (Err(e1), Err(e2)) => return error_page([e1, e2]),
            };

            #[derive(Serialize)]
            struct TemplateVars<'a> {
                body: &'a str,
                summary: &'a str,
            }
            let vars = TemplateVars {
                body: &markdown.body,
                summary: &markdown.summary,
            };
            let rendered = match templater.render(template, vars) {
                Ok(rendered) => rendered,
                Err(e) => return error_page([&e]),
            };

            minify::html(&rendered)
        })
        .map(move |html| {
            write_file(out_path, html)?;
            log::info!("successfully emitted index.html");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out_path)
}

use crate::templater::Templater;
use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::error_page;
use crate::util::log_errors;
use crate::util::markdown;
use crate::util::minify;
use crate::util::write_file;
use anyhow::Context as _;
use handlebars::Template;
use serde::Serialize;
use std::path::Path;
use std::rc::Rc;
