use crate::push_str::{escape_href, escape_html, push, PushStr};
use ::{
    anyhow::{bail, ensure, Context as _},
    std::{
        borrow::Cow,
        collections::HashSet,
        hash::{Hash, Hasher},
    },
};

#[cfg_attr(test, derive(Debug, PartialEq))]
pub(crate) struct Markdown {
    pub(crate) published: Option<Box<str>>,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) outline: String,
}

pub(crate) fn parse(source: &str) -> anyhow::Result<Markdown> {
    let (published, markdown) = if let Some(rest) = source.strip_prefix("published: ") {
        let (published, rest) = rest
            .split_once('\n')
            .context("unexpected EOF after publish date")?;

        (
            Some(published),
            Cow::Owned(rest.replace("[published]", published)),
        )
    } else {
        (None, Cow::Borrowed(source))
    };

    let options = pulldown_cmark::Options::empty()
        | pulldown_cmark::Options::ENABLE_TABLES
        | pulldown_cmark::Options::ENABLE_HEADING_ATTRIBUTES
        | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
        | pulldown_cmark::Options::ENABLE_SMART_PUNCTUATION;

    Renderer {
        published,
        title: String::new(),
        parser: pulldown_cmark::Parser::new_ext(&markdown, options),
        body: String::new(),
        in_table_head: false,
        used_classes: HashSet::new(),
        outline: String::new(),
        outline_level: 0,
        in_heading: false,
    }
    .render()
    .context("failed to render markdown")
}

struct Renderer<'input> {
    published: Option<&'input str>,
    title: String,
    parser: pulldown_cmark::Parser<'input, 'input>,
    body: String,
    /// Whether we are in a `<thead>`.
    /// Used to determine whether to output `<td>`s or `<th>`s.
    in_table_head: bool,
    /// Class names that need to be generated in the resulting CSS.
    used_classes: HashSet<Class>,
    outline: String,
    /// The level of the currently opened heading `<li>` in the outline.
    /// In the range [0..6].
    outline_level: u8,
    /// Whether we are in a `<hN>` tag.
    /// Used to determine whether to also write to the title and the outline.
    in_heading: bool,
}

impl<'input> Renderer<'input> {
    fn render(mut self) -> anyhow::Result<Markdown> {
        while let Some(event) = self.parser.next() {
            match event {
                pulldown_cmark::Event::Start(tag) => self.start_tag(tag)?,
                pulldown_cmark::Event::End(tag) => self.end_tag(tag),
                pulldown_cmark::Event::Text(text) => escape_html(&mut self, &text),
                pulldown_cmark::Event::Code(text) => {
                    if let Some((_option, _rest)) =
                        text.strip_prefix('[').and_then(|rest| rest.split_once(']'))
                    {
                        todo!("syntax highlighting")
                    } else {
                        self.push_str("<code>");
                        escape_html(&mut self, &text);
                        self.push_str("</code>");
                    }
                }
                pulldown_cmark::Event::Html(html) => self.push_str(&html),
                pulldown_cmark::Event::SoftBreak => self.push_str(" "),
                pulldown_cmark::Event::HardBreak => self.push_str("<br>"),
                pulldown_cmark::Event::Rule => self.push_str("<hr>"),
                // We do not enable these extensions
                pulldown_cmark::Event::FootnoteReference(_)
                | pulldown_cmark::Event::TaskListMarker(_) => unreachable!(),
            }
        }

        assert!(!self.in_table_head);
        assert!(!self.in_heading);

        // Close remaining opened tags in the outline.
        for _ in 0..self.outline_level {
            self.outline.push_str("</li></ul>");
        }

        if !self.used_classes.is_empty() {
            self.push_str("<style>");
            for class in &self.used_classes {
                class.write_definition(&mut self.body);
            }
            self.push_str("</style>");
        }

        Ok(Markdown {
            published: self.published.map(Into::into),
            title: self.title,
            body: self.body,
            outline: self.outline,
        })
    }

