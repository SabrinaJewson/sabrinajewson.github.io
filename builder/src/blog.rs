use crate::{
    asset::{self, Asset},
    common_css, icons, minify,
    util::{
        error_page, log_errors,
        markdown::{self, Markdown},
        write_file,
    },
};
use ::{
    anyhow::Context as _,
    handlebars::{template::Template, Handlebars, Renderable as _},
    serde::{Serialize, Serializer},
    std::{
        cmp,
        path::{Path, PathBuf},
        rc::Rc,
    },
    syntect::highlighting::ThemeSet,
};

pub(crate) fn asset<'a>(
    in_dir: &'a Path,
    out_dir: &'a Path,
    templater: impl Asset<Output = Rc<Handlebars<'static>>> + Clone + 'a,
    drafts: bool,
) -> impl Asset<Output = ()> + 'a {
    let post_template = Rc::new(
        asset::TextFile::new(in_dir.join("post.hbs"))
            .map(|src| Template::compile(&*src?).context("failed to compile blog post template"))
            .map(Rc::new)
            .cache(),
    );

    let index_template = Rc::new(
        asset::TextFile::new(in_dir.join("index.hbs"))
            .map(|src| Template::compile(&*src?).context("failed to compile blog index template"))
            .map(Rc::new)
            .cache(),
    );

    let html = asset::Dir::new(in_dir)
        .map(move |files| -> anyhow::Result<_> {
            // TODO: Whenever the directory is changed at all, this entire bit of code is re-run
            // which throws away all the old `Asset`s.
            // That's a problem because we loes all our in-memory cache.

            let mut posts = Vec::new();
            let mut post_pages = Vec::new();

            for path in files? {
                let path = path?;
                if path.extension() != Some("md".as_ref()) {
                    continue;
                }

                let stem = if let Some(s) = path.file_stem().unwrap().to_str() {
                    <Rc<str>>::from(s)
                } else {
                    log::error!("filename `{}` is not valid UTF-8", path.display());
                    continue;
                };

                let mut output_path = out_dir.join(&*stem);
                output_path.set_extension("html");

                let post = Rc::new(
                    asset::TextFile::new(path)
                        .map(move |src| read_post(stem.clone(), src, drafts).map(Rc::new))
                        .cache(),
                );

                posts.push(post.clone());

                let post_page = asset::all((post, templater.clone(), post_template.clone()))
                    .map({
                        let output_path = output_path.clone();
                        move |(post, templater, template)| {
                            if let Some(post) = post {
                                let built = build_post(&post, &*templater, (*template).as_ref());
                                log_errors(write_file(&output_path, built))?;
                            }
                            Ok(())
                        }
                    })
                    .modifies_path(output_path);

                post_pages.push(post_page);
            }

            let post_pages = asset::all(post_pages)
                .map(|successes| successes.iter().copied().fold(Ok(()), Result::and));

            let index = asset::all((asset::all(posts), templater.clone(), index_template.clone()))
                .map(|(posts, templater, template)| {
                    // Remove drafts from the index
                    let posts = Vec::from(posts).into_iter().flatten().collect();
                    let index = build_index(posts, &*templater, &*template);
                    log_errors(write_file(out_dir.join("index.html"), index))
                })
                .modifies_path(out_dir.join("index.html"));

            Ok(asset::all((post_pages, index))
                .map(|(blog_success, index_success)| Result::and(blog_success, index_success)))
        })
        .map(|res| -> Rc<dyn Asset<Output = _>> {
            match res {
                Ok(asset) => Rc::new(asset),
                Err(e) => {
                    log::error!("{:?}", e);
                    Rc::new(asset::Constant::new(Err(())))
                }
            }
        })
        .cache()
        .flatten();

    let post_css = asset::TextFile::new(in_dir.join("post.css"))
        .map(|res| res.map_err(|e| log::error!("{e:?}")).unwrap_or_default());

    let code_themes_dir = in_dir.join("code_themes");
    let dark_theme = theme_asset(code_themes_dir.join("dark.tmTheme"));
    let light_theme = theme_asset(code_themes_dir.join("light.tmTheme"));

    let css = asset::all((post_css, light_theme, dark_theme))
        .map(|(mut post_css, light_theme, dark_theme)| {
            post_css.push_str(&**dark_theme);
            post_css.push_str("@media(prefers-color-scheme:light){");
            post_css.push_str(&**light_theme);
            post_css.push('}');
            let css = match minify::css(&post_css) {
                Ok(minified) => minified,
                Err(e) => {
                    log::error!("{:?}", e.context("failed to minify post CSS"));
                    post_css
                }
            };
            log_errors(write_file(out_dir.join(POST_CSS_PATH), css))
        })
        .modifies_path(out_dir.join(POST_CSS_PATH));

    asset::all((html, css)).map(|(html_success, css_success)| {
        if Result::and(html_success, css_success).is_ok() {
            log::info!("successfully emitted blog posts");
        }
    })
}

