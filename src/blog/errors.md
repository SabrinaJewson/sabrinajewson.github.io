{
	"published": "2023-04-08"
}

# Modular Errors in Rust

It is thankfully common wisdom nowadays that
documentation must be placed as near as possible to the code it documents,
and should be fine-grained to a minimal unit of describability
(the thing being documented).
The practice provides numerous benefits to the codebase and project as a whole:

1. When editing the source code,
	contributors are less likely to forget to update the documentation as well,
	ensuring it is kept up-to-date and accurate.
2. When reading the source code,
	reviewers can easily jump back and forth between the docs and the code it documents,
	helping them understand it and allowing them to contrast the expected with actual behaviour.
3. The codebase becomes more modular.
	Individual parts can be extracted into different crates or projects if necessary,
	and strong abstraction boundaries make the code easier to understand in small pieces.

But you probably already knew this;
after all,
Rust made the excellent design choice of making it
the by far easiest method of writing documentation at all.
And you probably also know that these same principles apply to tests:
when unit tests are kept next to their minimum unit of checkability,
you get the same benefits of
convenient updating, assisted understanding and modularity.
And most Rust projects do use unit tests in this way
(when they can, for often there are limitations that prevent it from working),
which again we can thank the tooling for.

But that’s all old news.
What I’m here to convince you of today is that
this principle applies additionally to _error types_:
that is, error types should be located near to their unit of fallibility.
To illustrate this point, I will follow the initial development
and later API improvement of a hypothetical Rust library.

## Case Study: A Blocks.txt Parser { #blocks-txt }

Suppose you’re a library author,
and you’re working on a crate to implement the parsing of [Blocks.txt]
in the [Unicode Character Database].
If you’re not familiar with this file,
it defines the list of so-called [Unicode blocks],
which are non-overlapping contiguous categories that Unicode characters can be sorted into.
It looks a bit like this:

```
0000..007F; Basic Latin
0080..00FF; Latin-1 Supplement
0100..017F; Latin Extended-A
0180..024F; Latin Extended-B
0250..02AF; IPA Extensions
```

This file tells you that, for example,
the character “½”, `U+00BD`, is in the block “Latin-1 Supplement”
because `0x0080 ≤ 0x00BD ≤ 0x00FF`.
Every character has an associated block;
characters which have not yet been assigned a block in the file above
are considered to be in the special pseudo-block `No_Block`.

