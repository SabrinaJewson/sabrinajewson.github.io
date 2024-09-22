pub(crate) fn asset<'a>(
    template_dir: &'a Path,
    src_dir: &'a Path,
    out_dir: &'a Path,
    templater: impl Asset<Output = Templater> + Clone + 'a,
    config: impl Asset<Output = &'a Config> + Copy + 'a,
) -> impl Asset<Output = ()> + 'a {
    let post_template = Rc::new(
        asset::TextFile::new(template_dir.join("post.hbs"))
            .map(|src| Template::compile(&src?).context("failed to compile blog post template"))
            .map(Rc::new)
            .cache(),
    );

    let index_template = Rc::new(
        asset::TextFile::new(template_dir.join("index.hbs"))
            .map(|src| Template::compile(&src?).context("failed to compile blog index template"))
            .map(Rc::new)
            .cache(),
    );

    let feed_metadata = Rc::new(
        asset::TextFile::new(template_dir.join("feed.json"))
            .map(|src| {
                serde_json::from_str::<FeedMetadata>(&src?).context("failed to read feed.json")
            })
            .map(|res| res.map(Rc::new).map_err(|e| log::error!("{e:?}")))
            .cache(),
    );

    let html = asset::Dir::new(src_dir)
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

                let post = asset::TextFile::new(path)
                    .map(move |src| Rc::new(read_post(stem.clone(), src)))
                    .cache();

                let post = Rc::new(asset::all((config, post)).map(move |(config, post)| {
                    (config.drafts || !post.is_draft()).then_some(post)
                }));

                posts.push(post.clone());

                let post_page = asset::all((post, templater.clone(), post_template.clone()))
                    .map({
                        let output_path = output_path.clone();
                        move |(post, templater, template)| {
                            if let Some(post) = post {
                                let built = build_post(&post, &templater, (*template).as_ref())
                                    .unwrap_or_else(ErrorPage::into_html);
                                write_file(&output_path, built)?;
                                log::info!("successfully emitted {}.html", post.stem);
                            }
                            Ok(())
                        }
                    })
                    .map(log_errors)
                    .modifies_path(output_path);

                post_pages.push(post_page);
            }

            let posts = Rc::new(asset::all(posts).map(process_posts).cache());

            let feed = asset::all((posts.clone(), feed_metadata.clone()))
                .map(|(posts, metadata)| {
                    let Ok(metadata) = metadata else {
                        return Ok(());
                    };
                    let feed = build_feed(&posts, &metadata);
                    write_file(out_dir.join(FEED_PATH), feed)?;
                    log::info!("successfully emitted Atom feed");
                    Ok(())
                })
                .map(log_errors)
                .modifies_path(out_dir.join(FEED_PATH));

            let index = asset::all((posts, templater.clone(), index_template.clone()))
                .map(|(posts, templater, template)| {
                    let index = build_index(&posts, &templater, &template)
                        .unwrap_or_else(ErrorPage::into_html);
                    write_file(out_dir.join("index.html"), index)?;
                    log::info!("successfully emitted blog index");
                    Ok(())
                })
                .map(log_errors)
                .modifies_path(out_dir.join("index.html"));

            Ok(asset::all((asset::all(post_pages), feed, index)).map(|_| {}))
        })
        .map(|res| -> Rc<dyn Asset<Output = _>> {
            match res {
                Ok(asset) => Rc::new(asset),
                Err(e) => {
                    log::error!("{:?}", e);
                    Rc::new(asset::Constant::new(()))
                }
            }
        })
        .cache()
        .flatten();

    let post_css = asset::TextFile::new(template_dir.join("post.css")).map(|res| {
        res.unwrap_or_else(|e| {
            log::error!("{e:?}");
            String::new()
        })
    });

    let code_themes_dir = template_dir.join("code_themes");
    let dark_theme = theme_asset(code_themes_dir.join("dark.tmTheme"));
    let light_theme = theme_asset(code_themes_dir.join("light.tmTheme"));

    let css = asset::all((post_css, light_theme, dark_theme, config))
        .map(|(mut post_css, light_theme, dark_theme, config)| {
            post_css.push_str(&dark_theme);
            post_css.push_str("@media(prefers-color-scheme:light){");
            post_css.push_str(&light_theme);
            post_css.push('}');
            if config.minify {
                minify(minify::FileType::Css, &mut post_css);
            }
            write_file(out_dir.join(POST_CSS_PATH), post_css)?;
            log::info!("successfully emitted post CSS");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out_dir.join(POST_CSS_PATH));

    asset::all((html, css)).map(|((), ())| {})
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
    content: anyhow::Result<PostContent>,
}

impl Post {
    fn is_draft(&self) -> bool {
        self.content
            .as_ref()
            .map_or(false, |content| content.metadata.published.is_none())
    }
}

#[derive(Serialize)]
struct PostContent {
    metadata: PostMetadata,
    markdown: Markdown,
}

#[derive(Default, Serialize, Deserialize)]
struct PostMetadata {
    published: Option<NaiveDate>,
    updated: Option<NaiveDate>,
}

fn read_post(stem: Rc<str>, src: anyhow::Result<String>) -> Post {
    Post {
        content: src.map(|src| {
            let mut json = serde_json::Deserializer::from_str(&src).into_iter();
            let metadata = json.next().and_then(Result::ok).unwrap_or_default();
            let markdown = &src[json.byte_offset()..];

            let mut markdown = markdown::parse(markdown);
            if markdown.title.is_empty() {
                log::warn!("Post in {stem}.md does not have title");
                markdown.title = format!("Untitled post from {stem}.md");
            }
            PostContent { metadata, markdown }
        }),
        stem,
    }
}

