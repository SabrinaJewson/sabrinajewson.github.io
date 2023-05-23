#[derive(Clone)]
pub(crate) struct Templater {
    handlebars: Rc<Handlebars<'static>>,
    live_reload: bool,
    icons: bool,
    minify: bool,
}

impl Templater {
    #[context("failed to render template")]
    pub(crate) fn render(
        &self,
        template: &Template,
        vars: impl Serialize,
    ) -> anyhow::Result<String> {
        #[derive(Serialize)]
        struct TemplateVars<T> {
            #[serde(flatten)]
            rest: T,
            icons: Option<icons::Paths>,
            common_css: &'static str,
            live_reload: bool,
        }

        let vars = TemplateVars {
            rest: vars,
            icons: self.icons.then_some(icons::PATHS),
            common_css: common_css::PATH,
            live_reload: self.live_reload,
        };
        let context = handlebars::Context::wraps(vars).unwrap();

        let mut render_context = handlebars::RenderContext::new(None);
        let mut rendered = template.renders(&self.handlebars, &context, &mut render_context)?;
        if self.minify {
            minify(minify::FileType::Html, &mut rendered);
        }
        Ok(rendered)
    }
}

thread_local! {
    static FALLBACK_TEMPLATER: Templater = Templater {
        handlebars: Rc::new(Handlebars::new()),
        // This value doesn't matter since we haven't included templates that reference it
        live_reload: false,
        icons: false,
        minify: false,
    };
}

pub(crate) fn asset<'a>(
    include_dir: &'a Path,
    config: impl Asset<Output = &'a Config> + Copy + 'a,
) -> impl Asset<Output = Templater> + 'a {
    asset::Dir::new(include_dir)
        .map(move |files| -> anyhow::Result<_> {
            let mut includes = Vec::new();

            for path in files? {
                let path = path?;
                if path.extension() != Some("hbs".as_ref()) {
                    continue;
                }

                let name = if let Some(name) = path.file_stem().unwrap().to_str() {
                    <Rc<str>>::from(name)
                } else {
                    log::error!("filename `{}` is not valid UTF-8", path.display());
                    continue;
                };

                let include = asset::TextFile::new(path)
                    .map(move |source| -> anyhow::Result<_> {
                        let template = Template::compile(&source?)
                            .with_context(|| format!("failed to compile template {name}"))?;
                        Ok((name.clone(), template))
                    })
                    .map(|res| res.map_err(|e| log::error!("{e:?}")))
                    .cache();

                includes.push(include);
            }

            Ok(asset::all((config, asset::all(includes)))
                .map(|(config, includes)| {
                    let mut handlebars = Handlebars::new();
                    for (name, include) in Vec::from(includes).into_iter().flatten() {
                        handlebars.register_template(&name, include);
                    }
                    Templater {
                        handlebars: Rc::new(handlebars),
                        icons: config.icons,
                        live_reload: config.live_reload,
                        minify: config.minify,
                    }
                })
                .cache())
        })
        .map(|res| -> Rc<dyn Asset<Output = _>> {
            match res {
                Ok(asset) => Rc::new(asset),
                Err(e) => {
                    log::error!("{e:?}");
                    Rc::new(asset::Constant::new(
                        FALLBACK_TEMPLATER.with(Templater::clone),
                    ))
                }
            }
        })
        .cache()
        .flatten()
}

use crate::common_css;
use crate::config::Config;
use crate::icons;
use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::minify;
use crate::util::minify::minify;
use anyhow::Context as _;
use fn_error_context::context;
use handlebars::template::Template;
use handlebars::Handlebars;
use handlebars::Renderable as _;
use serde::Serialize;
use std::path::Path;
use std::rc::Rc;