So let’s get started on a Rust parser.
The specification for the format is given by [section 4.2 of Unicode Annex #44],
but the format is so trivial you could almost guess it.
Upon seeing this task, a typical Rustacean may write code like this:

```rust
//! This crate provides tools for working with Unicode blocks and its data files.

pub struct Blocks {
	ranges: Vec<(RangeInclusive<u32>, String)>,
}

impl Blocks {
	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
		Self::from_str(&fs::read_to_string(path)?)
	}

	pub fn download(agent: &ureq::Agent) -> Result<Self, Error> {
		let response = agent.get(LATEST_URL).call()?;
		Self::from_str(&response.into_string()?)
	}
}

impl FromStr for Blocks {
	type Err = Error;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let ranges = s
			.lines()
			.map(|line| line.split_once('#').map(|(l, _)| l).unwrap_or(line))
			.filter(|line| !line.is_empty())
			.map(|line| {
				let (range, name) = line.split_once(';').ok_or(Error::NoSemicolon)?;
				let (range, name) = (range.trim(), name.trim());
				let (start, end) = range.split_once("..").ok_or(Error::NoDotDot)?;
				let start = u32::from_str_radix(start, 16)?;
				let end = u32::from_str_radix(end, 16)?;
				Ok((start..=end, name.to_owned()))
			})
			.collect::<Result<Vec<_>, Error>>()?;
		Ok(Self { ranges })
	}
}
```

Now we need to define an error type,
so let’s just follow the “big `[rs] #[non_exhaustive] enum`” convention
and bash out some boilerplate that gets the job done:

```rust
/// An error in this library.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
	NoSemicolon,
	NoDotDot,
	ParseInt(ParseIntError),
	Io(io::Error),
	Ureq(Box<ureq::Error>),
}

impl From<ParseIntError> for Error {
	fn from(error: ParseIntError) -> Self {
		Self::ParseInt(error)
	}
}

impl From<io::Error> for Error {
	fn from(error: io::Error) -> Self {
		Self::Io(error)
	}
}

impl From<ureq::Error> for Error {
	fn from(error: ureq::Error) -> Self {
		Self::Ureq(Box::new(error))
	}
}

impl Display for Error {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::NoSemicolon => f.write_str("no semicolon"),
			Self::NoDotDot => f.write_str("no `..` in character range"),
			Self::ParseInt(e) => Display::fmt(e, f),
			Self::Io(e) => Display::fmt(e, f),
			Self::Ureq(e) => Display::fmt(e, f),
		}
	}
}

impl std::error::Error for Error {}
```

Lastly, a couple other bits and imports go at the end:

```rust
pub const LATEST_URL: &str = "https://www.unicode.org/Public/UCD/latest/ucd/Blocks.txt";

use std::cmp;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fs;
use std::io;
use std::num::ParseIntError;
use std::ops::RangeInclusive;
use std::path::Path;
use std::str::FromStr;
```

And we’re done.

There are a few small things to note with this code just before we move on:

1. I omitted documentation, since it’s not relevant to the real example;
	in actual code, all the public items would be documented.
	Similarly, unit tests are omitted.
1. In a real library, one would not hard-depend on [ureq] and `std`
	and would use feature-flags instead,
	but again I omitted that for this example.
1. You might have noticed I put my imports on separate lines each at bottom —
	I do have my reasons for this, but that’s best saved for another day ;)
1. `Blocks` implements [`FromStr`], but not [`[rs] TryFrom<&str>`][TryFrom].
	This is actually intentional,
	because despite being nearly identical traits signature-wise
	they mean two very different things:
	`FromStr` implies _parsing_ from a string
	whereas `[rs] TryFrom<&str>` is for when your data type _is_ a subset of all strings.
	In our case, `FromStr` is the correct one to use.
1. The `Display` implementation of `Error` formats error messages like `no semicolon`
	in lowercase and without a full stop at the end —
	this is in accordance with [conventions established by the Standard Library][error convention]
	(“Error messages are typically concise lowercase sentences without trailing punctuation”).
	A common pitfall of both new and experienced Rustaceans is using incorrect casing
	for error messages.
1. Another common pitfall is naming things
	like what we’ve named `Error::Io`
	as `Error::IoError` instead.
	Simply: you don’t need the `Error` suffix, it says it in the name already!
1. One could use the [thiserror] crate to shorten the code by using a `[rs] #[derive(Error)]`.
	Personally, I would never use this for a library crate
	since it’s really not that many lines of code saved for a whole extra dependency,
	but you might want to know about it.
1. The `Ureq` variant of the `Error` enum is boxed
	because `ureq::Error` is actually very large
	and Clippy complains about it.

So there we have it:
our perfect little library,
let’s go off and publish it to crates.io.

What we’ve written so far, with regard to error handling,
is what I’d say most libraries on crates.io do.
It’s by far the most _common_ way of handling errors:
just stick everything in
a big `[rs] enum` of “different ways things can go wrong in the library”
and don’t think about it after that.
But unfortunately,
while it is _common_ it is not exactly _good_,
for a few reasons the rest of this post will be covering.

### Problem 1: Backtraces { #problem-1-backtraces }

Suppose you then decide to use your library in a CLI application;
and as per usual advice and your own experience,
you decide to use [anyhow] to handle the errors in it.
So you write out all your code and it looks a little like this:

```rs
fn main() -> anyhow::Result<()> {
	init_something()?;
	let blocks = Blocks::from_file("Blocks.txt")?;
	init_something_else()?;
	// Run the main code…
	Ok(())
}

use unicode_blocks::Blocks;
```

Looks good, so you go ahead and run it — only, you’re rather abruptly met with:

```
Error: invalid digit found in string
```

