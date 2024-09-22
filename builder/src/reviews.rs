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
    config: impl Asset<Output = &'a Config> + Copy + 'a,
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
                sites: data.sites,
                entries: data.entries.into_iter().map(Entry::from).collect(),
                reviews_css: CSS_PATH,
                reviews_js: JS_PATH,
            })
        })
        .map(Rc::new)
        .cache();

    let html = asset::all((templater, template, template_vars))
        .map(|(templater, template, template_vars)| {
            let (template, template_vars) =
                ErrorPage::zip((*template).as_ref(), (*template_vars).as_ref())?;
            Ok(templater.render(template, template_vars)?)
        })
        .map(move |html| {
            let html = html.unwrap_or_else(ErrorPage::into_html);
            write_file(out_path.join(HTML_PATH), html)?;
            log::info!("successfully emitted {HTML_PATH}");
            Ok(())
        })
        .map(log_errors)
        .modifies_path(out_path.join(HTML_PATH));

    let css = copy_minify(
        config,
        minify::FileType::Css,
        css_path,
        out_path.join(CSS_PATH),
    );

    let js = copy_minify(
        config,
        minify::FileType::Js,
        js_path,
        out_path.join(JS_PATH),
    );

    asset::all((html, css, js)).map(|((), (), ())| {})
}

#[derive(Serialize)]
struct TemplateVars {
    summary: String,
    introduction: String,
    sites: Vec<data::Site>,
    entries: Vec<Entry>,
    reviews_css: &'static str,
    reviews_js: &'static str,
}

#[derive(Serialize)]
struct Entry {
    r#type: &'static str,
    artists: String,
    title: String,
    released_short: String,
    released_full: String,
    genres: String,
    review: Option<Review>,
    links: Option<Box<[Option<String>]>>,
}

