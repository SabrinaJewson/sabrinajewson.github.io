//! Makefile-like system.

pub(crate) trait Asset {
    type Output;

    /// Get the time at which [`Self::generate`] started returning the value that it did.
    ///
    /// This can be used to avoid calling `generate` again, since that may be expensive.
    fn modified(&self) -> Modified;

    /// Generate the asset's value.
    fn generate(&self) -> Self::Output;

    fn map<O, F: Fn(Self::Output) -> O>(self, f: F) -> Map<Self, F>
    where
        Self: Sized,
    {
        Map::new(self, f)
    }

    fn flatten(self) -> Flatten<Self>
    where
        Self: Sized,
        Self::Output: Asset,
    {
        Flatten::new(self)
    }

    /// Cache the result of this asset.
    fn cache(self) -> Cache<Self>
    where
        Self: Sized,
        Self::Output: Clone,
    {
        Cache::new(self)
    }

    /// Cache the output of the asset based on the fact that it modifies a certain path.
    ///
    /// `to_file` already does this caching, so it's not necessary to apply after that.
    fn modifies_path<P: AsRef<Path>>(self, path: P) -> ModifiesPath<Self, P>
    where
        Self: Asset<Output = ()> + Sized,
    {
        ModifiesPath::new(self, path)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub(crate) enum Modified {
    Never,
    At(SystemTime),
}

impl Modified {
    fn path<P: AsRef<Path>>(path: P) -> Option<Self> {
        path.as_ref()
            .symlink_metadata()
            .and_then(|meta| meta.modified())
            .map(Self::At)
            .ok()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Map<A, F> {
    asset: A,
    f: F,
}
impl<A, F> Map<A, F> {
    fn new(asset: A, f: F) -> Self {
        Self { asset, f }
    }
}
impl<A: Asset, F: Fn(A::Output) -> O, O> Asset for Map<A, F> {
    type Output = O;

    fn modified(&self) -> Modified {
        self.asset.modified()
    }
    fn generate(&self) -> Self::Output {
        (self.f)(self.asset.generate())
    }
}

pub(crate) struct Flatten<A> {
    asset: A,
}
impl<A> Flatten<A> {
    fn new(asset: A) -> Self {
        Self { asset }
    }
}
impl<A: Asset> Asset for Flatten<A>
where
    A::Output: Asset,
{
    type Output = <A::Output as Asset>::Output;

    fn modified(&self) -> Modified {
        Ord::max(self.asset.modified(), self.asset.generate().modified())
    }
    fn generate(&self) -> Self::Output {
        self.asset.generate().generate()
    }
}

pub(crate) struct Cache<A: Asset> {
    asset: A,
    cached: Cell<Option<(Modified, A::Output)>>,
}
impl<A: Asset> Cache<A> {
    fn new(asset: A) -> Self {
        Self {
            asset,
            cached: Cell::new(None),
        }
    }
}
impl<A: Asset> Asset for Cache<A>
where
    A::Output: Clone,
{
    type Output = A::Output;

    fn modified(&self) -> Modified {
        self.asset.modified()
    }
    fn generate(&self) -> Self::Output {
        let inner_modified = self.asset.modified();
        let (last_modified, output) = self
            .cached
            .take()
            .filter(|&(last_modified, _)| last_modified >= inner_modified)
            .unwrap_or_else(|| (inner_modified, self.asset.generate()));
        self.cached.set(Some((last_modified, output.clone())));
        output
    }
}

static EXE_MODIFIED: Lazy<Modified> = Lazy::new(|| {
    env::current_exe()
        .ok()
        .and_then(Modified::path)
        .unwrap_or_else(|| Modified::At(SystemTime::now()))
});

pub(crate) struct ModifiesPath<A, P> {
    asset: A,
    path: P,
}
impl<A, P> ModifiesPath<A, P> {
    fn new(asset: A, path: P) -> Self {
        Self { asset, path }
    }
}
impl<A, P: AsRef<Path>> Asset for ModifiesPath<A, P>
where
    A: Asset<Output = ()>,
{
    type Output = ();

    fn modified(&self) -> Modified {
        Modified::path(&self.path).unwrap_or(Modified::Never)
    }
    fn generate(&self) -> Self::Output {
        let output_modified = self.modified();
        if self.asset.modified() >= output_modified || *EXE_MODIFIED >= output_modified {
            self.asset.generate();
        }
    }
}

macro_rules! impl_for_refs {
    ($($ty:ty),*) => { $(
        impl<A: Asset + ?Sized> Asset for $ty {
            type Output = A::Output;

            fn modified(&self) -> Modified {
                (**self).modified()
            }
            fn generate(&self) -> Self::Output {
                (**self).generate()
            }
        }
    )* };
}

impl_for_refs!(&A, Box<A>, std::rc::Rc<A>);

pub(crate) fn all<T: IntoAll>(into_all: T) -> T::All {
    into_all.into_all()
}

pub(crate) trait IntoAll: Sized {
    type All: Asset;
    fn into_all(self) -> Self::All;
}

macro_rules! impl_for_tuples {
    (@$_:ident) => {};
    (@$first:ident $($ident:ident)*) => {
        impl_for_tuples!($($ident)*);
    };
    ($($ident:ident)*) => {
        #[allow(non_snake_case)]
        const _: () = {
            pub(crate) struct All<$($ident,)*>($($ident,)*);
            impl<$($ident: Asset,)*> Asset for All<$($ident,)*> {
                type Output = ($(<$ident as Asset>::Output,)*);

                #[allow(unused_mut)]
                fn modified(&self) -> Modified {
                    let Self($($ident,)*) = self;
                    let mut latest = Modified::Never;
                    $(latest = Ord::max(latest, $ident.modified());)*
                    latest
                }
                #[allow(clippy::unused_unit)]
                fn generate(&self) -> Self::Output {
                    let Self($($ident,)*) = self;
                    ($($ident.generate(),)*)
                }
            }

            impl<$($ident: Asset,)*> IntoAll for ($($ident,)*) {
                type All = All<$($ident,)*>;
                fn into_all(self) -> Self::All {
                    let ($($ident,)*) = self;
                    All($($ident,)*)
                }
            }
        };
        impl_for_tuples!(@$($ident)*);
    };
}
impl_for_tuples!(A B C D E F G H I);

macro_rules! impl_for_seq {
    ($($ty:ty),*) => { $(
        const _: () = {
            pub(crate) struct All<A>($ty);

            impl<A: Asset> Asset for All<A> {
                // TODO: don't allocate? I think that would need GATs
                type Output = Box<[A::Output]>;

                fn modified(&self) -> Modified {
                    self.0.iter().map(A::modified).max().unwrap_or(Modified::Never)
                }
                fn generate(&self) -> Self::Output {
                    self.0.iter().map(A::generate).collect()
                }
            }

            impl<A: Asset> IntoAll for $ty {
                type All = All<A>;
                fn into_all(self) -> Self::All {
                    All(self)
                }
            }
        };
    )* };
}
impl_for_seq!(Box<[A]>, std::rc::Rc<[A]>, Vec<A>);

pub(crate) struct Constant<T> {
    value: T,
}
impl<T> Constant<T> {
    pub(crate) fn new(value: T) -> Self {
        Self { value }
    }
}
impl<T: Clone> Asset for Constant<T> {
    type Output = T;

    fn modified(&self) -> Modified {
        Modified::Never
    }
    fn generate(&self) -> Self::Output {
        self.value.clone()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Dynamic<T> {
    value: T,
    created: SystemTime,
}
impl<T> Dynamic<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            value,
            created: SystemTime::now(),
        }
    }
}
impl<T: Clone> Asset for Dynamic<T> {
    type Output = T;

    fn modified(&self) -> Modified {
        Modified::At(self.created)
    }
    fn generate(&self) -> Self::Output {
        self.value.clone()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Volatile;
impl Asset for Volatile {
    type Output = ();

    fn modified(&self) -> Modified {
        Modified::At(SystemTime::now())
    }
    fn generate(&self) -> Self::Output {}
}

/// No-op asset that sources its modification time from a path on the filesystem.
pub(crate) struct FsPath<P> {
    path: P,
}
impl<P: AsRef<Path>> FsPath<P> {
    pub(crate) fn new(path: P) -> Self {
        Self { path }
    }
}
impl<P: AsRef<Path>> Asset for FsPath<P> {
    type Output = ();

    fn modified(&self) -> Modified {
        Modified::path(&self.path).unwrap_or(Modified::Never)
    }
    fn generate(&self) -> Self::Output {}
}

/// Asset that reads in an entire file as UTF-8.
///
/// Conceptually `FsPath` followed by `fs::read_to_string`.
pub(crate) struct TextFile<P> {
    path: P,
}
impl<P: AsRef<Path>> TextFile<P> {
    pub(crate) fn new(path: P) -> Self {
        Self { path }
    }
}
impl<P: AsRef<Path>> Asset for TextFile<P> {
    type Output = anyhow::Result<String>;

    fn modified(&self) -> Modified {
        Modified::path(&self.path).unwrap_or(Modified::Never)
    }
    fn generate(&self) -> Self::Output {
        let path = self.path.as_ref();
        fs::read_to_string(path)
            .with_context(|| format!("failed to read file `{}`", path.display()))
    }
}

/// Asset that reads the top-level contents of a directory.
///
/// Conceptually `FsPath` followed by `fs::read_dir`.
pub(crate) struct Dir<P> {
    path: P,
}
impl<P: AsRef<Path>> Dir<P> {
    pub(crate) fn new(path: P) -> Self {
        Self { path }
    }
}
impl<P: AsRef<Path>> Asset for Dir<P> {
    type Output = anyhow::Result<DirFiles>;

    fn modified(&self) -> Modified {
        Modified::path(&self.path).unwrap_or(Modified::Never)
    }
    fn generate(&self) -> Self::Output {
        let path = self.path.as_ref();
        Ok(DirFiles {
            iter: fs::read_dir(path)
                .with_context(|| format!("failed to open directory `{}`", path.display()))?,
            path: path.to_owned(),
        })
    }
}

pub(crate) struct DirFiles {
    iter: fs::ReadDir,
    path: PathBuf,
}

impl Iterator for DirFiles {
    type Item = anyhow::Result<PathBuf>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(
            self.iter
                .next()?
                .map(|entry| entry.path())
                .with_context(|| format!("failed to read directory `{}`", self.path.display())),
        )
    }
}

use anyhow::Context as _;
use once_cell::sync::Lazy;
use std::cell::Cell;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