Um, okay.
That doesn’t help us very much at all.
What went wrong here?

Well, much pain and many `dbg!` statements later,
you discover that the culprit is that somehow,
on line 223 of `Blocks.txt` you replaced a `0` with an `O`. Oops!

```diff
--- Blocks.txt
+++ Blocks.txt
@@ -222,3 +222,3 @@
 10800..1083F; Cypriot Syllabary
-10840..1O85F; Imperial Aramaic
+10840..1085F; Imperial Aramaic
 10860..1087F; Palmyrene
```

And then you run it again and it works fine.

But it didn’t have to be this hard.
The error message _could_ have displayed something more useful,
and maybe this is just a pipe dream,
but I’ve seen `anyhow` emit this sort of thing before:

```
Error: error reading `Blocks.txt`

Caused by:
	0: invalid Blocks.txt data on line 223
	1: one end of range is not a valid hexidecimal integer
	2: invalid digit found in string
```

That’s so much more helpful — you wouldn’t ever have had to suspect
`init_something` and `init_something_else` as potential causes of the error,
or even search `Blocks.txt` for mistakes,
it completely guides you to exactly where it went wrong!

Oh well,
you say to yourself,
at least this time it was decently obvious where the source of the error came from;
at least I wasn’t getting a
[file not found error from `TcpListener::bind`][io::Error MAKES WE WANT TO DIE]
(the natural conclusion to this kind of “flat”-style error handling).
But wouldn’t it be nice if all errors came with backtrace and context tracking built-in?

### Problem 2: Inextensibility { #problem-2-inextensibility }

At least one of the things in the above image looks feasible to fix though:
adding line numbers as context to the error messages.
All we have to do is return to our `Error` enum
and add more fields to the `NoSemicolon`, and `NoDotDot`, and `ParseInt`, variants:

```rust
pub enum Error {
	NoSemicolon { line: usize },
	NoDotDot { line: usize },
	ParseInt { line: usize, source: ParseIntError },
	Io(io::Error),
	Ureq(Box<ureq::Error>),
}
```

Except… we can’t do that without breaking backward compatibility,
because while the enum itself is `[rs] #[non_exhaustive]`
the individual variants aren’t,
meaning you’ve fixed them to forever have the fields they do currently
(without breaking changes).

### Problem 3: Error Matching { #problem-3-error-matching }

Okay, so back to the application.
You’ve now realized that you still want to call `[rs] Blocks::from_file("Blocks.txt")`,
but if it fails with a “file not found” error
you actually want to download the file automatically
instead of exiting the program entirely.
We have to match on the `[rs] Result` for that:

```rs
let blocks = match Blocks::from_file("Blocks.txt") {
	Ok(blocks) => blocks,
	Err(unicode_blocks::Error::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
		// download and retry…
	}
};
```

Great!
But the compiler is yelling that the `[rs] match` arms aren’t exhaustive.
Not too hard to fix, let’s look at the cases we need to deal with:

- `NoSemicolon`, `NoDotDot`, `ParseInt`:
	Those are pretty obvious, they look like parsing errors, so we can just propagate them.
- `Io`: Other I/O errors than “file not found” can also safely be propagated.
- `Ureq`: Ummm…?
	Wait, is this function doing HTTP requests?
	Let me check the source code again…
	[please stand by…]
	oh okay, so it’s not.
	Then I could add an `[rs] unreachable!()` here which would be correct
	and indicates semantics nicely;
	on the other hand,
	nowhere is it written in the documentation of the API
	that it _won’t_ ever return this,
	so maybe I should just propagate it anyway?
- Oh, and I forgot, we added `#[non_exhaustive]` to `[rs] enum Error`
	so there’s always the possibility of it returning a variant that doesn’t exist yet.
	Well, I guess we can just propagate it anyway.

So, this situation isn’t ideal.
The library doesn’t document anywhere what errors a given function _can_ return,
so users are often left shooting in the dark.
From personal experience,
there have been many times I have seen an error variant which was appropriate for me to catch,
then I had to spend ages digging around in the source code
to find out whether it was _actually_ generated or not —
and even an answer to that doesn’t constitute an API guarantee
that it will or won’t be in future.

