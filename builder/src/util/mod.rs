//! This module contains many small independent components.

use self::push_str::push;
use ::{
    anyhow::Context as _,
    std::{fs, path::Path},
};

pub(crate) mod asset;
pub(crate) mod markdown;
pub(crate) mod minify;
pub(crate) mod push_str;

pub(crate) fn log_errors<T>(res: anyhow::Result<T>) {
    if let Err(e) = res {
        log::error!("{e:?}");
    }
}

pub(crate) fn error_page<'a, I: IntoIterator<Item = &'a anyhow::Error>>(errors: I) -> String {
    let mut res = String::new();
    for error in errors {
        log::error!("{error:?}");
        push!(res, "<p style='color:red'>Error: {error:?}</p>");
    }
    res
}

pub(crate) fn write_file<P: AsRef<Path>, D: AsRef<[u8]>>(path: P, data: D) -> anyhow::Result<()> {
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create dir `{}`", parent.display()))?;
    }

    fs::write(path, data)
        .with_context(|| format!("couldn't write asset to `{}`", path.display()))?;

    Ok(())
}

pub(crate) mod bump {
    use ::{bumpalo::Bump, std::str};

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
        use super::alloc_str_concat;
        use ::bumpalo::Bump;

        #[test]
        fn strings() {
            let bump = Bump::new();
            let res = alloc_str_concat(&bump, &["hello ", "", "world"]);
            assert_eq!(res, "hello world");
        }
    }
}