    fn start_tag(&mut self, tag: pulldown_cmark::Tag<'input>) -> anyhow::Result<()> {
        match tag {
            pulldown_cmark::Tag::Paragraph => self.push_str("<p>"),
            pulldown_cmark::Tag::Heading(level, id, classes) => {
                ensure!(classes.is_empty(), "heading classes are disallowed");
                let id = id.context("heading does not have ID")?;
                push!(self, "<{} id='", level);
                escape_html(self, id);
                self.push_str("'>");

                let level = level as u8;

                // Update the outline.
                if level == self.outline_level {
                    // If this next heading is on the same level, just close the previous one.
                    self.outline.push_str("</li>");
                } else if level == self.outline_level + 1 {
                    // If it is on the next level, open an new `<ul>` tag.
                    self.outline.push_str("<ul>");
                } else if level + 1 == self.outline_level {
                    // If it is on the previous level, close the tags.
                    self.outline.push_str("</li></ul></li>");
                } else {
                    bail!(
                        "heading level jump of > 1: {} to {}",
                        self.outline_level,
                        level
                    );
                }

                self.outline.push_str("<li><a href='#");
                escape_href(&mut self.outline, id);
                self.outline.push_str("'>");

                self.outline_level = level;
                self.in_heading = true;
            }
            pulldown_cmark::Tag::Table(alignments) => {
                if alignments
                    .iter()
                    .all(|&align| align == pulldown_cmark::Alignment::None)
                {
                    self.push_str("<table>");
                } else {
                    let class = Class::Table(TableAlignments(alignments));
                    self.push_str("<table class='");
                    class.write_name(self);
                    self.push_str("'>");
                    self.used_classes.insert(class);
                }
            }
            pulldown_cmark::Tag::TableHead => {
                self.push_str("<thead><tr>");
                self.in_table_head = true;
            }
            pulldown_cmark::Tag::TableRow => self.push_str("<tr>"),
            pulldown_cmark::Tag::TableCell => {
                self.push_str(match self.in_table_head {
                    true => "<th>",
                    false => "<td>",
                });
            }
            pulldown_cmark::Tag::BlockQuote => self.push_str("<blockquote>"),
            pulldown_cmark::Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(info)) => {
                let lang = info.split(' ').next().unwrap();
                if lang.is_empty() {
                    self.push_str("<pre><code>");
                } else {
                    todo!("syntax highlighting")
                }
            }
            pulldown_cmark::Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Indented) => {
                self.push_str("<pre><code>");
            }
            pulldown_cmark::Tag::List(Some(1)) => self.push_str("<ol>"),
            pulldown_cmark::Tag::List(Some(start)) => {
                push!(self, "<ol start='{}'>", start);
            }
            pulldown_cmark::Tag::List(None) => self.push_str("<ul>"),
            pulldown_cmark::Tag::Item => self.push_str("<li>"),
            pulldown_cmark::Tag::Emphasis => self.push_str("<em>"),
            pulldown_cmark::Tag::Strong => self.push_str("<strong>"),
            pulldown_cmark::Tag::Strikethrough => self.push_str("<del>"),
            pulldown_cmark::Tag::Link(pulldown_cmark::LinkType::Email, ..) => {
                bail!("email links are not supported yet");
            }
            pulldown_cmark::Tag::Link(_type, href, title) => {
                self.push_str("<a href='");
                escape_href(self, &href);
                if !title.is_empty() {
                    self.push_str("' title='");
                    escape_html(self, &title);
                }
                self.push_str("'>");
            }
            pulldown_cmark::Tag::Image(_, url, title) => {
                self.push_str("<img src='");
                escape_href(self, &url);
                self.push_str("' alt='");
                while let Some(event) = self.parser.next() {
                    match event {
                        pulldown_cmark::Event::End(_) => break,
                        pulldown_cmark::Event::Text(text) => escape_html(self, &text),
                        // FIXME: soft breaks, hard breaks => ' '
                        _ => unreachable!(),
                    }
                }
                if !title.is_empty() {
                    self.push_str("' title='");
                    escape_html(self, &title);
                }
                self.push_str("'>");
            }
            // We do not enable this extension
            pulldown_cmark::Tag::FootnoteDefinition(_) => unreachable!(),
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: pulldown_cmark::Tag<'input>) {
        match tag {
            pulldown_cmark::Tag::Paragraph => {
                self.push_str("</p>");
            }
            pulldown_cmark::Tag::Heading(level, _id, _classes) => {
                self.in_heading = false;

                self.outline.push_str("</a>");

                // TODO: anchor links
                self.push_str("</");
                push!(self, "{}", level);
                self.push_str(">");
            }
            pulldown_cmark::Tag::Table(_) => {
                self.push_str("</tbody></table>");
            }
            pulldown_cmark::Tag::TableHead => {
                self.push_str("</tr></thead><tbody>");
                self.in_table_head = false;
            }
            pulldown_cmark::Tag::TableRow => {
                self.push_str("</tr>");
            }
            pulldown_cmark::Tag::TableCell => {
                self.push_str(match self.in_table_head {
                    true => "</th>",
                    false => "</td>",
                });
            }
            pulldown_cmark::Tag::BlockQuote => self.push_str("</blockquote>"),
            pulldown_cmark::Tag::CodeBlock(_) => self.push_str("</code></pre>"),
            pulldown_cmark::Tag::List(Some(_)) => self.push_str("</ol>"),
            pulldown_cmark::Tag::List(None) => self.push_str("</ul>"),
            pulldown_cmark::Tag::Item => self.push_str("</li>"),
            pulldown_cmark::Tag::Emphasis => self.push_str("</em>"),
            pulldown_cmark::Tag::Strong => self.push_str("</strong>"),
            pulldown_cmark::Tag::Strikethrough => self.push_str("</del>"),
            pulldown_cmark::Tag::Link(_, _, _) => self.push_str("</a>"),
            // Image tag closing is handled by the opening logic, since alt tags aren't HTML
            pulldown_cmark::Tag::Image(_, _, _)
                // We do not enable this extension
                | pulldown_cmark::Tag::FootnoteDefinition(_)
                => unreachable!(),
        }
    }
}