const POST_CSS_PATH: &str = "post.css";

// Serialization used in the templates
#[derive(Serialize)]
struct Post {
    stem: Rc<str>,
    #[serde(
        skip_serializing_if = "Result::is_err",
        serialize_with = "serialize_unwrap"
    )]
    content: anyhow::Result<Markdown>,
}

fn read_post(stem: Rc<str>, src: anyhow::Result<String>, drafts: bool) -> Option<Post> {
    let post = Post {
        content: src.map(|src| {
            let mut markdown = markdown::parse(&src);
            if markdown.title.is_empty() {
                log::warn!("Post in {stem}.md does not have title");
                markdown.title = format!("Untitled post from {stem}.md");
            }
            markdown
        }),
        stem,
    };
    if !drafts
        && post
            .content
            .as_ref()
            .map_or(false, |content| content.published.is_none())
    {
        None
    } else {
        Some(post)
    }
}

fn build_index(
    mut posts: Vec<Rc<Post>>,
    templater: &Handlebars<'static>,
    template: &anyhow::Result<Template>,
) -> String {
    let template = match template {
        Ok(template) => template,
        Err(e) => return error_page([e]),
    };

    posts.sort_unstable_by(|a, b| match (&a.content, &b.content) {
        (Ok(a_content), Ok(b_content)) => match (&a_content.published, &b_content.published) {
            (Some(a_date), Some(b_date)) => b_date.cmp(a_date),
            // Posts without a date should sort before those with one
            (Some(_), None) => cmp::Ordering::Greater,
            (None, Some(_)) => cmp::Ordering::Less,
            (None, None) => a.stem.cmp(&b.stem),
        },
        // `Ok`s should sort after `Err`s
        (Ok(_), Err(_)) => cmp::Ordering::Greater,
        (Err(_), Ok(_)) => cmp::Ordering::Less,
        (Err(_), Err(_)) => a.stem.cmp(&b.stem),
    });

    #[derive(Serialize)]
    struct TemplateVars<'a> {
        posts: &'a [Rc<Post>],
        icons: icons::Paths,
        common_css: &'static str,
    }
    let context = handlebars::Context::wraps(TemplateVars {
        posts: &*posts,
        icons: icons::PATHS,
        common_css: common_css::PATH,
    })
    .unwrap();

    let mut render_context = handlebars::RenderContext::new(None);
    let res = template
        .renders(templater, &context, &mut render_context)
        .context("failed to render blog index template");
    let rendered = match res {
        Ok(rendered) => rendered,
        Err(e) => return error_page([&e]),
    };

    match crate::minify::html(&rendered) {
        Ok(minified) => minified,
        Err(e) => {
            log::error!("{:?}", e.context("failed to minify index file"));
            rendered
        }
    }
}

fn build_post(
    post: &Post,
    templater: &Handlebars<'static>,
    template: Result<&Template, &anyhow::Error>,
) -> String {
    let (post_content, template) = match (&post.content, template) {
        (Ok(post), Ok(template)) => (post, template),
        (Ok(_), Err(e)) | (Err(e), Ok(_)) => return error_page([e]),
        (Err(e1), Err(e2)) => return error_page([e1, e2]),
    };

    #[derive(Serialize)]
    struct TemplateVars<'a> {
        post: &'a Markdown,
        icons: icons::Paths,
        common_css: &'static str,
        post_css: &'static str,
    }
    let context = handlebars::Context::wraps(TemplateVars {
        post: post_content,
        icons: icons::PATHS,
        common_css: common_css::PATH,
        post_css: POST_CSS_PATH,
    })
    .unwrap();

    let mut render_context = handlebars::RenderContext::new(None);
    let res = template
        .renders(templater, &context, &mut render_context)
        .context("failed to render blog post template");
    let rendered = match res {
        Ok(rendered) => rendered,
        Err(e) => return error_page([&e]),
    };

    match crate::minify::html(&rendered) {
        Ok(minified) => minified,
        Err(e) => {
            log::error!(
                "{:?}",
                e.context(format!("failed to minify {}", post_content.title))
            );
            rendered
        }
    }
}

fn theme_asset(path: PathBuf) -> impl Asset<Output = Rc<String>> {
    asset::FsPath::new(path.clone())
        .map(move |()| {
            let res = ThemeSet::get_theme(&path)
                .with_context(|| format!("failed to read theme file {}", path.display()));
            Rc::new(match res {
                Ok(theme) => markdown::theme_css(&theme),
                Err(e) => {
                    log::error!("{e:?}");
                    String::new()
                }
            })
        })
        .cache()
}

fn serialize_unwrap<S, T, E>(result: &Result<T, E>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    result
        .as_ref()
        .unwrap_or_else(|_| panic!())
        .serialize(serializer)
}
