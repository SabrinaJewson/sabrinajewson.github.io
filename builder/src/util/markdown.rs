#[derive(Serialize)] // Serialization used in templates
#[cfg_attr(test, derive(Debug, PartialEq))]
pub(crate) struct Markdown {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) summary: String,
    pub(crate) outline: String,
}

pub(crate) fn parse(source: &str) -> Markdown {
    let options = pulldown_cmark::Options::empty()
        | pulldown_cmark::Options::ENABLE_TABLES
        | pulldown_cmark::Options::ENABLE_HEADING_ATTRIBUTES
        | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
        | pulldown_cmark::Options::ENABLE_SMART_PUNCTUATION;

    Renderer {
        parser: pulldown_cmark::Parser::new_ext(source, options),
        title: String::new(),
        in_title: false,
        body: String::new(),
        summary: String::new(),
        in_summary: false,
        in_table_head: false,
        used_classes: HashSet::new(),
        outline: String::new(),
        outline_level: 1,
        in_heading: false,
        syntax_set: &SYNTAX_SET,
    }
    .render()
}

pub(crate) fn theme_css(theme: &Theme) -> String {
    syntect::html::css_for_theme_with_class_style(theme, SYNTECT_CLASS_STYLE).unwrap()
}

struct Renderer<'a> {
    parser: pulldown_cmark::Parser<'a, 'a>,
    title: String,
    /// Whether we are currently writing to the title instead of the body.
    in_title: bool,
    body: String,
    summary: String,
    /// Whether we are currently writing to the summary.
    in_summary: bool,
    /// Whether we are in a `<thead>`.
    /// Used to determine whether to output `<td>`s or `<th>`s.
    in_table_head: bool,
    /// Class names that need to be generated in the resulting CSS.
    used_classes: HashSet<Classes>,
    outline: String,
    /// The level of the currently opened heading `<li>` in the outline.
    /// In the range [1..6].
    outline_level: u8,
    /// Whether we are in a `<hN>` tag.
    /// Used to determine whether to also write to the outline.
    in_heading: bool,
    syntax_set: &'a SyntaxSet,
}

impl<'a> Renderer<'a> {
    fn render(mut self) -> Markdown {
        while let Some(event) = self.parser.next() {
            match event {
                pulldown_cmark::Event::Start(tag) => self.start_tag(tag),
                pulldown_cmark::Event::End(tag) => self.end_tag(tag),
                pulldown_cmark::Event::Text(text) => {
                    self.push_summary(&text);
                    escape_html(&mut self, &text);
                }
                pulldown_cmark::Event::Code(text) => {
                    self.push_str("<code class='scode'>");

                    let (language, code) =
                        match text.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
                            Some((language, code)) => (Some(language), code),
                            None => (None, &*text),
                        };

                    if let Some(language) = language {
                        self.syntax_highlight(language, code);
                    } else {
                        escape_html(&mut self, &text);
                    }

                    self.push_summary(code);

                    self.push_str("</code>");
                }
                pulldown_cmark::Event::Html(html) => self.push_str(&html),
                pulldown_cmark::Event::SoftBreak => {
                    self.push_summary(" ");
                    self.push_str(" ");
                }
                pulldown_cmark::Event::HardBreak => {
                    self.push_summary(" ");
                    self.push_str("<br>");
                }
                pulldown_cmark::Event::Rule => self.push_str("<hr>"),
                // We do not enable these extensions
                pulldown_cmark::Event::FootnoteReference(_)
                | pulldown_cmark::Event::TaskListMarker(_) => unreachable!(),
            }
        }

        assert!(!self.in_table_head);
        assert!(!self.in_heading);

        // Close remaining opened tags in the outline.
        for _ in 0..self.outline_level - 1 {
            self.outline.push_str("</li></ul>");
        }

        if !self.used_classes.is_empty() {
            self.push_str("<style>");
            for class in &self.used_classes {
                class.write_definition(&mut self.body);
            }
            self.push_str("</style>");
        }