Another issue with the code that we’ve written
is that it’s entirely non-obvious that our `[rs] match` arm
refers specifically to the `Blocks.txt` file not being found.
The arm itself just says “check if an I/O not found error occurred”,
but in theory,
and especially for more complex functions,
an I/O not found error could mean one of several different things
that the user can no longer differentiate between
because they were all put together in a single `Io` variant.

### Problem 4: Privacy and Stability { #problem-4-privacy-and-stability }

One very common mistake libraries make with this style of big-`[rs] enum` error is
accidentally exposing dependencies intended to be private
in their public API through error types.
In our example code,
suppose `std::fs` and `io::Error` weren’t part of the standard library
but were rather types from an external library that was on version `0.4`.
Now, when they bump their version to `0.5`
I _also_ have to make a breaking change to update it to the newer version,
because I exposed the `io::Error` type in my public API through the `Error` enum,
even though I never expose my usage of the library anywhere else
(it’s covered up by the opaque interface of `from_file`).
The same issue occurs if I tried to switch out my usage of that library for a different one;
it also forbids me from ever releasing 1.0
until the dependency library also reaches 1.0 as per the [C-STABLE] API requirement.

This is hard to fix with this approach to errors,
because `[rs] enum` data is hardcoded to always use inherited visibility,
meaning if the outer `[rs] enum` fields are public _all_ inner fields are too.
Private fields are also useful in errors in general,
for reasons other than stability:
private fields are just generally
a nice feature to have on types.

### Problem 5: Non-Modularity { #problem-5-non-modularity }

And lastly,
touching back on what I mentioned at the beginning of this article:
this approach to error handling is _non-modular_.
I couldn’t easily take a component alone, like the parser,
and extract it to a different crate,
because I’d have to change many APIs or otherwise hack around it.
Every API is interconnected with each other through the underlying error type,
tying the crate together in a big knot that makes it difficult to
untangle and remove stuff.

This kind of non-modularity also makes the codebase more difficult to understand:
one is forced, to a greater degree, to learn the entire codebase at once
to work on it,
rather than learn it piece by piece,
a far preferable way of learning.

## Guidelines for Good Errors { #guidelines-for-good-errors }

So the current error type we have has problems.
But how do we fix them?
And this is where we bring in that principle from the start:

> Error types should be located near to their unit of fallibility.

The key phrase here is “unit of fallibility”.
What are the units of fallibility in our library?
Well, it’s certainly not _the library itself_ —
the library is just a way of interacting with Unicode blocks,
and it’s not like that can particularly fail.
The only libraries that _would_ have the entire library as a unit of fallibility
are those whose only purpose is to perform a single operation
(they typically have an API surface of no more than two functions,
maybe a `Params` builder type, and nothing more).

This tells us that the `unicode_blocks::Error` type is inherently misguided.
Rather, the units of fallibility in our case are the _operations we do_,
like downloading, reading a file, and parsing.

Now, things get a little subjective at this point
on deciding what counts as two separate units or the same unit.
In general, you should ask yourself the following two questions:
1. Do they have different ways in which they can fail?
1. Should they show different error messages should they fail?

If the answer to either of those questions is “yes”,
then they should normally be separate error types.

For us, this means we actually want _three_ separate error types:
1. `FromFileError`, for errors in `Blocks::from_file`;
2. `DownloadError`, for errors in `Blocks::download`;
3. `ParseError`, for errors in `from_str`.

### Leveraging the `.source()` method { #leveraging-source }

Earlier, we said we wanted our error messages (printed with `anyhow`) to look good, like this:

```
Error: error reading `Blocks.txt`

Caused by:
	0: invalid Blocks.txt data on line 223
	1: one end of range is not a valid hexidecimal integer
	2: invalid digit found in string
```

So how do we get `anyhow` to print this?
It turns out what the library calls internally is the [`Error::source()`] method,
a default-implemented method of the `Error` trait
that tells you the cause of an error.
What we see in the above graphic depicts:
1. an error type (we know to be `FromFileError`)
	whose `Display` implementation prints “error reading `Blocks.txt`”,
	and whose `source` is…
