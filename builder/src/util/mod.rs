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
    use ::{
        bumpalo::Bump,
        std::{alloc, ptr, slice, str},
    };

    pub(crate) fn alloc_str_concat<'bump, const N: usize>(
        bump: &'bump Bump,
        data: [&str; N],
    ) -> &'bump mut str {
        let s = alloc_slice_concat_copy(bump, data.map(|s| s.as_bytes()));
        unsafe { str::from_utf8_unchecked_mut(s) }
    }

    pub(crate) fn alloc_slice_concat_copy<'bump, T, const N: usize>(
        bump: &'bump Bump,
        data: [&[T]; N],
    ) -> &'bump mut [T]
    where
        T: Copy,
    {
        let total_len = data.iter().map(|slice| slice.len()).sum::<usize>();
        let layout = alloc::Layout::array::<T>(total_len).unwrap();
        let pointer = bump.alloc_layout(layout).as_ptr().cast::<T>();
        let mut i = 0;
        for slice in data {
            let dst = unsafe { pointer.add(i) };
            unsafe { ptr::copy_nonoverlapping(slice.as_ptr(), dst, slice.len()) };
            i += slice.len();
        }
        unsafe { slice::from_raw_parts_mut(pointer, total_len) }
    }

    #[cfg(test)]
    mod tests {
        use super::alloc_slice_concat_copy;
        use ::bumpalo::Bump;

        #[test]
        fn slices() {
            let bump = Bump::new();
            let res = alloc_slice_concat_copy(&bump, [&[0, 1, 2], &[3, 4], &[], &[5]]);
            assert_eq!(res, [0, 1, 2, 3, 4, 5]);
        }
    }
}
