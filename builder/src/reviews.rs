const HTML_PATH: &str = "reviews.html";
const CSS_PATH: &str = "reviews.css";
const JS_PATH: &str = "reviews.js";

pub(crate) fn asset<'a>(
    toml_path: &'a Path,
    template_path: &'a Path,
    css_path: &'a Path,
    js_path: &'a Path,
    out_path: &'a Path,
    templater: impl Asset<Output = Templater> + Clone + 'a,
) -> impl Asset<Output = ()> + 'a {
    let template = asset::TextFile::new(template_path)
        .map(|src| Template::compile(&src?).context("failed to compile reviews template"))
        .map(Rc::new)
        .cache();

    let template_vars = asset::TextFile::new(toml_path)
        .map(|src| -> anyhow::Result<TemplateVars> {
            let data = toml::from_str::<Data>(&src?)?;
            let introduction = markdown::parse(&data.introduction);
            Ok(TemplateVars {
                summary: introduction.summary,
                introduction: introduction.body,
                entries: data.entries.into_iter().map(Entry::from).collect(),
                reviews_css: CSS_PATH,
                reviews_js: JS_PATH,
            })
        })
        .map(|res| {
            res.unwrap_or_else(|e| {
                log::error!("{e:?}");
                TemplateVars {
                    summary: String::new(),
                    introduction: format!("<p style='color:red'>Error: {e:?}</p>"),
                    entries: Vec::new(),
                    reviews_css: CSS_PATH,
                    reviews_js: JS_PATH,
                }
            })
        })
        .map(Rc::new)
        .cache();

    let html = asset::all((templater, template, template_vars))
        .map(|(templater, template, template_vars)| {
            let template = match &*template {
                Ok(template) => template,
                Err(e) => return error_page([e]),
            };
            let rendered = match templater.render(template, template_vars) {
                Ok(rendered) => rendered,
                Err(e) => return error_page([&e]),
            };

            minify::html(&rendered)
        })
        .map(move |html| {
            write_file(out_path.join(HTML_PATH), html)?;
            log::info!("successfully emitted {HTML_PATH}");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out_path.join(HTML_PATH));

    let css = asset::TextFile::new(css_path)
        .map(move |res| -> anyhow::Result<()> {
            let css = minify::css(&res?);
            write_file(out_path.join(CSS_PATH), css)?;
            log::info!("successfully emitted {CSS_PATH}");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out_path.join(CSS_PATH));

    let js = asset::TextFile::new(js_path)
        .map(move |res| -> anyhow::Result<()> {
            let js = minify::js(&res?);
            write_file(out_path.join(JS_PATH), js)?;
            log::info!("successfully emitted {JS_PATH}");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out_path.join(JS_PATH));

    asset::all((html, css, js)).map(|((), (), ())| {})
}

#[derive(Serialize)]
struct TemplateVars {
    summary: String,
    introduction: String,
    entries: Vec<Entry>,
    reviews_css: &'static str,
    reviews_js: &'static str,
}

#[derive(Serialize)]
struct Entry {
    r#type: String,
    artists: String,
    title: String,
    released_year: u32,
    released: String,
    genres: String,
    review: Option<Review>,
    links: data::Links,
}

impl Entry {
    fn from(entry: data::Entry) -> Self {
        let r#type = match entry.r#type {
            data::Type::MusicRelease(r) => {
                let (format_lower, format_upper) = match r.format {
                    data::r#type::music_release::Format::Single => ("single", "Single"),
                    data::r#type::music_release::Format::EP => ("EP", "EP"),
                    data::r#type::music_release::Format::Album => ("album", "Album"),
                    data::r#type::music_release::Format::Mixtape => ("mixtape", "Mixtape"),
                    data::r#type::music_release::Format::Compilation => (
                        "<abbr title='Compilation'>comp</abbr>",
                        "<abbr title='Compilation'>Comp</abbr>",
                    ),
                };
                match r.recording_type {
                    data::r#type::music_release::RecordingType::Studio => format_upper.to_owned(),
                    data::r#type::music_release::RecordingType::Live => {
                        format!("Live {format_lower}")
                    }
                    data::r#type::music_release::RecordingType::Bootleg => {
                        format!("Bootleg {format_lower}")
                    }
                }
            }
        };
        Entry {
            r#type,
            artists: entry.artists.join(", "),
            title: entry.title,
            released_year: entry.released.year,
            released: entry.released.to_string(),
            genres: entry.genres.join(", "),
            review: entry.review.map(|review| Review {
                date: review.date.to_string(),
                score: review.score.as_str(),
                comment: review.comment,
            }),
            links: entry.links,
        }
    }
}

#[derive(Serialize)]
struct Review {
    date: String,
    score: &'static str,
    comment: Option<String>,
}