        Markdown {
            title: self.title,
            body: self.body,
            summary: self.summary,
            outline: self.outline,
        }
    }

    fn start_tag(&mut self, tag: pulldown_cmark::Tag<'a>) {
        match tag {
            pulldown_cmark::Tag::Paragraph => {
                if self.summary.is_empty() {
                    self.in_summary = true;
                }
                self.push_str("<p>");
            }
            pulldown_cmark::Tag::Heading(pulldown_cmark::HeadingLevel::H1, id, classes) => {
                if !classes.is_empty() || id.is_some() {
                    self.error("title IDs and classes are disallowed");
                }
                self.in_title = true;
            }
            pulldown_cmark::Tag::Heading(level, id, classes) => {
                if !classes.is_empty() {
                    self.error("heading classes are disallowed");
                }

                let mut level = level as u8;

                // Update the outline and normalize heading levels.
                if let Some(levels_down) = self.outline_level.checked_sub(level) {
                    self.outline.push_str("</li>");
                    for _ in 0..levels_down {
                        self.outline.push_str("</ul></li>");
                    }
                } else {
                    self.outline.push_str("<ul>");

                    if level != self.outline_level + 1 {
                        let outline_level = self.outline_level;
                        self.error(format_args!(
                            "heading level jump: {outline_level} to {level}"
                        ));
                        level = self.outline_level + 1;
                    }
                }

                self.outline.push_str("<li><a href='#");
                if let Some(id) = id {
                    escape_href(&mut self.outline, id);
                }
                self.outline.push_str("'>");
                self.outline_level = level;

                if let Some(id) = id {
                    push!(self, "<h{level} id='");
                    escape_html(self, id);
                    self.push_str("'><a href='#");
                    escape_html(self, id);
                    self.push_str("' class='anchor'></a>");
                } else {
                    self.error("heading does not have id");
                    push!(self, "<h{level}>");
                }

                self.in_heading = true;
            }
            pulldown_cmark::Tag::Table(alignments) => {
                if alignments
                    .iter()
                    .all(|&align| align == pulldown_cmark::Alignment::None)
                {
                    self.push_str("<table>");
                } else {
                    let alignments = TableAlignments(alignments);
                    self.push_str("<table class='");
                    alignments.write_class_name(self);
                    self.push_str("'>");
                    self.used_classes.insert(Classes::Table(alignments));
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
            pulldown_cmark::Tag::CodeBlock(kind) => {
                self.push_str("<pre class='scode'><code>");

                let language = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) if lang.is_empty() => None,
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => Some(lang),
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };

                fn event_text(
                    event: pulldown_cmark::Event<'_>,
                ) -> Option<pulldown_cmark::CowStr<'_>> {
                    match event {
                        pulldown_cmark::Event::End(_) => None,
                        pulldown_cmark::Event::Text(text) => Some(text),
                        // Other events shouldn't happen
                        _ => unreachable!("unexpected event in code block {:?}", event),
                    }
                }

                if let Some(language) = language {
                    let mut code = String::new();
                    while let Some(part) = self.parser.next().and_then(event_text) {
                        code.push_str(&part);
                    }
                    self.syntax_highlight(&language, &code);
                } else {
                    while let Some(part) = self.parser.next().and_then(event_text) {
                        escape_html(self, &part);
                    }
                }

                self.push_str("</code></pre>");
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
                self.error("email links are not supported yet");
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
    }

    fn end_tag(&mut self, tag: pulldown_cmark::Tag<'a>) {
        match tag {
            pulldown_cmark::Tag::Paragraph => {
                self.push_str("</p>");
                self.in_summary = false;
            }
            pulldown_cmark::Tag::Heading(pulldown_cmark::HeadingLevel::H1, _id, _classes) => {
                self.in_title = false;
            }
            pulldown_cmark::Tag::Heading(level, _id, _classes) => {
                self.in_heading = false;

                self.outline.push_str("</a>");

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
            pulldown_cmark::Tag::List(Some(_)) => self.push_str("</ol>"),
            pulldown_cmark::Tag::List(None) => self.push_str("</ul>"),
            pulldown_cmark::Tag::Item => self.push_str("</li>"),
            pulldown_cmark::Tag::Emphasis => self.push_str("</em>"),
            pulldown_cmark::Tag::Strong => self.push_str("</strong>"),
            pulldown_cmark::Tag::Strikethrough => self.push_str("</del>"),
            pulldown_cmark::Tag::Link(_, _, _) => self.push_str("</a>"),
            // We do not enable this extension
            pulldown_cmark::Tag::FootnoteDefinition(_)
            // We handle closing of these tags in the opening logic
            | pulldown_cmark::Tag::Image(_, _, _)
                | pulldown_cmark::Tag::CodeBlock(_)
                => unreachable!(),
        }
    }

    fn syntax_highlight(&mut self, language: &str, code: &str) {
        let Some(syntax) = self.syntax_set.find_syntax_by_token(language) else {
            self.error(format_args!("no known language {language}"));
            self.push_str(code);
            return;
        };

        let mut generator = syntect::html::ClassedHTMLGenerator::new_with_class_style(
            syntax,
            self.syntax_set,
            SYNTECT_CLASS_STYLE,
        );

        for line in LinesWithEndings::from(code) {
            generator.parse_html_for_line_which_includes_newline(line)
                .expect("thanks syntect, really good API design where you return a `Result` but don’t specify when it can even fail");
        }

        self.push_str(&generator.finalize());
    }

    fn error(&mut self, msg: impl Display) {
        self.push_str("<span style='color:red'>");
        push!(self, "{}", msg);
        self.push_str("</span>");
    }

    fn push_summary(&mut self, s: &str) {
        if self.in_summary {
            self.summary.push_str(s);
        }
    }
}

impl PushStr for Renderer<'_> {
    fn push_str(&mut self, s: &str) {
        if self.in_title {
            self.title.push_str(s);
        } else {
            self.body.push_str(s);
            if self.in_heading {
                self.outline.push_str(s);
            }
        }
    }
}