impl PushStr for Renderer<'_> {
    fn push_str(&mut self, s: &str) {
        self.body.push_str(s);
        if self.in_heading {
            self.outline.push_str(s);
            if self.outline_level == 1 {
                self.title.push_str(s);
            }
        }
    }
}

struct TableAlignments(Vec<pulldown_cmark::Alignment>);

impl PartialEq for TableAlignments {
    fn eq(&self, other: &TableAlignments) -> bool {
        Iterator::eq(
            self.0.iter().map(|&alignment| alignment as u8),
            other.0.iter().map(|&alignment| alignment as u8),
        )
    }
}

impl Eq for TableAlignments {}

// pulldown_cmark::Alignment isn't Hash
impl Hash for TableAlignments {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for &alignment in &self.0 {
            state.write_u8(alignment as u8);
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
enum Class {
    Table(TableAlignments),
}

impl Class {
    fn write_name(&self, buf: &mut impl PushStr) {
        match self {
            Self::Table(alignments) => {
                buf.push_str("t");
                for alignment in &alignments.0 {
                    buf.push_str(match alignment {
                        pulldown_cmark::Alignment::None => "n",
                        pulldown_cmark::Alignment::Left => "l",
                        pulldown_cmark::Alignment::Center => "c",
                        pulldown_cmark::Alignment::Right => "r",
                    });
                }
            }
        }
    }
    fn write_definition(&self, buf: &mut impl PushStr) {
        match self {
            Self::Table(alignments) => {
                for (i, alignment) in alignments.0.iter().copied().enumerate() {
                    if alignment == pulldown_cmark::Alignment::None {
                        continue;
                    }
                    buf.push_str(".");
                    self.write_name(buf);
                    push!(buf, " td:nth-child({})", i);
                    buf.push_str("{text-align:");
                    buf.push_str(match alignment {
                        pulldown_cmark::Alignment::None => unreachable!(),
                        pulldown_cmark::Alignment::Left => "left",
                        pulldown_cmark::Alignment::Center => "center",
                        pulldown_cmark::Alignment::Right => "right",
                    });
                    buf.push_str("}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, Class, Markdown, TableAlignments};
    use ::pulldown_cmark::Alignment;

    #[test]
    fn table_class() {
        let class = Class::Table(TableAlignments(vec![
            Alignment::Left,
            Alignment::None,
            Alignment::Right,
            Alignment::Center,
            Alignment::Right,
        ]));

        let mut buf = String::new();
        class.write_name(&mut buf);
        assert_eq!(buf, "tlnrcr");

        buf.clear();
        class.write_definition(&mut buf);
        let css = concat!(
            ".tlnrcr td:nth-child(0){text-align:left}",
            ".tlnrcr td:nth-child(2){text-align:right}",
            ".tlnrcr td:nth-child(3){text-align:center}",
            ".tlnrcr td:nth-child(4){text-align:right}",
        );
        assert_eq!(buf, css);
    }

    #[test]
    fn published() {
        assert_eq!(
            parse("published: false\nfoo").unwrap(),
            Markdown {
                published: Some("false".into()),
                title: String::new(),
                body: "<p>foo</p>".to_owned(),
                outline: String::new(),
            }
        );
        assert_eq!(
            parse("published: 2038-01-19\nPublished: [published]").unwrap(),
            Markdown {
                published: Some("2038-01-19".into()),
                title: String::new(),
                body: "<p>Published: 2038-01-19</p>".to_owned(),
                outline: String::new(),
            },
        );
    }

    #[track_caller]
    fn just_body(input: &str) -> String {
        let markdown = parse(input).unwrap();
        assert_eq!(markdown.published, None, "published is present");
        assert_eq!(markdown.title, "", "title is not empty");
        assert_eq!(markdown.outline, "", "outline is not empty");
        markdown.body
    }

    #[test]
    fn empty() {
        assert_eq!(just_body(""), "");
    }

    #[test]
    fn spacing() {
        assert_eq!(just_body("foobar"), "<p>foobar</p>");
        assert_eq!(just_body("foo\nbar"), "<p>foo bar</p>");
        assert_eq!(just_body("foo  \nbar"), "<p>foo<br>bar</p>");
        assert_eq!(just_body("a\n\nb"), "<p>a</p><p>b</p>");
        assert_eq!(just_body("foo\n\n---"), "<p>foo</p><hr>");
    }

    #[test]
    fn heading() {
        assert_eq!(
            parse("# foo bar { #foo-bar }").unwrap(),
            Markdown {
                published: None,
                title: "foo bar".to_owned(),
                body: "<h1 id='foo-bar'>foo bar</h1>".to_owned(),
                outline: "<ul><li><a href='#foo-bar'>foo bar</a></li></ul>".to_owned(),
            },
        );
        assert_eq!(
            parse(
                "\
                    # the title { #top }\n\
                    ## a { #a }\n\
                    ### b { #b }\n\
                    ### c { #c }\n\
                    ## d { #d }\n\
                ",
            )
            .unwrap(),
            Markdown {
                published: None,
                title: "the title".to_owned(),
                body: "\
                    <h1 id='top'>the title</h1>\
                        <h2 id='a'>a</h2>\
                            <h3 id='b'>b</h3>\
                            <h3 id='c'>c</h3>\
                        <h2 id='d'>d</h2>\
                "
                .to_owned(),
                outline: "\
                    <ul>\
                        <li><a href='#top'>the title</a><ul>\
                            <li><a href='#a'>a</a><ul>\
                                <li><a href='#b'>b</a></li>\
                                <li><a href='#c'>c</a></li>\
                            </ul></li>\
                            <li><a href='#d'>d</a></li>\
                        </ul></li>\
                    </ul>\
                "
                .to_owned(),
            },
        );
    }

    #[test]
    fn table() {
        assert_eq!(
            just_body(
                "\
                    | a | b | c |\n\
                    | - | - | - |\n\
                    | d | e | f |\n\
                    | g | h | i |\
                ",
            ),
            "\
                <table>\
                    <thead>\
                        <tr><th>a</th><th>b</th><th>c</th></tr>\
                    </thead>\
                    <tbody>\
                        <tr><td>d</td><td>e</td><td>f</td></tr>\
                        <tr><td>g</td><td>h</td><td>i</td></tr>\
                    </tbody>\
                </table>\
            "
        );
        assert_eq!(
            just_body(
                "\
                    | Language | Score |\n\
                    | :------: | ----: |\n\
                    | Rust     |   10  |\n\
                    | Zig      |    8  |\n\
                    | Go       |    0  |\n\
                    \n\
                    | Crate | Size (KB) |\n\
                    | :-: | -: |\n\
                    | `cfg-if` v1.0.0 | 7.93 |\n\
                    | `syn` v1.0.86 | 235 |\n\
                ",
            ),
            "\
                <table class='tcr'>\
                    <thead>\
                        <tr><th>Language</th><th>Score</th></tr>\
                    </thead>\
                    <tbody>\
                        <tr><td>Rust</td><td>10</td></tr>\
                        <tr><td>Zig</td><td>8</td></tr>\
                        <tr><td>Go</td><td>0</td></tr>\
                    </tbody>\
                </table>\
                <table class='tcr'>\
                    <thead>\
                        <tr><th>Crate</th><th>Size (KB)</th></tr>\
                    </thead>\
                    <tbody>\
                        <tr><td><code>cfg-if</code> v1.0.0</td><td>7.93</td></tr>\
                        <tr><td><code>syn</code> v1.0.86</td><td>235</td></tr>\
                    </tbody>\
                </table>\
                <style>\
                    .tcr td:nth-child(0){text-align:center}\
                    .tcr td:nth-child(1){text-align:right}\
                </style>\
            ",
        );
    }

    #[test]
    fn blockquote() {
        assert_eq!(just_body("> foo"), "<blockquote><p>foo</p></blockquote>");
    }

    #[test]
    fn code() {
        assert_eq!(
            just_body("`no language`"),
            "<p><code>no language</code></p>"
        );
        assert_eq!(
            just_body("```\ncode\n```"),
            "<pre><code>code\n</code></pre>"
        );
    }

    #[test]
    fn lists() {
        assert_eq!(
            just_body("1. Rust\n1. other languages"),
            "<ol><li>Rust</li><li>other languages</li></ol>"
        );
        assert_eq!(
            just_body("2. Rust\n1. other languages"),
            "<ol start='2'><li>Rust</li><li>other languages</li></ol>"
        );
        assert_eq!(
            just_body("- item\n- item"),
            "<ul><li>item</li><li>item</li></ul>"
        );
    }

    #[test]
    fn emphasis() {
        assert_eq!(just_body("*very* good"), "<p><em>very</em> good</p>");
        assert_eq!(
            just_body("**very** good"),
            "<p><strong>very</strong> good</p>"
        );
        assert_eq!(
            just_body("~~not~~ very good"),
            "<p><del>not</del> very good</p>"
        );
    }

    #[test]
    fn links() {
        assert_eq!(
            just_body("[here](https://www.youtube.com/watch?v=dQw4w9WgXcQ)"),
            "<p><a href='https://www.youtube.com/watch?v=dQw4w9WgXcQ'>here</a></p>",
        );
    }

    #[test]
    fn images() {
        assert_eq!(
            just_body("![a nice image](image.jpg)"),
            "<p><img src='image.jpg' alt='a nice image'></p>",
        );
    }
}
