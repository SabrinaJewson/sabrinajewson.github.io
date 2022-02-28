use crate::asset::{self, Asset};
use ::{
    anyhow::{bail, Context as _},
    fn_error_context::context,
    std::{ops::Range, path::Path, rc::Rc},
};

pub(crate) fn asset<'a>(in_dir: &'a Path, out_dir: &'a Path) -> impl Asset<Output = ()> + 'a {
    let post_template = Rc::new(
        asset::TextFile::new(in_dir.join("post.html"))
            .and_then(|src| Ok(Rc::new(Template::new(src)?)))
            .cache(),
    );

    let index_template = Rc::new(
        asset::TextFile::new(in_dir.join("index.html"))
            .and_then(|src| Ok(Rc::new(Template::new(src)?)))
            .cache(),
    );

    asset::Dir::new(in_dir)
        .and_then(move |files| {
            // TODO: Whenever the directory is changed at all, this entire bit of code is re-run
            // which throws away all the old `Asset`s.
            // That's a problem because we loes all our in-memory cache.

            let mut post_assets = Vec::new();
            let mut html_assets = Vec::new();

            for path in files {
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

                let post_asset = Rc::new(
                    asset::TextFile::new(path)
                        .and_then(move |src| read_post(stem.clone(), &src).map(Rc::new))
                        .cache(),
                );

                post_assets.push(post_asset.clone());

                let html_asset = (post_asset, post_template.clone())
                    .and_then(|(post, template)| build_post(&post, &template))
                    .to_file(output_path);

                html_assets.push(html_asset);
            }

            let index_asset = (post_assets, index_template.clone())
                .and_then(|(mut posts, template)| build_index(&mut posts, &template))
                .to_file(out_dir.join("index.html"));

            Ok(Rc::new((html_assets, index_asset)))
        })
        .cache()
        .and_then(|rc| {
            let mut ok = true;
            let (post_assets, index_asset) = &*rc;
            for asset in &*post_assets {
                if let Err(e) = asset.generate() {
                    log::error!("skipping generating blog post: {:?}\n", e);
                    ok = false;
                }
            }
            if let Err(e) = index_asset.generate().context("failed to generate index") {
                log::error!("{:?}\n", e);
                ok = false;
            }

            if ok {
                log::info!("succesfully build posts and index");
            }

            Ok(())
        })
}

struct Post {
    stem: Rc<str>,
    published: Box<str>,
    title: String,
    body: String,
    outline: String,
}

fn read_post(stem: Rc<str>, src: &str) -> anyhow::Result<Post> {
    let markdown = crate::markdown::parse(src)?;
    Ok(Post {
        stem,
        published: markdown
            .published
            .with_context(|| format!("post '{}' has no publish date", markdown.title))?,
        title: markdown.title,
        body: markdown.body,
        outline: markdown.outline,
    })
}

fn build_index(posts: &mut [Rc<Post>], template: &Template) -> anyhow::Result<String> {
    posts.sort_by(|a, b| b.published.cmp(&a.published));

    let mut ul = "<ul>".to_owned();
    for post in &*posts {
        ul.push_str("<li><a href='");
        ul.push_str(&post.stem);
        ul.push_str("'>");
        ul.push_str(&post.title);
        ul.push_str("</a> (");
        ul.push_str(&post.published);
        ul.push_str(")</li>");
    }
    ul.push_str("</ul>");

    let mut html = String::new();
    template
        .apply(&mut html, |var_name, output| {
            if var_name == "list" {
                output.push_str(&ul);
            } else {
                bail!("no known variable `{}`", var_name);
            }
            Ok(())
        })
        .context("failed to apply template to blog index")?;

    crate::minify::html(&mut html)?;

    Ok(html)
}

fn build_post(post: &Post, template: &Template) -> anyhow::Result<String> {
    let mut html = String::new();
    template
        .apply(&mut html, |var_name, output| {
            match var_name {
                "title" => output.push_str(&post.title),
                "body" => output.push_str(&post.body),
                "outline" => output.push_str(&post.outline),
                _ => bail!("no known variable `{}`", var_name),
            }
            Ok(())
        })
        .context("failed to apply template to post")?;

    crate::minify::html(&mut html)?;

    Ok(html)
}

struct Template {
    origin: String,
    substitutions: Vec<Range<usize>>,
}

impl Template {
    #[context("failed to parse template")]
    fn new(origin: String) -> anyhow::Result<Self> {
        let mut substitutions = Vec::new();

        let mut bytes = origin.as_bytes();
        while let Some(start) = memchr::memchr(b'\\', bytes) {
            let end = match bytes.get(start + 1).context("trailing backslash")? {
                b'\\' => start + 2,
                b'{' => memchr::memchr(b'}', bytes).context("no closing `}`")? + 1,
                &c => bail!("unexpected character '{}' after backslash", char::from(c)),
            };

            bytes = &bytes[end..];
            substitutions.push(start..end);
        }

        Ok(Self {
            origin,
            substitutions,
        })
    }

    fn apply<E>(
        &self,
        output: &mut String,
        mut var: impl FnMut(&str, &mut String) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut rest = &*self.origin;
        for substitution in &self.substitutions {
            output.push_str(&rest[..substitution.start]);
            match rest.as_bytes()[substitution.start + 1] {
                b'\\' => output.push('\\'),
                b'{' => var(&rest[substitution.start + 2..substitution.end - 1], output)?,
                _ => unreachable!(),
            }
            rest = &rest[substitution.end..];
        }
        output.push_str(rest);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Template;
    use ::std::convert::Infallible;

    #[track_caller]
    fn template(src: &str, mut var: impl FnMut(&str) -> String) -> String {
        let mut output = String::new();
        let template = Template::new(src.to_owned()).unwrap();
        template
            .apply(&mut output, |name, output| {
                output.push_str(&var(name));
                Ok::<_, Infallible>(())
            })
            .unwrap();
        output
    }

    #[test]
    fn templating() {
        assert_eq!(template(r"", |_| panic!()), r"");
        assert_eq!(template(r"simple", |_| panic!()), r"simple");
        assert_eq!(template(r"foo\\", |_| panic!()), r"foo\");
        assert_eq!(template(r"foo\\bar", |_| panic!()), r"foo\bar");
        assert_eq!(template(r"\\bar", |_| panic!()), r"\bar");
        assert_eq!(
            template(r"\{best programming lang}", |s| {
                assert_eq!(s, "best programming lang");
                "rust".to_owned()
            }),
            r"rust"
        );
        assert_eq!(
            template(r":\{best programming lang}:", |s| {
                assert_eq!(s, "best programming lang");
                "rust".to_owned()
            }),
            r":rust:"
        );
        let mut count = 0;
        assert_eq!(
            template(r"\{1} text here \{2}", |s| {
                let res = match count {
                    0 => {
                        assert_eq!(s, "1");
                        "one"
                    }
                    1 => {
                        assert_eq!(s, "2");
                        "two"
                    }
                    _ => panic!(),
                };
                count += 1;
                res.to_owned()
            }),
            r"one text here two"
        );
    }
}
