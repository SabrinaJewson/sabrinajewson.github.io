use crate::{
    common_css, icons,
    util::{
        asset::{self, Asset},
        error_page, log_errors, markdown, write_file,
    },
};
use ::{
    anyhow::Context as _,
    handlebars::{Handlebars, Renderable as _, Template},
    serde::Serialize,
    std::{path::Path, rc::Rc},
};

pub(crate) fn asset<'a>(
    in_dir: &'a Path,
    out_dir: &'a Path,
    templater: impl Asset<Output = Rc<Handlebars<'static>>> + Clone + 'a,
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
                // TODO: refactor `icons` and `common_css` into a common type
                icons: icons::Paths,
                common_css: &'static str,
            }
            let context = handlebars::Context::wraps(TemplateVars {
                body: &*markdown.body,
                summary: &*markdown.summary,
                icons: icons::PATHS,
                common_css: common_css::PATH,
            })
            .unwrap();

            let mut render_context = handlebars::RenderContext::new(None);
            let res = template
                .renders(&*templater, &context, &mut render_context)
                .context("failed to render blog post template");
            let rendered = match res {
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
