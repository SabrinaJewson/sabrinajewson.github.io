//! Makefile-like system.

use ::{
    anyhow::Context as _,
    once_cell::sync::Lazy,
    std::{
        cell::Cell,
        env, fs,
        path::{Path, PathBuf},
        time::SystemTime,
    },
};

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
    fn modifies_path<E, P: AsRef<Path>>(self, path: P) -> ModifiesPath<Self, P>
    where
        Self: Asset<Output = Result<(), E>> + Sized,
    {
        ModifiesPath::new(self, path)
    }

    /// Output the asset to a file.
    ///
    /// Conceptually this is just a `.map` that writes the file followed by a `.modifies_path`.
    fn to_file<P: AsRef<Path>>(self, path: P) -> ToFile<Self, P>
    where
        Self: Sized,
        Self::Output: AsRef<[u8]>,
    {
        ToFile::new(self, path)
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub(crate) enum Modified {
    Never,
    At(SystemTime),
    Now,
}

impl Modified {
    fn path<P: AsRef<Path>>(path: P) -> Option<Self> {
        path.as_ref()
            .metadata()
            .and_then(|meta| meta.modified())
            .map(Self::At)
            .ok()
    }
}

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
impl<E, A: Asset<Output = Result<(), E>>, P: AsRef<Path>> Asset for ModifiesPath<A, P> {
    type Output = Result<(), E>;

    fn modified(&self) -> Modified {
        Modified::path(&self.path).unwrap_or(Modified::Now)
    }
    fn generate(&self) -> Self::Output {
        let output_modified = Modified::path(&self.path).unwrap_or(Modified::Never);
        if self.asset.modified() > output_modified || *EXE_MODIFIED > output_modified {
            self.asset.generate()?;
        }
        Ok(())
    }
}

pub(crate) struct ToFile<A, P> {
    asset: A,
    path: P,
}
impl<A, P> ToFile<A, P> {
    fn new(asset: A, path: P) -> Self {
        Self { asset, path }
    }
}
impl<A: Asset, P: AsRef<Path>> Asset for ToFile<A, P>
where
    A::Output: AsRef<[u8]>,
{
    type Output = anyhow::Result<()>;

    fn modified(&self) -> Modified {
        Modified::path(&self.path).unwrap_or(Modified::Now)
    }
    fn generate(&self) -> Self::Output {
        let output = self.path.as_ref();
        let output_modified = Modified::path(output).unwrap_or(Modified::Never);
        if self.asset.modified() > output_modified || *EXE_MODIFIED > output_modified {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create dir `{}`", parent.display()))?;
            }

            fs::write(&output, self.asset.generate())
                .with_context(|| format!("couldn't write asset to `{}`", output.display()))?;
        }
        Ok(())
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

impl_for_refs!(&A, std::rc::Rc<A>);

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
impl_for_tuples!(A B C D E F G);

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

pub(crate) struct Constant<C> {
    constant: C,
}
impl<C> Constant<C> {
    pub(crate) fn new(constant: C) -> Self {
        Self { constant }
    }
}
impl<C: Clone> Asset for Constant<C> {
    type Output = C;

    fn modified(&self) -> Modified {
        Modified::Never
    }
    fn generate(&self) -> Self::Output {
        self.constant.clone()
    }
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
        Modified::path(&self.path).unwrap_or(Modified::Now)
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
        Modified::path(&self.path).unwrap_or(Modified::Now)
    }
    fn generate(&self) -> Self::Output {
        let path = self.path.as_ref();
        fs::read_to_string(&path)
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
        Modified::path(&self.path).unwrap_or(Modified::Now)
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