struct TableAlignments(Vec<pulldown_cmark::Alignment>);

impl TableAlignments {
    fn write_class_name(&self, buf: &mut impl PushStr) {
        buf.push_str("t");
        for alignment in &self.0 {
            buf.push_str(match alignment {
                pulldown_cmark::Alignment::None => "n",
                pulldown_cmark::Alignment::Left => "l",
                pulldown_cmark::Alignment::Center => "c",
                pulldown_cmark::Alignment::Right => "r",
            });
        }
    }
}

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
enum Classes {
    Table(TableAlignments),
}

impl Classes {
    fn write_definition(&self, buf: &mut impl PushStr) {
        match self {
            Self::Table(alignments) => {
                for (i, alignment) in alignments.0.iter().copied().enumerate() {
                    if alignment == pulldown_cmark::Alignment::None {
                        continue;
                    }
                    buf.push_str(".");
                    alignments.write_class_name(buf);
                    push!(buf, " td:nth-child({})", i + 1);
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

const SYNTECT_CLASS_STYLE: syntect::html::ClassStyle =
    syntect::html::ClassStyle::SpacedPrefixed { prefix: "s" };

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);

#[cfg(test)]
mod tests {
    #[test]
    fn table_class() {
        let class = TableAlignments(vec![
            Alignment::Left,
            Alignment::None,
            Alignment::Right,
            Alignment::Center,
            Alignment::Right,
        ]);

        let mut buf = String::new();
        class.write_class_name(&mut buf);
        assert_eq!(buf, "tlnrcr");

        buf.clear();
        Classes::Table(class).write_definition(&mut buf);
        let css = concat!(
            ".tlnrcr td:nth-child(1){text-align:left}",
            ".tlnrcr td:nth-child(3){text-align:right}",
            ".tlnrcr td:nth-child(4){text-align:center}",
            ".tlnrcr td:nth-child(5){text-align:right}",
        );
        assert_eq!(buf, css);
    }

    #[track_caller]
    fn just_body(input: &str) -> String {
        let markdown = parse(input);
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
            parse("# foo bar"),
            Markdown {
                title: "foo bar".to_owned(),
                body: String::new(),
                summary: String::new(),
                outline: String::new(),
            },
        );
        assert_eq!(
            parse(
                "\
                    # the _title_\n\
                    ## a { #a }\n\
                    ### b { #b }\n\
                    ### c { #c }\n\
                    #### d { #d }\n\
                    ## e { #e }\n\
                ",
            ),
            Markdown {
                title: "the <em>title</em>".to_owned(),
                body: "\
                    <h2 id='a'><a href='#a' class='anchor'></a>a</h2>\
                        <h3 id='b'><a href='#b' class='anchor'></a>b</h3>\
                        <h3 id='c'><a href='#c' class='anchor'></a>c</h3>\
                            <h4 id='d'><a href='#d' class='anchor'></a>d</h4>\
                    <h2 id='e'><a href='#e' class='anchor'></a>e</h2>\
                "
                .to_owned(),
                summary: String::new(),
                outline: "\
                    <ul>\
                        <li><a href='#a'>a</a><ul>\
                            <li><a href='#b'>b</a></li>\
                            <li><a href='#c'>c</a><ul>\
                                <li><a href='#d'>d</a></li>\
                            </ul></li>\
                        </ul></li>\
                        <li><a href='#e'>e</a></li>\
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
                        <tr><td><code class='scode'>cfg-if</code> v1.0.0</td><td>7.93</td></tr>\
                        <tr><td><code class='scode'>syn</code> v1.0.86</td><td>235</td></tr>\
                    </tbody>\
                </table>\
                <style>\
                    .tcr td:nth-child(1){text-align:center}\
                    .tcr td:nth-child(2){text-align:right}\
                </style>\
            ",
        );
    }

    #[test]
    fn blockquote() {
        assert_eq!(just_body("> foo"), "<blockquote><p>foo</p></blockquote>");
    }

    #[test]
    fn inline_code() {
        assert_eq!(
            just_body("`no language`"),
            "<p><code class='scode'>no language</code></p>"
        );
        assert_eq!(
            just_body("`[rs] let foo = 5;`"),
            "<p><code class='scode'><span class=\"ssource srust\"> \
                <span class=\"sstorage stype srust\">let</span> \
                foo \
                <span class=\"skeyword soperator srust\">=</span> \
                <span class=\"sconstant snumeric sinteger sdecimal srust\">5</span>\
                <span class=\"spunctuation sterminator srust\">;</span>\
            </span></code></p>",
        );
    }

    #[test]
    fn block_code() {
        assert_eq!(
            just_body("```\ncode\n```"),
            "<pre class='scode'><code>code\n</code></pre>"
        );
        assert_eq!(
            just_body("```rs\nprintln!(\"Hello World!\");\n```"),
            "<pre class='scode'><code><span class=\"ssource srust\">\
                <span class=\"ssupport smacro srust\">println!</span>\
                <span class=\"smeta sgroup srust\">\
                    <span class=\"spunctuation ssection sgroup sbegin srust\">(</span>\
                </span>\
                <span class=\"smeta sgroup srust\">\
                    <span class=\"sstring squoted sdouble srust\">\
                        <span class=\"spunctuation sdefinition sstring sbegin srust\">&quot;</span>\
                        Hello World!\
                        <span class=\"spunctuation sdefinition sstring send srust\">&quot;</span>\
                    </span>\
                </span>\
                <span class=\"smeta sgroup srust\">\
                    <span class=\"spunctuation ssection sgroup send srust\">)</span>\
                </span>\
                <span class=\"spunctuation sterminator srust\">;</span>\n\
            </span></code></pre>"
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

    #[track_caller]
    fn just_summary(input: &str) -> String {
        let markdown = parse(input);
        assert_eq!(markdown.title, "", "title is not empty");
        assert_eq!(markdown.outline, "", "outline is not empty");
        markdown.summary
    }

    #[test]
    fn summary() {
        assert_eq!(just_summary("lorem ipsum dolor"), "lorem ipsum dolor");
        assert_eq!(just_summary("lorem\nipsum  \ndolor"), "lorem ipsum dolor");
        assert_eq!(
            just_summary("`[rs]lorem` **ipsum** _dolor_"),
            "lorem ipsum dolor"
        );
        assert_eq!(just_summary("lorem ipsum\n\ndolor sit amet"), "lorem ipsum");
    }

    use super::parse;
    use super::Classes;
    use super::Markdown;
    use super::TableAlignments;
    use pulldown_cmark::Alignment;
}

use crate::util::push_str::escape_href;
use crate::util::push_str::escape_html;
use crate::util::push_str::push;
use crate::util::push_str::PushStr;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt::Display;
use std::hash::Hash;
use std::hash::Hasher;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