2. …another error type,
	whose `Display` implementation prints “invalid Blocks.txt data on line 223”,
	and whose `source` is…
3. …another error type,
	whose `Display` implementation prints “one end of range is not a valid hexidecimal integer”,
	and whose `source` is…
4. …another error type (we know to be `ParseIntError`)
	whose `Display` implementation prints “invalid digit found in string”
	and whose `source` is `None`.

That might seem like a lot of layers,
but they all map very nicely to our code:
layer 1 is a `FromFileError`,
layer 2 has to be our `ParseError`,
layer 3 has to be something contained within the `ParseError`,
and layer 4 is `ParseIntError`.

This leads us to a much nicer structure for the error types in the `from_file` API.

```rust
#[derive(Debug)]
#[non_exhaustive]
pub struct FromFileError {
	pub path: Box<Path>,
	pub kind: FromFileErrorKind,
}

impl Display for FromFileError {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "error reading `{}`", self.path.display())
	}
}

impl Error for FromFileError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match &self.kind {
			FromFileErrorKind::ReadFile(e) => Some(e),
			FromFileErrorKind::Parse(e) => Some(e),
		}
	}
}

#[derive(Debug)]
pub enum FromFileErrorKind {
	ReadFile(io::Error),
	Parse(ParseError),
}
```

This error:
- has very good backtraces, as it implements `Display` and `source()` well;
- is extensible, as the `[rs] struct` is attributed with `[rs] #[non_exhaustive]`;
- supports precise error matching,
	as we’ve now _automatically_ given the public API guarantee that
	we won’t produce HTTP errors from our function,
	so our users needn’t worry about dealing with that case;
- makes it clear where the `io::Error`s can come from,
	because the variant is named `ReadFile` instead of simply `Io`;
- would easily be able to adjust to support hiding `io::Error` from the public API surface
	simply by making `kind` and `FromFileErrorKind` private;
- is entirely modular,
	being conceptually contained within the `from_file` logic portion of the code,
	so it can be extracted, learnt independently, et cetera.

`ParseError` can be defined in a somewhat similar fashion,
also with the above benefits.

```rust
#[derive(Debug)]
#[non_exhaustive]
pub struct ParseError {
	pub line: usize,
	pub kind: ParseErrorKind,
}

impl Display for ParseError {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "invalid Blocks.txt data on line {}", self.line + 1)
	}
}

impl Error for ParseError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		Some(&self.kind)
	}
}

#[derive(Debug)]
pub enum ParseErrorKind {
	#[non_exhaustive]
	NoSemicolon,
	#[non_exhaustive]
	NoDotDot,
	#[non_exhaustive]
	ParseInt { source: ParseIntError },
}

impl Display for ParseErrorKind {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match *self {
			Self::NoSemicolon => f.write_str("no semicolon"),
			Self::NoDotDot => f.write_str("no `..` in range"),
			Self::ParseInt { .. } => {
				f.write_str("one end of range is not a valid hexadecimal integer")
			}
		}
	}
}

impl Error for ParseErrorKind {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::ParseInt { source } => Some(source),
			_ => None,
		}
	}
}
```

Note that the `[rs] enum` variants themselves are `[rs] #[non_exhaustive]`,
so that they can be extended in future with more information.

There is a slight deviation from `FromFileError`’s design here,
that its corresponding `*Kind` type
actually implements `Display` and `Error` in and of itself
instead of simply existing as a data holder for other error types.
The logic is that while we could separate make unit structs
for `NoSemicolon`, `NoDotDot` and `ParseInt`,
it just isn’t very necessary here
(where on the other hand `io::Error` is an external type
and `ParseError` is required to be a distinct type because of `FromStr`).
However, sometimes it is still better to make unit structs:
it depends on the use case.

Finally, `DownloadError` showcases a similar pattern
(although it’s not that interesting at this point):

