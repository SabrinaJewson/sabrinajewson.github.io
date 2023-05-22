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
            let (markdown, template) = ErrorPage::zip((*markdown).as_ref(), (*template).as_ref())?;

            #[derive(Serialize)]
            struct TemplateVars<'a> {
                body: &'a str,
                summary: &'a str,
            }
            let vars = TemplateVars {
                body: &markdown.body,
                summary: &markdown.summary,
            };
            Ok(templater.render(template, vars)?)
        })
        .map(move |html| {
            write_file(out_path, html.unwrap_or_else(ErrorPage::into_html))?;
            log::info!("successfully emitted index.html");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out_path)
}

use crate::templater::Templater;
use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use crate::util::markdown;
use crate::util::write_file;
use crate::util::ErrorPage;
use anyhow::Context as _;
use handlebars::Template;
use serde::Serialize;
use std::path::Path;
use std::rc::Rc;
