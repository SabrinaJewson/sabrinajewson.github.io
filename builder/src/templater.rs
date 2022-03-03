use crate::{
    common_css, icons,
    util::{
        asset::{self, Asset},
        log_errors,
    },
};
use ::{
    anyhow::Context as _,
    fn_error_context::context,
    handlebars::{template::Template, Handlebars, Renderable as _},
    serde::Serialize,
    std::rc::Rc,
};

#[derive(Clone)]
pub(crate) struct Templater {
    handlebars: Rc<Handlebars<'static>>,
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
            icons: icons::Paths,
            common_css: &'static str,
        }

        let vars = TemplateVars {
            rest: vars,
            icons: icons::PATHS,
            common_css: common_css::PATH,
        };
        let context = handlebars::Context::wraps(vars).unwrap();

        let mut render_context = handlebars::RenderContext::new(None);
        Ok(template.renders(&*self.handlebars, &context, &mut render_context)?)
    }
}

thread_local! {
    static FALLBACK_TEMPLATER: Templater = Templater {
        handlebars: Rc::new(Handlebars::new()),
    };
}

pub(crate) fn asset() -> impl Asset<Output = Templater> {
    asset::Dir::new("template_include")
        .map(|files| -> anyhow::Result<_> {
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
                        let template = Template::compile(&*source?)
                            .with_context(|| format!("failed to compile template {name}"))?;
                        Ok((name.clone(), template))
                    })
                    .map(log_errors)
                    .cache();

                includes.push(include);
            }

            Ok(asset::all(includes)
                .map(|includes| {
                    let mut handlebars = Handlebars::new();
                    for (name, include) in Vec::from(includes).into_iter().flatten() {
                        handlebars.register_template(&name, include);
                    }
                    Templater {
                        handlebars: Rc::new(handlebars),
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
