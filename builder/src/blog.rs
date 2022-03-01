use crate::{
    asset::{self, Asset},
    markdown::{self, Markdown},
    minify,
    push_str::push,
    template::Template,
};
use ::{
    anyhow::Context as _,
    std::{
        cmp,
        path::{Path, PathBuf},
        rc::Rc,
    },
    syntect::highlighting::ThemeSet,
};

pub(crate) fn asset<'a>(in_dir: &'a Path, out_dir: &'a Path) -> impl Asset<Output = ()> + 'a {
    let post_template = Rc::new(
        asset::TextFile::new(in_dir.join("post.html"))
            .map(|src| anyhow::Ok(Template::new(src?)?))
            .map(|res| res.context("failed to load blog post template"))
            .map(Rc::new)
            .cache(),
    );

    let index_template = Rc::new(
        asset::TextFile::new(in_dir.join("index.html"))
            .map(|src| anyhow::Ok(Template::new(src?)?))
            .map(|res| res.context("failed to load blog index template"))
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
                        .map(move |src| Rc::new(read_post(stem.clone(), src)))
                        .cache(),
                );

                posts.push(post.clone());

                let post_page = asset::all((post, post_template.clone()))
                    .map(|(post, template)| build_post(&*post, (*template).as_ref()))
                    .to_file(output_path)
                    .map(log_errors);

                post_pages.push(post_page);
            }

            let post_pages = asset::all(post_pages)
                .map(|successes| successes.iter().copied().fold(Ok(()), Result::and));

            let index = asset::all((asset::all(posts), index_template.clone()))
                .map(|(mut posts, template)| build_index(&mut *posts, &*template))
                .to_file(out_dir.join("index.html"))
                .map(log_errors);

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
            match minify::css(&post_css) {
                Ok(minified) => minified,
                Err(e) => {
                    log::error!("{:?}", e.context("failed to minify post CSS"));
                    post_css
                }
            }
        })
        .to_file(out_dir.join("post.css"))
        .map(log_errors);

    asset::all((html, css)).map(|(html_success, css_success)| {
        if Result::and(html_success, css_success).is_ok() {
            log::info!("successfully emitted blog posts");
        }
    })
}

struct Post {
    stem: Rc<str>,
    content: anyhow::Result<Markdown>,
}

fn read_post(stem: Rc<str>, src: anyhow::Result<String>) -> Post {
    Post {
        content: src.map(|src| {
            let mut markdown = markdown::parse(&src);
            if markdown.title.is_empty() {
                log::warn!("Post in {stem}.md does not have title");
                markdown.title = format!("Untitled post from {stem}.md");
            }
            markdown
        }),
        stem,
    }
}

fn build_index(posts: &mut [Rc<Post>], template: &anyhow::Result<Template>) -> String {
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

    let mut ul = "<ul>".to_owned();
    for post in &*posts {
        ul.push_str("<li><a href='");
        ul.push_str(&post.stem);
        ul.push_str("'>");
        if let Ok(content) = &post.content {
            ul.push_str(&content.title);
            ul.push_str("</a> (");
            ul.push_str(content.published.as_deref().unwrap_or("draft"));
            ul.push(')');
        } else {
            log::error!("failed to generate post from {:?}.md", post.stem);
            push!(ul, "Error generating post from {:?}.md</a>", post.stem);
        }
        ul.push_str("</li>");
    }
    ul.push_str("</ul>");

    let mut html = String::new();
    template.apply(&mut html, [("list", &ul)]);

    match crate::minify::html(&html) {
        Ok(res) => html = res,
        Err(e) => log::error!("{:?}", e.context("failed to minify index file")),
    }

    html
}

fn build_post(post: &Post, template: Result<&Template, &anyhow::Error>) -> String {
    let (post_content, template) = match (&post.content, template) {
        (Ok(post), Ok(template)) => (post, template),
        (Ok(_), Err(e)) | (Err(e), Ok(_)) => return error_page([e]),
        (Err(e1), Err(e2)) => return error_page([e1, e2]),
    };

    let published = post_content.published.as_deref().unwrap_or("Draft");

    let mut html = String::new();
    template.apply(
        &mut html,
        [
            ("title", &post_content.title),
            ("published", published),
            ("outline", &post_content.outline),
            ("body", &post_content.body),
        ],
    );

    match crate::minify::html(&html) {
        Ok(res) => html = res,
        Err(e) => log::error!(
            "{:?}",
            e.context(format!("failed to minify {}", post_content.title))
        ),
    }

    html
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

fn log_errors(res: anyhow::Result<()>) -> Result<(), ()> {
    if let Err(e) = &res {
        log::error!("{:?}", e);
    }
    res.map_err(drop)
}

fn error_page<'a, I: IntoIterator<Item = &'a anyhow::Error>>(errors: I) -> String {
    let mut res = String::new();
    for error in errors {
        log::error!("{error:?}");
        push!(res, "<p style='color:red'>Error: {error:?}</p>");
    }
    res
}