```rust
#[derive(Debug)]
#[non_exhaustive]
pub struct DownloadError {
	pub kind: DownloadErrorKind,
}

impl Display for DownloadError {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "failed to download Blocks.txt from the Unicode website")
	}
}

impl Error for DownloadError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match &self.kind {
			DownloadErrorKind::Request(e) => Some(e),
			DownloadErrorKind::ReadBody(e) => Some(e),
			DownloadErrorKind::Parse(e) => Some(e),
		}
	}
}

#[derive(Debug)]
pub enum DownloadErrorKind {
	Request(Box<ureq::Error>),
	ReadBody(io::Error),
	Parse(ParseError),
}
```

Note that we could have merged `DownloadErrorKind` and `DownloadError` into a single type;
I chose not to here in favour of extensibility,
because it seems quite possible that
one would want to add more fields to `DownloadError` in future.
But for some cases it definitely makes sense.

### Constructing the error types { #constructing-the-error-types }

If you try to implement the functions that return these error types,
you’ll quickly run into something rather annoying:
they require quite a bit of boilerplate to use.
For example, the body of `from_file` now looks like this:

```rust
pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, FromFileError> {
	let path = path.as_ref();
	(|| {
		let s = fs::read_to_string(path).map_err(FromFileErrorKind::ReadFile)?;
		Self::from_str(&s).map_err(FromFileErrorKind::Parse)
	})()
	.map_err(|kind| FromFileError {
		path: path.into(),
		kind,
	})
}
```

Yeah, not the prettiest.
Unfortunately, I don’t think there’s much we can actually do here;
once we get `try` blocks it’ll definitely be nicer,
but it seems to be an unavoidable cost of many good error-handling schemes.

### On `From` { #on-from }

One thing notably omitted from the definitions of the new error types
was implementations of `From` for inner types.
There is no problem with them really,
one just has to be careful that it (a) works with extensibility and (b) actually makes _sense_.
For example, taking `FromFileErrorKind`:

```rust
#[derive(Debug)]
pub enum FromFileErrorKind {
	ReadFile(io::Error),
	Parse(ParseError),
}
```

While it does make sense to implement `From<ParseError>`,
because `Parse` is literally the name of one of the variants of `FromFileErrorKind`,
it does not make sense to implement `From<io::Error>`
because such an implementation would
implicitly add meaning that one failed during the process of reading the file from disk
(as the variant is named `ReadFile` instead of `Io`).
Constraining the meaning of “any I/O error”
to “an error reading the file from the disk”
is helpful but should not be done implicitly,
thus rendering `From` inappropriate.

### On “nearness” { #on-nearness }

One part of my principle of errors I haven’t yet touched on
is the aspect of “nearness”;
that errors should,
as well as having an appropriate associated unit of fallibility,
be sufficiently _near_ to it.
The fact is,
with Rust’s current design you can’t put them as close as I’d like
without sacrificing documentation quality.
That is, while you’d ideally write something like:

```rs
impl Blocks {
	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, FromFileError> { /* … */ }
}

pub struct FromFileError { /* … */ }

impl Blocks {
	pub fn download(agent: &ureq::Agent) -> Result<Self, DownloadError> { /* … */ }
}

pub struct DownloadError { /* … */ }
```

This just makes your rustdoc look bad,
since the `[rs] impl` blocks are needlessly separated.
So usually I end up writing something more like:

```rs
impl Blocks {
	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, FromFileError> { /* … */ }
	pub fn download(agent: &ureq::Agent) -> Result<Self, DownloadError> { /* … */ }
}
pub struct FromFileError { /* … */ }
pub struct DownloadError { /* … */ }
```

It’s unfortunate, but I don’t think it’s terrible —
you still get most the benefits of nearness.

The only thing to make sure of is that they stay in the same module;
this same concept of “nearness” is a similar reason why
one should be extremely wary of any module named “errors”,
which is of equal organizational value
to having a drawer labelled “medium-sized and flat”.

### Verbosity { #verbosity }

