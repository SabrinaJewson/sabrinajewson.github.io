//! This module contains many small independent components.

pub(crate) mod asset;
pub(crate) mod markdown;
pub(crate) mod minify;
pub(crate) mod push_str;
pub(crate) mod serde;

pub(crate) fn log_errors<T>(res: anyhow::Result<T>) {
    if let Err(e) = res {
        log::error!("{e:?}");
    }
}

pub(crate) struct ErrorPage(String);

impl ErrorPage {
    fn new<'e, I: IntoIterator<Item = &'e anyhow::Error>>(errors: I) -> Self {
        let mut res = String::new();
        for error in errors {
            log::error!("{error:?}");
            push!(res, "<pre style='color:red'>Error: {error:?}</pre>");
        }
        Self(res)
    }

    pub(crate) fn into_html(self) -> String {
        self.0
    }

    pub(crate) fn zip<T0, T1, E0, E1>(
        r0: Result<T0, E0>,
        r1: Result<T1, E1>,
    ) -> Result<(T0, T1), Self>
    where
        E0: Borrow<anyhow::Error>,
        E1: Borrow<anyhow::Error>,
    {
        match (r0, r1) {
            (Ok(v0), Ok(v1)) => Ok((v0, v1)),
            (Ok(_), Err(e1)) => Err(Self::new([e1.borrow()])),
            (Err(e0), Ok(_)) => Err(Self::new([e0.borrow()])),
            (Err(e0), Err(e1)) => Err(Self::new([e0.borrow(), e1.borrow()])),
        }
    }
}

impl From<&anyhow::Error> for ErrorPage {
    fn from(e: &anyhow::Error) -> Self {
        Self::new([e])
    }
}

impl From<anyhow::Error> for ErrorPage {
    fn from(e: anyhow::Error) -> Self {
        Self::new([&e])
    }
}

pub(crate) fn write_file<P: AsRef<Path>, D: AsRef<[u8]>>(path: P, data: D) -> anyhow::Result<()> {
    let path = path.as_ref();
    make_parents(path)?;
    fs::write(path, data)
        .with_context(|| format!("couldn't write asset to `{}`", path.display()))?;

    Ok(())
}

pub(crate) fn make_parents<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create dir `{}`", parent.display()))?;
    }
    Ok(())
}

pub(crate) mod bump {
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn alloc_str_concat<'bump>(bump: &'bump Bump, data: &[&str]) -> &'bump mut str {
        let total_len = data
            .iter()
            .fold(0_usize, |len, s| len.checked_add(s.len()).unwrap());
        let mut bytes = data.iter().flat_map(|s| s.bytes());
        let s = bump.alloc_slice_fill_with(total_len, |_| bytes.next().unwrap());
        unsafe { str::from_utf8_unchecked_mut(s) }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn strings() {
            let bump = Bump::new();
            let res = alloc_str_concat(&bump, &["hello ", "", "world"]);
            assert_eq!(res, "hello world");
        }

        use super::alloc_str_concat;
        use bumpalo::Bump;
    }

    use bumpalo::Bump;
    use std::str;
}

pub(crate) mod precision_date {
    /// A date with some precision.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum PrecisionDate {
        Year(u32),
        Month(u32, Month),
        Day(NaiveDate),
    }

    impl PrecisionDate {
        pub fn year(self) -> u32 {
            match self {
                PrecisionDate::Year(year) | PrecisionDate::Month(year, _) => year,
                PrecisionDate::Day(date) => u32::try_from(date.year()).unwrap(),
            }
        }
    }

    impl Display for PrecisionDate {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            if f.alternate() {
                write!(f, "{:04}", self.year())
            } else {
                match self {
                    PrecisionDate::Year(year) => write!(f, "{year:04}"),
                    PrecisionDate::Month(year, month) => {
                        write!(f, "{year:04}-{:02}", month.number_from_month())
                    }
                    PrecisionDate::Day(date) => Display::fmt(date, f),
                }
            }
        }
    }

    impl FromStr for PrecisionDate {
        type Err = ParseError;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let mut parts = s.splitn(3, '-');

            let year = parts.next().unwrap();
            if year.len() != 4 || year.chars().any(|c| !c.is_ascii_digit()) {
                return Err(ParseError("year is not 4 digits".to_owned()));
            }
            let year = year.parse::<u32>().unwrap();

            let Some(month) = parts.next() else {
                return Ok(PrecisionDate::Year(year));
            };

            if month.len() != 2 || month.chars().any(|c| !c.is_ascii_digit()) {
                return Err(ParseError("month is not 2 digits".to_owned()));
            }
            let month = Month::from_u8(month.parse::<u8>().unwrap())
                .ok_or_else(|| ParseError(format!("month {month} is not in the range [1, 12]")))?;

            let Some(day) = parts.next() else {
                return Ok(PrecisionDate::Month(year, month));
            };

            if day.len() != 2 || day.chars().any(|c| !c.is_ascii_digit()) {
                return Err(ParseError("day is not 2 digits".to_owned()));
            }
            let day = day.parse::<u8>().unwrap();

            let year = i32::try_from(year).unwrap();
            let month = month.number_from_month();
            let date = NaiveDate::from_ymd_opt(year, month, u32::from(day)).ok_or_else(|| {
                ParseError(format!("date {year:04}-{month:02}-{day:02} is not real"))
            })?;

            Ok(PrecisionDate::Day(date))
        }
    }

    pub(crate) struct ParseError(String);

    impl Display for ParseError {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write!(f, "invalid date: {}", self.0)
        }
    }

    use chrono::Datelike;
    use chrono::Month;
    use chrono::NaiveDate;
    use num_traits::FromPrimitive;
    use std::fmt;
    use std::fmt::Display;
    use std::fmt::Formatter;
    use std::str::FromStr;
}

use self::push_str::push;
use anyhow::Context as _;
use std::borrow::Borrow;
use std::fs;
use std::path::Path;
