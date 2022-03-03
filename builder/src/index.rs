use crate::{
    templater::Templater,
    util::{
        asset::{self, Asset},
        error_page, log_errors, markdown, write_file,
    },
};
use ::{
    anyhow::Context as _,
    handlebars::Template,
    serde::Serialize,
    std::{path::Path, rc::Rc},
};

pub(crate) fn asset<'a>(
    in_dir: &'a Path,
    out_dir: &'a Path,
    templater: impl Asset<Output = Templater> + Clone + 'a,
) -> impl Asset<Output = ()> + 'a {
    let template = asset::TextFile::new(in_dir.join("index.hbs"))
        .map(|src| Template::compile(&*src?).context("failed to compile index template"))
        .map(Rc::new)
        .cache();

    let markdown = asset::TextFile::new(in_dir.join("index.md"))
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
                body: &*markdown.body,
                summary: &*markdown.summary,
            };
            let rendered = match templater.render(template, vars) {
                Ok(rendered) => rendered,
                Err(e) => return error_page([&e]),
            };

            match crate::minify::html(&rendered) {
                Ok(minified) => minified,
                Err(e) => {
                    log::error!("{:?}", e.context("failed to minify index"));
                    rendered
                }
            }
        })
        .map(|html| log_errors(write_file(out_dir.join("index.html"), html)))
        .modifies_path(out_dir.join("index.html"))
        .map(|res| {
            if res.is_ok() {
                log::info!("successfully emitted index.html");
            }
        })
}
