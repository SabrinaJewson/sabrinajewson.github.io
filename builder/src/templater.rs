use crate::util::{
    asset::{self, Asset},
    log_errors,
};
use ::{
    anyhow::Context as _,
    handlebars::{template::Template, Handlebars},
    std::rc::Rc,
};

pub(crate) fn asset() -> impl Asset<Output = Rc<Handlebars<'static>>> {
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

            Ok(asset::all(includes).map(|includes| {
                let mut templater = Handlebars::new();
                for (name, include) in Vec::from(includes).into_iter().flatten() {
                    templater.register_template(&name, include);
                }
                templater
            }))
        })
        .map(|res| -> Rc<dyn Asset<Output = _>> {
            match res {
                Ok(asset) => Rc::new(asset),
                Err(e) => {
                    log::error!("{e:?}");
                    Rc::new(asset::Constant::new(Handlebars::new()))
                }
            }
        })
        .cache()
        .flatten()
        .map(Rc::new)
        .cache()
}