fn process_posts(posts: Box<[Option<Rc<Post>>]>) -> Rc<Vec<Rc<Post>>> {
    // Remove disabled posts: drafts when they are disabled
    let mut posts: Vec<_> = Vec::from(posts).into_iter().flatten().collect();

    posts.sort_unstable_by(|a, b| match (&a.content, &b.content) {
        (Ok(a_content), Ok(b_content)) => {
            match (&a_content.metadata.published, &b_content.metadata.published) {
                (Some(a_date), Some(b_date)) => b_date.cmp(a_date),
                // Posts without a date should sort before those with one
                (Some(_), None) => cmp::Ordering::Greater,
                (None, Some(_)) => cmp::Ordering::Less,
                // Between drafts, sort alphabetically
                (None, None) => a.stem.cmp(&b.stem),
            }
        }
        // `Ok`s should sort after `Err`s
        (Ok(_), Err(_)) => cmp::Ordering::Greater,
        (Err(_), Ok(_)) => cmp::Ordering::Less,
        // Between errored posts, sort alphabetically
        (Err(_), Err(_)) => a.stem.cmp(&b.stem),
    });

    Rc::new(posts)
}

#[derive(Deserialize)]
struct FeedMetadata {
    site: String,
    url: String,
    title: String,
    name: String,
}

const FEED_PATH: &str = "feed.xml";

fn build_feed(posts: &[Rc<Post>], metadata: &FeedMetadata) -> String {
    fn datetime(date: NaiveDate) -> DateTime<chrono::offset::FixedOffset> {
        chrono::offset::Utc
            .from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .into()
    }

    let mut feed = atom_syndication::FeedBuilder::default();

    feed.title(&*metadata.title);
    feed.id(metadata.url.clone());

    // Last updated is the date of the lastest post
    if let Some(updated) = posts
        .iter()
        .filter_map(|post| post.content.as_ref().ok()?.metadata.published.map(datetime))
        .max()
    {
        feed.updated(updated);
    }

    feed.author(
        atom_syndication::PersonBuilder::default()
            .name(metadata.name.clone())
            .uri(Some(metadata.site.clone()))
            .build(),
    );

    let generator = atom_syndication::GeneratorBuilder::default()
        .value("sabrinajewson.github.io".to_owned())
        .uri(Some(
            "https://github.com/SabrinaJewson/sabrinajewson.github.io".to_owned(),
        ))
        .build();
    feed.generator(Some(generator));

    feed.icon(Some(format!(
        "{}/{}",
        metadata.site,
        crate::icons::PATHS.apple_touch_icon
    )));

    // self-link
    feed.link(
        atom_syndication::LinkBuilder::default()
            .href(format!("{}{FEED_PATH}", metadata.url))
            .rel("self".to_owned())
            .mime_type(Some("application/atom+xml".to_owned()))
            .build(),
    );

    // HTML link
    feed.link(
        atom_syndication::LinkBuilder::default()
            .href(metadata.url.clone())
            .rel("alternate".to_owned())
            .mime_type(Some("text/html".to_owned()))
            .build(),
    );

    for post in posts.iter().take(10) {
        let Ok(content) = &post.content else { continue };
        let Some(published) = content.metadata.published.map(datetime) else {
            continue;
        };

        let post_url = format!("{}{}", metadata.url, post.stem);

        feed.entry(
            atom_syndication::EntryBuilder::default()
                .title(&*content.markdown.title)
                .id(post_url.clone())
                .link(
                    atom_syndication::LinkBuilder::default()
                        .href(post_url.clone())
                        .mime_type(Some("text/html".to_owned()))
                        .title(Some(content.markdown.title.clone()))
                        .build(),
                )
                .published(published)
                .updated(content.metadata.updated.map_or(published, datetime))
                .content(
                    atom_syndication::ContentBuilder::default()
                        .base(Some(post_url))
                        .value(Some(content.markdown.body.clone()))
                        .content_type(Some("html".to_owned()))
                        .build(),
                )
                .build(),
        );
    }

    feed.lang(Some("en".to_owned()));

    feed.build().to_string()
}

fn build_index(
    posts: &[Rc<Post>],
    templater: &Templater,
    template: &anyhow::Result<Template>,
) -> Result<String, ErrorPage> {
    #[derive(Serialize)]
    struct TemplateVars<'a> {
        posts: &'a [Rc<Post>],
        feed: &'static str,
    }
    let vars = TemplateVars {
        posts,
        feed: FEED_PATH,
    };
    Ok(templater.render(template.as_ref()?, vars)?)
}

fn build_post(
    post: &Post,
    templater: &Templater,
    template: Result<&Template, &anyhow::Error>,
) -> Result<String, ErrorPage> {
    let (post_content, template) = ErrorPage::zip(post.content.as_ref(), template)?;

    #[derive(Serialize)]
    struct TemplateVars<'a> {
        post: &'a PostContent,
        post_css: &'static str,
        feed: &'static str,
    }
    let vars = TemplateVars {
        post: post_content,
        post_css: POST_CSS_PATH,
        feed: FEED_PATH,
    };

    Ok(templater.render(template, vars)?)
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

use crate::config::Config;
use crate::templater::Templater;
use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use crate::util::markdown;
use crate::util::markdown::Markdown;
use crate::util::minify;
use crate::util::minify::minify;
use crate::util::write_file;
use crate::util::ErrorPage;
use anyhow::Context as _;
use chrono::naive::NaiveDate;
use chrono::offset::TimeZone as _;
use chrono::DateTime;
use handlebars::template::Template;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use std::cmp;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use syntect::highlighting::ThemeSet;