Possibly the biggest objection to this style of error is the sheer number of lines of code
required to implement it;
error types aren’t a trivial number of lines,
and making a new error type for every function
can easily hugely increase the number of lines a library needs.
This is definitely a valid criticism,
I also find it tiresome to write the same things over and over again,
but let me also offer an alternate perspective:
rather than seeing it as simply a more verbose way to do the same thing,
see it as due treatment for an oft ignored area.

Traditionally, errors as something to be pushed to the side as soon as possible
to get on with “real” logic.
But the art of resilient, reliable and user-friendly systems
considers _all_ outcomes,
not just the successful one.
As a success story, look no further than the Rust compiler itself;
I don’t think it would be an exaggeration to say that
Rust enjoys the current popularity it does _because of_ how good its error messages are,
and how much effort was put into it.

## Conclusion { #conclusion }

This post is not here to give you a structure that you should follow for your errors.
The structure I used as an example in this post had one specific use case,
and filled it appropriately.
If you find you can apply the same structure to your own code and it works well,
then great!
But really, what post is for is to get people to _start caring_ about errors,
putting actual thought into their designs,
and learning how to elegantly pull off	ever-present balancing act
between the five goals of
good backtraces,
extensibility,
inspectability (matching),
stability and
modularity.

If there’s one thing I wish for you to take away,
it’s that error handling is _hard_,
but it’s _worth it_ to learn.
Because I’m tired of having to deal with lazy kitchen-sink-type errors.

<details><summary>The final code</summary>