mod data {
    #[derive(Deserialize)]
    pub(in crate::reviews) struct Data {
        pub introduction: String,
        pub entries: Vec<Entry>,
    }

    mod entry {
        pub(in crate::reviews) struct Entry {
            pub r#type: Type,
            pub artists: Vec<String>,
            pub title: String,
            pub released: Released,
            pub genres: Vec<String>,
            pub review: Option<Review>,
            pub links: Links,
        }

        impl<'de> Deserialize<'de> for Entry {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_map(DeVisitor)
            }
        }

        struct DeVisitor;
        impl<'de> de::Visitor<'de> for DeVisitor {
            type Value = Entry;
            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("an entry table")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let r#type = de_map_access_require_entry(&mut map, "type")?;
                let Artists(artists) = de_map_access_require_entry(&mut map, "artists")?;
                let title = de_map_access_require_entry(&mut map, "title")?;
                let released = de_map_access_require_entry(&mut map, "released")?;
                let genres = de_map_access_require_entry(&mut map, "genres")?;
                let review::Maybe(review) = de_map_access_require_entry(&mut map, "review")?;
                let links = match map.next_key_seed(LiteralStr("links"))? {
                    Some(()) => map.next_value::<Links>()?,
                    None => Links::default(),
                };

                Ok(Entry {
                    r#type,
                    artists,
                    title,
                    released,
                    genres,
                    review,
                    links,
                })
            }
        }

        mod artists {
            pub(super) struct Artists(pub Vec<String>);

            impl<'de> Deserialize<'de> for Artists {
                fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                    deserializer.deserialize_any(DeVisitor)
                }
            }

            struct DeVisitor;
            impl<'de> de::Visitor<'de> for DeVisitor {
                type Value = Artists;
                fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                    f.write_str("a single artist or list of artists")
                }
                fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                    self.visit_string(v.to_owned())
                }
                fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                    Ok(Artists(vec![v]))
                }
                fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
                    Ok(Artists(<Vec<String>>::deserialize(
                        SeqAccessDeserializer::new(seq),
                    )?))
                }
            }

            use serde::de;
            use serde::de::value::SeqAccessDeserializer;
            use serde::de::Deserialize;
            use serde::de::Deserializer;
            use std::fmt;
            use std::fmt::Formatter;
        }
        use artists::Artists;

        use super::review;
        use super::Links;
        use super::Released;
        use super::Review;
        use super::Type;
        use crate::util::serde::de_map_access_require_entry;
        use crate::util::serde::LiteralStr;
        use serde::de;
        use serde::Deserialize;
        use serde::Deserializer;
        use std::fmt;
        use std::fmt::Formatter;
    }
    pub(in crate::reviews) use entry::Entry;

    pub(in crate::reviews) mod r#type {
        pub(in crate::reviews) enum Type {
            /// A music release.
            MusicRelease(MusicRelease),
        }

        impl<'de> Deserialize<'de> for Type {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_seq(DeVisitor)
            }
        }

        struct DeVisitor;

        impl<'de> de::Visitor<'de> for DeVisitor {
            type Value = Type;

            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("a non-empty list")
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let form = seq
                    .next_element::<Form>()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let deserializer = SeqAccessDeserializer::new(seq);
                Ok(match form {
                    Form::MusicRelease => {
                        Type::MusicRelease(MusicRelease::deserialize(deserializer)?)
                    }
                })
            }
        }

        #[derive(Deserialize)]
        enum Form {
            #[serde(rename = "music release")]
            MusicRelease,
        }

        pub(in crate::reviews) mod music_release {
            #[derive(Deserialize)]
            pub(in crate::reviews) struct MusicRelease {
                pub recording_type: RecordingType,
                pub format: Format,
            }

            /// How the music release was recorded.
            #[derive(Deserialize)]
            #[serde(rename_all = "lowercase")]
            pub(in crate::reviews) enum RecordingType {
                Studio,
                Live,
                Bootleg,
            }

            /// The format the music release was released as.
            #[derive(Deserialize)]
            #[serde(rename_all = "lowercase")]
            pub(in crate::reviews) enum Format {
                Single,
                #[serde(rename = "EP")]
                EP,
                Album,
                Mixtape,
                Compilation,
            }

            use serde::Deserialize;
        }
        use music_release::MusicRelease;

        use serde::de;
        use serde::de::value::SeqAccessDeserializer;
        use serde::de::Deserializer;
        use serde::Deserialize;
        use std::fmt;
        use std::fmt::Formatter;
    }
    pub(in crate::reviews) use r#type::Type;

    mod released {
        pub(in crate::reviews) struct Released {
            pub year: u32,
            pub month: Option<u8>,
            pub day: Option<u8>,
        }

        impl Display for Released {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                let year = self.year;
                match (self.month, self.day) {
                    (Some(month), Some(day)) => write!(f, "{year:04}-{month:02}-{day:02}"),
                    (Some(month), None) => write!(f, "{year:04}-{month:02}"),
                    (None, None) => write!(f, "{year:04}"),
                    _ => unreachable!(),
                }
            }
        }

        impl<'de> Deserialize<'de> for Released {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_str(DeVisitor)
            }
        }

        struct DeVisitor;
        impl<'de> de::Visitor<'de> for DeVisitor {
            type Value = Released;
            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("a release date")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let mut parts = v.splitn(3, '-');
                let year = parts
                    .next()
                    .unwrap()
                    .parse::<u32>()
                    .map_err(|e| de::Error::custom(format_args!("invalid year: {e}")))?;
                let month = match parts.next() {
                    Some(month) => Some(
                        month
                            .parse::<u8>()
                            .map_err(|e| de::Error::custom(format_args!("invalid month: {e}")))?,
                    ),
                    None => None,
                };
                let day = match parts.next() {
                    Some(day) => Some(
                        day.parse::<u8>()
                            .map_err(|e| de::Error::custom(format_args!("invalid day: {e}")))?,
                    ),
                    None => None,
                };
                Ok(Released { year, month, day })
            }
        }

        use serde::de;
        use serde::Deserialize;
        use serde::Deserializer;
        use std::fmt;
        use std::fmt::Display;
        use std::fmt::Formatter;
    }
    pub(in crate::reviews) use released::Released;

    mod review {
        pub(in crate::reviews) struct Review {
            pub date: toml::value::Date,
            pub score: Score,
            pub comment: Option<String>,
        }

        pub(in crate::reviews) struct Maybe(pub Option<Review>);

        impl<'de> Deserialize<'de> for Maybe {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_any(DeVisitor).map(Maybe)
            }
        }

        struct DeVisitor;
        impl<'de> de::Visitor<'de> for DeVisitor {
            type Value = Option<Review>;
            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("a review table or \"TODO\"")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                if v != "TODO" {
                    return Err(de::Error::invalid_value(de::Unexpected::Str(v), &self));
                }
                Ok(None)
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let date =
                    de_map_access_require_entry::<toml::value::Datetime, _>(&mut map, "date")?;
                let toml::value::Datetime { date: Some(date), time: None, offset: None } =
                    date else {
                    return Err(de::Error::custom("review date is in invalid format"));
                };

                let score = de_map_access_require_entry(&mut map, "score")?;
                let comment = match map.next_key_seed(LiteralStr("comment"))? {
                    Some(()) => Some(map.next_value::<String>()?),
                    None => None,
                };
                Ok(Some(Review {
                    date,
                    score,
                    comment,
                }))
            }
        }

        use super::Score;
        use crate::util::serde::de_map_access_require_entry;
        use crate::util::serde::LiteralStr;
        use serde::de;
        use serde::de::Deserializer;
        use serde::Deserialize;
        use std::fmt;
        use std::fmt::Formatter;
    }
    use review::Review;

    mod score {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub(in crate::reviews) struct Score(u8);

        impl Score {
            pub const fn as_str(self) -> &'static str {
                match self.0 {
                    0 => "0.0",
                    1 => "0.5",
                    2 => "1.0",
                    3 => "1.5",
                    4 => "2.0",
                    5 => "2.5",
                    6 => "3.0",
                    7 => "3.5",
                    8 => "4.0",
                    9 => "4.5",
                    10 => "5.0",
                    _ => unreachable!(),
                }
            }
        }

        impl<'de> Deserialize<'de> for Score {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_str(DeVisitor)
            }
        }

        struct DeVisitor;
        impl<'de> de::Visitor<'de> for DeVisitor {
            type Value = Score;
            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("a score")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Score(match v {
                    "0.0" => 0,
                    "0.5" => 1,
                    "1.0" => 2,
                    "1.5" => 3,
                    "2.0" => 4,
                    "2.5" => 5,
                    "3.0" => 6,
                    "3.5" => 7,
                    "4.0" => 8,
                    "4.5" => 9,
                    "5.0" => 10,
                    _ => return Err(de::Error::invalid_value(de::Unexpected::Str(v), &self)),
                }))
            }
        }

        use serde::de;
        use serde::de::Deserializer;
        use serde::Deserialize;
        use std::fmt;
        use std::fmt::Formatter;
    }
    use score::Score;

    #[derive(Default, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::reviews) struct Links {
        #[serde(default)]
        pub rym: Option<String>,
    }

    use serde::Deserialize;
    use serde::Serialize;
}
use data::Data;

use crate::templater::Templater;
use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::error_page;
use crate::util::log_errors;
use crate::util::markdown;
use crate::util::minify;
use crate::util::write_file;
use anyhow::Context as _;
use handlebars::Template;
use serde::Serialize;
use std::path::Path;
use std::rc::Rc;