impl Entry {
    fn from(entry: data::Entry) -> Self {
        let r#type = match entry.r#type {
            data::Type::MusicRelease(r) => {
                macro_rules! match_recording_type {
                    ($format_lower:literal, $format_upper:literal) => {{
                        use data::r#type::music_release::RecordingType::*;
                        match r.recording_type {
                            Studio => $format_upper,
                            Live => concat!("Live ", $format_lower),
                            Bootleg => concat!("Bootleg ", $format_lower),
                            Demo => concat!("Demo ", $format_lower),
                        }
                    }};
                }

                {
                    #[allow(clippy::enum_glob_use)]
                    use data::r#type::music_release::Format::*;
                    match r.format {
                        Single => match_recording_type!("single", "Single"),
                        EP => match_recording_type!("EP", "EP"),
                        Album => match_recording_type!("album", "Album"),
                        Mixtape => match_recording_type!("mixtape", "Mixtape"),
                        Compilation => match_recording_type!("compilation", "Compilation"),
                    }
                }
            }
            data::Type::Comic(data::r#type::Comic::Oneshot) => "Oneshot comic",
            data::Type::Comic(data::r#type::Comic::Series) => "Comic series",
            data::Type::Comic(data::r#type::Comic::Anthology) => "Comic anthology",
            data::Type::Prose(r) => {
                let mut full = match r.installment_type {
                    data::r#type::prose::InstallmentType::ShortStory => "Short story series",
                    data::r#type::prose::InstallmentType::LightNovel => "Light novel series",
                    data::r#type::prose::InstallmentType::Novella => "Novella series",
                    data::r#type::prose::InstallmentType::Novel => "Novel series",
                };
                if !r.series {
                    full = full.strip_suffix(" series").unwrap();
                }
                full
            }
            data::Type::Film(data::r#type::Film::Short) => "Short film",
            data::Type::Film(data::r#type::Film::Feature) => "Feature film",
            data::Type::Film(data::r#type::Film::Series) => "Film series",
            data::Type::Film(data::r#type::Film::TvShow) => "TV show",
            data::Type::Film(data::r#type::Film::TvSeason) => "TV season",
            data::Type::VisualNovel => "Visual novel",
        };
        Entry {
            r#type,
            artists: entry.artists.join(", "),
            title: entry.title,
            released_short: format!("{:#}", entry.released),
            released_full: format!("{}", entry.released),
            genres: entry.genres.join(", "),
            review: entry.review.map(|review| Review {
                date: review.date.to_string(),
                score: review.score.as_str(),
                comment: review.comment.map(|c| markdown::parse(&c).body),
            }),
            links: Some(entry.links).filter(|links| links.iter().any(Option::is_some)),
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
    pub(in crate::reviews) struct Data {
        pub introduction: String,
        pub sites: Vec<Site>,
        pub entries: Vec<Entry>,
    }

    impl<'de> Deserialize<'de> for Data {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_map(DeVisitor)
        }
    }

    struct DeVisitor;
    impl<'de> de::Visitor<'de> for DeVisitor {
        type Value = Data;
        fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
            f.write_str("a data table")
        }
        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let introduction = de_map_access_require_entry(&mut map, "introduction")?;
            let (sites, site_indices) =
                de_map_access_require_entry_seed(&mut map, "sites", SiteMap)?;
            let entries_seed = entries::DeserializeSeed {
                site_indices: &site_indices,
            };
            let entries = de_map_access_require_entry_seed(&mut map, "entries", entries_seed)?;
            Ok(Data {
                introduction,
                sites,
                entries,
            })
        }
    }

    mod site_map {
        pub(in crate::reviews) struct SiteMap;

        impl<'de> DeserializeSeed<'de> for SiteMap {
            type Value = (Vec<Site>, HashMap<String, usize>);

            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_map(self)
            }
        }

        impl<'de> de::Visitor<'de> for SiteMap {
            type Value = (Vec<Site>, HashMap<String, usize>);
            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("a site table")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut sites = Vec::new();
                let mut indices = HashMap::default();
                while let Some((key, value)) = map.next_entry()? {
                    if indices.contains_key(&key) {
                        return Err(de::Error::custom(format_args!("duplicate site {key}")));
                    }
                    indices.insert(key, sites.len());
                    sites.push(value);
                }
                Ok((sites, indices))
            }
        }

        use super::Site;
        use serde::de;
        use serde::de::DeserializeSeed;
        use serde::de::Deserializer;
        use std::collections::HashMap;
        use std::fmt;
        use std::fmt::Formatter;
    }
    use site_map::SiteMap;

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::reviews) struct Site {
        pub icon: String,
        pub alt: String,
    }

    mod entries {
        pub(super) struct DeserializeSeed<'sites, S: BuildHasher> {
            pub site_indices: &'sites HashMap<String, usize, S>,
        }

        impl<'de, S: BuildHasher> serde::de::DeserializeSeed<'de> for DeserializeSeed<'_, S> {
            type Value = Vec<Entry>;
            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_seq(self)
            }
        }

        impl<'de, S: BuildHasher> de::Visitor<'de> for DeserializeSeed<'_, S> {
            type Value = Vec<Entry>;
            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("a sequence of entries")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut v = Vec::with_capacity(seq.size_hint().unwrap_or(0));

                while let Some(value) = seq.next_element_seed(entry::DeserializeSeed {
                    site_indices: self.site_indices,
                })? {
                    v.push(value);
                }

                Ok(v)
            }
        }

        use super::entry;
        use super::Entry;
        use serde::de;
        use serde::Deserializer;
        use std::collections::HashMap;
        use std::fmt;
        use std::fmt::Formatter;
        use std::hash::BuildHasher;
    }

    mod entry {
        pub(in crate::reviews) struct Entry {
            pub r#type: Type,
            pub artists: Vec<String>,
            pub title: String,
            pub released: Released,
            pub genres: Vec<String>,
            pub review: Option<Review>,
            /// One link for every site
            pub links: Box<[Option<String>]>,
        }

        pub(super) struct DeserializeSeed<'sites, S: BuildHasher> {
            pub site_indices: &'sites HashMap<String, usize, S>,
        }

        impl<'de, S: BuildHasher> serde::de::DeserializeSeed<'de> for DeserializeSeed<'_, S> {
            type Value = Entry;
            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_map(self)
            }
        }

        impl<'de, S: BuildHasher> de::Visitor<'de> for DeserializeSeed<'_, S> {
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
                let links_seed = links::DeserializeSeed {
                    site_indices: self.site_indices,
                };
                let links = match map.next_key_seed(LiteralStr("links"))? {
                    Some(()) => map.next_value_seed(links_seed)?,
                    None => links_seed.default(),
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

        use super::links;
        use super::review;
        use super::Released;
        use super::Review;
        use super::Type;
        use crate::util::serde::de_map_access_require_entry;
        use crate::util::serde::LiteralStr;
        use serde::de;
        use serde::Deserializer;
        use std::collections::HashMap;
        use std::fmt;
        use std::fmt::Formatter;
        use std::hash::BuildHasher;
    }
    pub(in crate::reviews) use entry::Entry;

    pub(in crate::reviews) mod r#type {
        pub(in crate::reviews) enum Type {
            /// A music release.
            MusicRelease(MusicRelease),
            /// A comic.
            Comic(Comic),
            /// Prose.
            Prose(Prose),
            /// A film.
            Film(Film),
            /// A visual novel.
            VisualNovel,
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
                    Form::Comic => Type::Comic(<(Comic,)>::deserialize(deserializer)?.0),
                    Form::Prose => Type::Prose(Prose::deserialize(deserializer)?),
                    Form::Film => Type::Film(<(Film,)>::deserialize(deserializer)?.0),
                    Form::VisualNovel => Type::VisualNovel,
                })
            }
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case")]
        enum Form {
            MusicRelease,
            Comic,
            Prose,
            Film,
            VisualNovel,
        }

        pub(in crate::reviews) mod music_release {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            pub(in crate::reviews) struct MusicRelease {
                pub recording_type: RecordingType,
                pub format: Format,
            }

            /// How the music release was recorded.
            #[derive(Deserialize)]
            #[serde(rename_all = "kebab-case")]
            pub(in crate::reviews) enum RecordingType {
                Studio,
                Live,
                Bootleg,
                Demo,
            }

            /// The format the music release was released as.
            #[derive(Deserialize)]
            #[serde(rename_all = "kebab-case")]
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

        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub(in crate::reviews) enum Comic {
            Oneshot,
            Series,
            Anthology,
        }

        pub(in crate::reviews) mod prose {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            pub(in crate::reviews) struct Prose {
                pub installment_type: InstallmentType,
                #[serde(default, deserialize_with = "deserialize_series")]
                pub series: bool,
            }

            #[derive(Deserialize)]
            #[serde(rename_all = "kebab-case")]
            pub(in crate::reviews) enum InstallmentType {
                ShortStory,
                LightNovel,
                Novella,
                Novel,
            }

            fn deserialize_series<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<bool, D::Error> {
                LiteralStr("series").deserialize(deserializer)?;
                Ok(true)
            }

            use crate::util::serde::LiteralStr;
            use serde::de::DeserializeSeed;
            use serde::Deserialize;
            use serde::Deserializer;
        }
        pub(in crate::reviews) use prose::Prose;

        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub(in crate::reviews) enum Film {
            Short,
            Feature,
            Series,
            TvShow,
            TvSeason,
        }

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
            pub start: PrecisionDate,
            pub end: Option<PrecisionDate>,
        }

        impl Display for Released {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                let start = self.start;
                if let Some(end) = self.end {
                    if end == start {
                        Display::fmt(&start, f)
                    } else {
                        Display::fmt(&start, f)?;
                        f.write_str("–")?;
                        Display::fmt(&end, f)
                    }
                } else {
                    Display::fmt(&start, f)?;
                    f.write_str("–")
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
                let mut parts = v.splitn(2, '–');
                let start = parts.next().unwrap();
                let start = start
                    .parse::<PrecisionDate>()
                    .map_err(|e| de::Error::custom(format_args!("invalid start date: {e}")))?;
                let end =
                    match parts.next() {
                        Some("") => None,
                        Some(date) => Some(date.parse::<PrecisionDate>().map_err(|e| {
                            de::Error::custom(format_args!("invalid end date: {e}"))
                        })?),
                        None => Some(start),
                    };
                Ok(Released { start, end })
            }
        }

        use crate::util::precision_date::PrecisionDate;
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
                let toml::value::Datetime {
                    date: Some(date),
                    time: None,
                    offset: None,
                } = date
                else {
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

    mod links {
        pub(super) struct DeserializeSeed<'sites, S: BuildHasher> {
            pub site_indices: &'sites HashMap<String, usize, S>,
        }

        impl<S: BuildHasher> DeserializeSeed<'_, S> {
            pub fn default(&self) -> Box<[Option<String>]> {
                (0..self.site_indices.len()).map(|_| None).collect()
            }
        }

        impl<'de, S: BuildHasher> serde::de::DeserializeSeed<'de> for DeserializeSeed<'_, S> {
            type Value = Box<[Option<String>]>;
            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_map(self)
            }
        }

        impl<'de, S: BuildHasher> de::Visitor<'de> for DeserializeSeed<'_, S> {
            type Value = Box<[Option<String>]>;
            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("a links table")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut links = self.default();
                while let Some((site, url)) = map.next_entry::<String, String>()? {
                    let &index = self
                        .site_indices
                        .get(&site)
                        .ok_or_else(|| de::Error::custom(format_args!("unknown site `{site}`")))?;
                    if links[index].is_some() {
                        return Err(de::Error::custom(format_args!("duplicate site `{site}`")));
                    }
                    links[index] = Some(url);
                }
                Ok(links)
            }
        }

        use serde::de;
        use serde::Deserializer;
        use std::collections::HashMap;
        use std::fmt;
        use std::fmt::Formatter;
        use std::hash::BuildHasher;
    }

    use crate::util::serde::de_map_access_require_entry;
    use crate::util::serde::de_map_access_require_entry_seed;
    use serde::de;
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serialize;
    use std::fmt;
    use std::fmt::Formatter;
}
use data::Data;

use crate::config::copy_minify;
use crate::config::Config;
use crate::templater::Templater;
use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use crate::util::markdown;
use crate::util::minify;
use crate::util::write_file;
use crate::util::ErrorPage;
use anyhow::Context as _;
use handlebars::Template;
use serde::Serialize;
use std::path::Path;
use std::rc::Rc;