```rust
//! This crate provides types for UCD’s `Blocks.txt`.

pub struct Blocks {
	ranges: Vec<(RangeInclusive<u32>, String)>,
}

impl Blocks {
	pub fn block_of(&self, c: char) -> &str {
		self.ranges
			.binary_search_by(|(range, _)| {
				if *range.end() < u32::from(c) {
					cmp::Ordering::Less
				} else if u32::from(c) < *range.start() {
					cmp::Ordering::Greater
				} else {
					cmp::Ordering::Equal
				}
			})
			.map(|i| &*self.ranges[i].1)
			.unwrap_or("No_Block")
	}
	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, FromFileError> {
		let path = path.as_ref();
		(|| {
			Self::from_str(&fs::read_to_string(path).map_err(FromFileErrorKind::ReadFile)?)
				.map_err(FromFileErrorKind::Parse)
		})()
		.map_err(|kind| FromFileError {
			path: path.into(),
			kind,
		})
	}
	pub fn download(agent: &ureq::Agent) -> Result<Self, DownloadError> {
		(|| {
			let response = agent
				.get(LATEST_URL)
				.call()
				.map_err(|e| DownloadErrorKind::Request(Box::new(e)))?;
			Self::from_str(
				&response
					.into_string()
					.map_err(DownloadErrorKind::ReadBody)?,
			)
			.map_err(DownloadErrorKind::Parse)
		})()
		.map_err(|kind| DownloadError { kind })
	}
}

impl FromStr for Blocks {
	type Err = ParseError;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let ranges = s
			.lines()
			.enumerate()
			.map(|(i, line)| {
				(
					i,
					line.split_once('#').map(|(line, _)| line).unwrap_or(line),
				)
			})
			.filter(|(_, line)| !line.is_empty())
			.map(|(i, line)| {
				(|| {
					let (range, name) = line.split_once(';').ok_or(ParseErrorKind::NoSemicolon)?;
					let (range, name) = (range.trim(), name.trim());
					let (start, end) = range.split_once("..").ok_or(ParseErrorKind::NoDotDot)?;
					let start = u32::from_str_radix(start, 16)
						.map_err(|source| ParseErrorKind::ParseInt { source })?;
					let end = u32::from_str_radix(end, 16)
						.map_err(|source| ParseErrorKind::ParseInt { source })?;
					Ok((start..=end, name.to_owned()))
				})()
				.map_err(|kind| ParseError { line: i, kind })
			})
			.collect::<Result<Vec<_>, ParseError>>()?;
		Ok(Self { ranges })
	}
}

#[derive(Debug)]
#[non_exhaustive]
pub struct DownloadError {
	pub kind: DownloadErrorKind,
}

impl Display for DownloadError {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "failed to download Blocks.txt from the Unicode website")
	}
}

impl Error for DownloadError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match &self.kind {
			DownloadErrorKind::Request(e) => Some(e),
			DownloadErrorKind::ReadBody(e) => Some(e),
			DownloadErrorKind::Parse(e) => Some(e),
		}
	}
}

#[derive(Debug)]
pub enum DownloadErrorKind {
	Request(Box<ureq::Error>),
	ReadBody(io::Error),
	Parse(ParseError),
}

#[derive(Debug)]
#[non_exhaustive]
pub struct FromFileError {
	pub path: Box<Path>,
	pub kind: FromFileErrorKind,
}

impl Display for FromFileError {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "error reading `{}`", self.path.display())
	}
}

impl Error for FromFileError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match &self.kind {
			FromFileErrorKind::ReadFile(e) => Some(e),
			FromFileErrorKind::Parse(e) => Some(e),
		}
	}
}

#[derive(Debug)]
pub enum FromFileErrorKind {
	ReadFile(io::Error),
	Parse(ParseError),
}

#[derive(Debug)]
#[non_exhaustive]
pub struct ParseError {
	pub line: usize,
	pub kind: ParseErrorKind,
}

impl Display for ParseError {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "invalid Blocks.txt data on line {}", self.line + 1)
	}
}

impl Error for ParseError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		Some(&self.kind)
	}
}

#[derive(Debug)]
pub enum ParseErrorKind {
	#[non_exhaustive]
	NoSemicolon,
	#[non_exhaustive]
	NoDotDot,
	#[non_exhaustive]
	ParseInt { source: ParseIntError },
}

impl Display for ParseErrorKind {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match *self {
			Self::NoSemicolon => f.write_str("no semicolon"),
			Self::NoDotDot => f.write_str("no `..` in range"),
			Self::ParseInt { .. } => {
				write!(f, "one end of range is not a valid hexadecimal integer")
			}
		}
	}
}

impl Error for ParseErrorKind {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::ParseInt { source } => Some(source),
			_ => None,
		}
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn real_unicode() {
		let data = include_str!("../Blocks.txt").parse::<Blocks>().unwrap();
		assert_eq!(data.block_of('\u{0080}'), "Latin-1 Supplement");
		assert_eq!(data.block_of('½'), "Latin-1 Supplement");
		assert_eq!(data.block_of('\u{00FF}'), "Latin-1 Supplement");
		assert_eq!(data.block_of('\u{EFFFF}'), "No_Block");
	}

	use crate::Blocks;
}

pub const LATEST_URL: &str = "https://www.unicode.org/Public/UCD/latest/ucd/Blocks.txt";

use std::cmp;
use std::error::Error;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fs;
use std::io;
use std::num::ParseIntError;
use std::ops::RangeInclusive;
use std::path::Path;
use std::str::FromStr;
```

</details>

[Blocks.txt]: https://www.unicode.org/Public/UCD/latest/ucd/Blocks.txt
[Unicode blocks]: https://en.wikipedia.org/wiki/Unicode_block
[section 4.2 of Unicode Annex #44]: https://www.unicode.org/reports/tr44/#Format_Conventions
[Unicode Character Database]: https://unicode.org/ucd/
[`FromStr`]: https://doc.rust-lang.org/stable/std/str/trait.FromStr.html
[TryFrom]: https://doc.rust-lang.org/stable/std/convert/trait.TryFrom.html
[error convention]: https://doc.rust-lang.org/stable/std/error/trait.Error.html
[ureq]: https://docs.rs/ureq
[thiserror]: https://docs.rs/thiserror/latest/thiserror/
[anyhow]: https://docs.rs/anyhow
[io::Error MAKES WE WANT TO DIE]: https://github.com/tokio-rs/mio/issues/1444
[C-STABLE]: https://rust-lang.github.io/api-guidelines/necessities.html#public-dependencies-of-a-stable-crate-are-stable-c-stable
[`Error::source()`]: https://doc.rust-lang.org/stable/std/error/trait.Error.html#method.source
