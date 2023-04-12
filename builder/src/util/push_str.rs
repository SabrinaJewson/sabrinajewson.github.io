/// Infallible write trait.
pub(crate) trait PushStr {
    fn push_str(&mut self, s: &str);
    fn writer(&mut self) -> PushWriter<'_, Self> {
        PushWriter(self)
    }
}

impl<W: PushStr + ?Sized> PushStr for &mut W {
    fn push_str(&mut self, s: &str) {
        (**self).push_str(s);
    }
}

impl PushStr for String {
    fn push_str(&mut self, s: &str) {
        self.push_str(s);
    }
}

macro_rules! push {
    ($target:expr, $($fmt:tt)*) => {{
        #[allow(unused_imports)]
        use {
            $crate::util::push_str::PushStr as _,
            core::fmt::Write as _,
        };
        write!($target.writer(), $($fmt)*).unwrap()
    }}
}
pub(crate) use push;

pub(crate) struct PushWriter<'a, T: ?Sized>(&'a mut T);

impl<T: ?Sized + PushStr> fmt::Write for PushWriter<'_, T> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.push_str(s);
        Ok(())
    }
}

impl<T: ?Sized + PushStr> pulldown_cmark::escape::StrWrite for PushWriter<'_, T> {
    fn write_str(&mut self, s: &str) -> io::Result<()> {
        self.0.push_str(s);
        Ok(())
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> io::Result<()> {
        fmt::write(self, args).unwrap();
        Ok(())
    }
}

pub(crate) fn escape_html(buf: &mut impl PushStr, s: &str) {
    pulldown_cmark::escape::escape_html(buf.writer(), s).unwrap();
}

pub(crate) fn escape_href(buf: &mut impl PushStr, s: &str) {
    pulldown_cmark::escape::escape_html(buf.writer(), s).unwrap();
}

use std::fmt;
use std::io;
