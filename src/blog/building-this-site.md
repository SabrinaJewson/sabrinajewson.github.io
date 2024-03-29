{
	"published": "2022-03-09"
}

# Building this site

Since I've spent the past few days working on creating this website,
I thought I'd make good use of the effort by documenting my experiences here.

I got the idea to create a website
from a desire to have a place to write blog posts.
Initially I had plans on just making GitHub gists and sharing them on Reddit or something,
but I (thankfully) decided against that
since a site allows for much more flexibility.

To avoid having to maintain a web server myself,
I'm just using GitHub Pages to host it
(but on a custom domain to make the URL shorter).
I also decided against using a static site generator
since it's a lot more fun to build it myself.

## The build system { #the-build-system }

I'm not going to write the HTML for this manually,
so I needed to decide on a build system to use.
I briefly considered existing options like Make, [Gulp] or [cargo-make]
but eventually decided to write my own thing in Rust.

The requirements I had for it were this:
- Does the minimum amount of work possible between rebuilds.
- Has a "watch mode" that can be enabled with zero extra configuration
	to watch the directory for changes
	and automatically rebuild when they happen.

After some thinking and a few failed attempts,
I had devised quite a neat solution:
the `Asset` trait.
The core API is this:

```rs
trait Asset {
    type Output;
    fn modified(&self) -> Modified;
    fn generate(&self) -> Self::Output;
}

enum Modified {
    Never,
    At(SystemTime),
}
```

Each `Asset` represents a resource in the generation process:
a text file being read,
a JSON file being parsed,
an HTML file being generated,
an image being tranformed,
et cetera.
It has two main capabilities:
calling `generate`
to do the (potentially expensive) work to actually produce the value,
and calling `modified`
to cheaply compute the time at which that value was last modified.

The `Modified` enum is mostly just a `SystemTime`
but also has a special variant `Never` to represent a time before all `SystemTime`s,
which is used for when getting the modification time fails
(e.g. a deleted/non-existent file)
or when the asset's value is a constant.

The three most basic implementors of this trait
are `Constant`, `Dynamic` and `FsPath`,
representing a constant value,
a dynamic but immutable value (typically command-line arguments)
and a value sourced from a filesystem path's modification time
respectively.
Their implementations are pretty much as you'd expect:

```rs
struct Constant<T>(T);
impl<T: Clone> Asset for Constant<T> {
    type Output = T;
    fn modified(&self) -> Modified { Modified::Never }
    fn generate(&self) -> Self::Output { self.0.clone() }
}

struct Dynamic<T> {
    created: SystemTime,
    value: T,
}
impl<T> Dynamic<T> {
    fn new(value: T) -> Self {
		let created = SystemTime::now();
        Self { created, value }
    }
}
impl<T: Clone> Asset for Dynamic<T> {
    type Output = T;
    fn modified(&self) -> Modified { Modified::At(self.created) }
    fn generate(&self) -> Self::Output { self.value.clone() }
}

struct FsPath<P>(P);
impl<P: AsRef<Path>> Asset for FsPath<P> {
    type Output = ();
    fn modified(&self) -> Modified {
		fs::symlink_metadata(&self.0)
            .and_then(|metadata| metadata.modified())
            .map_or(Modified::Never, Modified::At)
    }
    fn generate(&self) -> Self::Output {}
}
```

`FsPath` is intentionally agnostic over how the actual path is read,
allowing you to many different functions
depending on the actual nature of the path
(whether it's a binary file, text file, JSON file, directory, et cetera).

With these base types there are then many combinators you can apply.
One basic one is `all`, which combines multiple `Asset`s into one
for when a resulting asset is generated from more than one input file
(such as this HTML file, which is generated from the source markdown and a template).
It works on all kinds of containers of multiple assets including tuples and vectors.
Example usage looks like:

```rs
asset::all((foo_asset, bar_asset))
	.map(|(foo_value, bar_value)| /* use both `foo_value` and `bar_value` */)
```

Its `modified` implementation takes the latest modification time of all the inner assets,
and its `generate` implementation just forwards to the generation code of each one
then packages them all up together in a tuple.
However, you might notice a problem here:
with the code above, if `bar` is changed but `foo` isn't
then both `foo` _and_ `bar` are regenerated even if only `bar` actually needs to be.

This is where another combinator comes in: `Cache`.
It provides an in-memory cache of the output's value
(as long as it is `Clone`),
allowing cases like the above to simply use the cached value of `foo`
instead of regenerating it from scratch.

```rs
struct Cache<A: Asset> {
    asset: A,
    cached: Cell<Option<(Modified, A::Output)>>,
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
```

In the code snippet above,
the `generate` function of `Cache` will first attempt to use the cached value
instead of regenerating the asset
if the inner asset hasn't been modified since the cache was taken.

Another place where `Cache` is useful
is when an asset is shared between multiple output assets
(like how my "blog post template" asset is shared with every blog post),
and `Cache` can be applied
to avoid regenerating the shared asset every time.

The last combinator I will talk about here is called `ModifiesPath`,
and it is perhaps the most important one.
You can apply it to an asset
that as a side-effect makes changes to a path on the filesystem,
and it allows that asset to avoid rerunning itself
when the asset's age is older than the path it modifies.

```rs
struct ModifiesPath<A, P> {
    asset: A,
    path: P,
}
impl<A: Asset<Output = ()>, P: AsRef<Path>> Asset for ModifiesPath<A, P> {
    type Output = ();
    fn modified(&self) -> Modified {
		fs::symlink_metadata(&self.path)
            .and_then(|metadata| metadata.modified())
            .map_or(Modified::Never, Modified::At)
    }
    fn generate(&self) -> Self::Output {
        let output_modified = self.modified();
        if self.asset.modified() >= output_modified
			|| *EXE_MODIFIED >= output_modified
		{
            self.asset.generate();
        }
    }
}

static EXE_MODIFIED: Lazy<Modified> = Lazy::new(|| {
    let time = env::current_exe()
		.and_then(fs::symlink_metadata)
		.and_then(|metadata| metadata.modified())
		.unwrap_or_else(|_| SystemTime::now());
	Modified::At(time)
});
```

It is this combinator that allows the Make-like behaviour
of comparing ages of input and output files
and only rebuilding when necessary.

The other thing `ModifiesPath` does
is takes into account the age of the executable it is running in,
forcing a rebuild if the executable itself has been changed
since the output was last generated.
This is very useful during development
to avoid situations where you need to manually remove the destination directory
to force assets to be rebuilt.

The combination of all these features forms a very powerful build system
implemented in simple Rust code.
For example, suppose I wanted to make a build script
that copies over `source.txt` to `destination.txt`.
That would look like this:

```rs
fn main() {
	let asset = source_to_dest();
	asset.generate();
}

fn source_to_dest() -> impl Asset<Output = ()> {
	asset::FsPath::new("source.txt")
		.map(|()| {
			let res = fs::copy("source.txt", "destination.txt");
			if let Err(e) = res {
				log::error!("error copying files: {e}");
			}
		})
		.modifies_path("destination.txt")
}
```

And just like that,
we have automatic tracking of dependencies done for free.
Now suppose I wanted to add a "watch" mode
that waits for changes to `source.txt` to happen and copies it over again.
Absolutely no changes to the `source_to_dest` function are needed,
all we have to do is layer some code using [`notify`] on top of that:

```rs
fn main() {
	let asset = source_to_dest();
	asset.generate();

	let (events_sender, events) = crossbeam_channel::bounded(16);

	let mut watcher = notify::recommended_watcher(events_sender);
	watcher.watch(".".as_ref(), notify::RecursiveMode::Recursive).unwrap();

	loop {
		let _ = events.recv().unwrap().unwrap();
		asset.generate();
	}
}
```

And there we are,
everything is handled automatically from that point onward.
Due to the in-memory caching and on-disk comparison
that assets usually perform
it ends up being pretty efficient,
doing close to the minimum amount of work necessary between rebuilds.
It could theoretically be improved if the _contents_ of the `notify::Event`s
were actually paid attention to
instead of having to repeatedly call `fs::symlink_metadata` a bunch,
but I haven't had a need to implement that just yet.

So there it is,
a powerful and flexible build system implemented and configured
from just Rust code.
I haven't bothered to release it as a crate at all -
if someone asks me to I might
but I don't know if it would be useful to anyone else,
or if something like this already exists in the ecosystem.
But I'm sharing it
because I think it's quite a neat solution to this particular problem.

## The Markdown renderer { #the-markdown-renderer }

The heart of this ad-hoc site generator is really the Markdown renderer.
It's what converts the Markdown files that I write the posts in
into the HTML being rendered right now by your web browser.
So it's fitting for us to start there.

A markdown renderer consists of two main parts:
the first stage that parses the source strings into a more code-friendly format,
and the second stage that generates the HTML
from the abstract Rust representation produced by the parser.

I don't enjoy writing parsers,
so I decided to shell out to an external crate for that.
I chose [`pulldown_cmark`] because it is widely used,
has a flexible API
and supports a bunch of features that I really like
(CommonMark + tables + smart quotes + heading IDs).

While `pulldown_cmark` does come with its own HTML generator
and I could've just used that and called it a day,
there are a bunch of features and additions I would like to implement
that would be far easier if I could control generation myself
rather than trying to modify the HTML AST after-the-fact.

So, taking inspiration from `pulldown_cmark`'s HTML renderer,
I resolved to write my own.
It works by walking once through all the events emitted
by `pulldown_cmark`'s `Parser` struct
and keeping track of state along the way
in a gigantic `Renderer` type.
Once the tree walk is finished,
it runs a bit of finalization
before dumping its relevant fields in the resulting `Markdown` struct:

```rs
struct Markdown {
    title: String,
    body: String,
    summary: String,
    outline: String,
}
```

Looking at these fields,
you can probably tell why I didn't just use the default HTML generator -
there's a lot of custom functionality in there
not provided by plain `pulldown_cmark`.
`title` contains the title of the page,
`body` contains the body HTML (but excluding the title),
`summary` contains the un-HTML-ified first paragraph of the content
(this is used to put in each page's `[html] <meta name="description">` tags)
and `outline` is the automatically generated table of contents
you can see at the top of this page.

Another reason I wanted to write my own HTML renderer
is to enable syntax highlighting -
by default `pulldown_cmark`
puts all code into plain `[html] <pre>` and `[html] <code>` elements,
but I wanted to transform it with build-time syntax highlighting instead
to enable the pretty colours you can see in the code I write.

I chose the [`syntect`] crate to do the highlighting,
since it's widely used and has the features I need.
It turned out to be pretty simple to add this functionality;
I just embed the syntax definitions in the source code
and load it in a lazy static,
then use a [`ClassedHTMLGenerator`] to produce the actual HTML.
The themes can be loaded separately by loading them at runtime in an `Asset`,
converting them to CSS then concatenating with the CSS file for blog posts.

And that's pretty much all there is to it:
a single pure function that goes from markdown source to rendered HTML,
to be later inserted into whichever document needs it.
Actually, speaking of inserting it into documents, how _does_ that work?

## Templating { #templating }

Unfortunately it's not enough to just take rendered HTML,
stick it in a `.html` file and call it a day.
I need to add an HTML skeleton around it
to add the
document title,
[favicon](#adding-the-favicon),
metadata
and sitewide navigation links
you can see on this page.

Initially, I had written [my own custom templater] for this.
It was barely even a templater really, being so ridiculously minimal:
just ~100 lines of code that replaced `\{variable_name}` with its contents.
But as the project continued to grow
I realized that I needed a better solution than that,
so I decided to switch to a full-fledged templating library.

I chose [`handlebars`] for this,
not for any particular reason,
but I wanted to try it out since I've only used Tera before.
I used my `Asset` system to create an asset
that loads all the common "fragment" templates from an `include/` directory
as well as individual `Asset`s for each template per page,
then I combined them together
and rendered it all to produce the final pages.

The template system turned out to be pretty powerful
and definitely worth the extra dependency.
I'm able to automatically generate pretty much everything automatically,
like my [list of blog posts](.)
whose content is sourced from the Markdown files only.
Additionally, all the HTML boilerplate used repeatedly in every page
can be abstracted to [a common file][base.hbs]
which turned out to be very useful for code reuse.

## Minification { #minification }

To reduce page load times,
I decided to minify all my HTML and CSS
before writing each asset to its final file.
I know that there exist minifiers for this in native Rust,
but realistically all the state-of-the-art ones are in JavaScript.
I ended up choosing [html-minifier-terser] and [clean-css] for this,
which both seem to be well-maintained and have small output sizes.

Initially I planned on achieving maximum efficiency
by using both projects as a library
and starting up a single long-running Node process
that I communicate with via IPC,
to avoid the inefficiencies of starting up a whole new Node instance
each time I wanted to minify something.
But that plan ended up falling apart rather quickly,
since I totally lack experience with Node
and just couldn't figure out how to get it to work.
Maybe it's just me but Node's [readable stream] interface
seems a million times more complicated and hard to use than Rust's `AsyncRead` -
it has four (!) separate ways of using the API
of which none are as simple as just `read_exact`.
And it doesn't help that I despise writing JavaScript altogether -
TypeScript makes it somewhat better
but in comparison to Rust it's just painful.

So with that plan scrapped,
it was just a matter of calling into their CLIs each time
(which luckily both libraries have).
To avoid global dependencies I created a local npm package
that uses both packages as a dependency.
Then I could simply have a `std::process::Command` run
`[sh] npx html-minifier-terser` or `[sh] npx cleancss` in that package's directory
and pipe through my files to have them minified.

The one issue I encountered is that unlike Cargo,
npm doesn't automatically install required dependencies
before trying to run code.
This means that
in order to successfully build my website from a freshly cloned repository,
you would've had to manually `[sh] cd` to the package directory
and run `[sh] npm install` beforehand -
obviously not ideal.

My first solution to this
was just to run that command first thing
whenever the building binary starts up.
But since `[sh] npm install` is slow,
it ends up slowing down the whole building process quite a significant amount
since I have to wait for it each time.
What I really needed was a way to only run the command
when it hasn't been run yet, or when the `package.json` changes.
Lucky, my whole `Asset` system is just perfect for that -
I could simply define an asset that runs `[sh] npm install`
with `package.json` specified as its input file
and `package-lock.json` as the output one
(since `[sh] npm install` always updates its modification date).
It ended up just being a couple lines of code:

```rs
fn asset() -> impl Asset<Output = ()> {
    asset::FsPath::new("./builder/js/package.json")
        .map(|()| log_errors(npm_install()))
        .modifies_path("./builder/js/package-lock.json")
}
```

And now I have the best of both worlds:
fast building as well as automatic package setup.

## Adding a dark theme { #adding-a-dark-theme }

One specific goal I had for this site
was to allow it to work in both light and dark modes,
depending on the user's chosen `prefers-color-scheme` setting.
I was mildly dreading having to write out two large stylesheets
with a different colour palette
for each mode, but as it turns out
modern browsers have a built-in way to change the default color scheme
based on the user's current `prefers-color-scheme` value.
All I had to do was add one `[html] <meta>` to my `[html] <head>`:

```html
<meta name="color-scheme" content="dark light">
```

And everything magically worked first try -
if `prefers-color-scheme` was `dark`,
the page would show a black background with consistently white text
and if it was `light`
it would show a white background with consistently black text.
You can try it out now -
if you open developer tools and press ctrl+shift+p,
you should be able to enable the "emulate CSS prefers-color-scheme: dark/light" option
and see how the website changes.
And all that's done entirely by the browser's default styles.
Who knew it was so easy?

The only time I did have to mess with `prefers-color-scheme` media queries
was for the code blocks.
That was easy though,
I just wrote out the dark theme CSS
then wrapped the light version in `[css] @media (prefers-color-scheme: light) {`.

## Adding the favicon { #adding-the-favicon }

The favicon of this site is automatically generated by the build script
from a single `.png` file in source control.
I use the [`image`] crate to read in this source image,
then resize it to generate two files:
- `favicon.ico`,
	which contains 16x16, 32x32 and 64x64 versions of the icon
	all packed into a single `.ico` file.
- `apple-touch-icon.png`,
	which has been resized to 180x180 as is suitable for an apple touch icon.

The paths of these files are then passed in to the templates,
which include them in `[html] <link>` tags in the head:

```html
<link rel="icon" href="/{{icons.favicon}}">
<link rel="apple-touch-icon" href="/{{icons.apple_touch_icon}}">
```

I'm especially proud of this part of the code
because the entire thing is implemented in <100 lines of logic
and is far, far more convenient
than manually using a site like [RealFaviconGenerator]
to generate each of the files.
The only downside of it is that [`image`] is ridiculously slow in debug mode,
so I end up running the build process in `--release` all the time 😄.

## A live-reloading dev server { #a-live-reloading-dev-server }

For a long time I was previewing the website
by just opening the file in the browser as a `file://` URL.
But this had several disadvantages:
1. Paths like `/favicon.ico` would be resolved relative to the filesystem root,
	rather than the website root.
1. `index.html` wasn't automatically added to the end of paths
	if they pointed to directories and that file existed.
	I'd instead see a screen showing a file listing of the directory
	and have to click `index.html` manually each time.
1. `.html` wasn't automatically added to the end of paths like `/blog/foo`,
	making my links broken.
1. 404 links did not show my custom `404.html` page.
1. I didn't get live reloading.

At some point I switched to `[sh] python -m http.server`
and that solved issues (1) and (2)
but not the others.
So eventually I'd had enough
and decided to write my own server with all these features, in Rust.

Since the server doesn't need to be particularly complex,
I decided to just use plain [hyper] -
no higher-level framework or anything.
And it doesn't need performance,
so I'm only using Tokio's current thread runtime
instead of the heavier multi-threaded scheduler.

The server's main job is to take a request path
and map it to a path on the filesystem,
which it does just by splitting on `/` and reconstructing a `PathBuf`.
I also have some extra logic to solve problems (2) and (3) -
adding `index.html` and `.html` to paths as a fallback
if the requested path doesn't exist.
I guess the MIME type to serve based on file extension,
which works fine for me,
and also set `Cache-Control: no-cache`
to avoid having the browser cache the pages.

To achieve live reloading, two things need to be coordinated.
First,
the server has to expose an endpoint that allows the browser
to wait for a change to happen to any of the files it's viewing -
I do this via a `/watch` endpoint
that accepts a list of paths to watch in its query parameters
(decoded with [`form_urlencoded`])
and gives back an SSE stream that sends an empty event once something happens.
Internally
this is implemented with a Tokio `broadcast` channel of `notify::Event`s,
and a spawned task that subscribes to the channel
and checks whether any of the events apply to it,
sending an SSE event if so.
Secondly,
the client needs to produce a list of all the files it depends on
and then send that in the SSE request to the server,
reloading once it receives any data over that connection.

I do all that by passing in a boolean property `live_reload` to the templates,
and only enable it when the server is running
(this is easy since the server and build process share the same binary).
The page will build up a set of dependencies in a `URLSearchParams` object
then send off the request like so:

```html
{{#if live_reload}}
<script>
	const source = new EventSource(`/watch?${params}`);
	source.addEventListener("message", () => location.reload());
</script>
{{/if}}
```

And just like that, we have live reloading.
Whenever I edit one of the source files like the one I'm writing,
a whole chain of automated events is set off,
culminating in the reload of the page I'm viewing in-browser:

1. The `notify` watcher sees the event and regenerates the main asset.
1. The main asset generates the "blog posts" asset.
1. The "blog posts" asset generates the asset for this blog post.
1. This asset compares the dates of its input and output files,
	and upon seeing that the input file is newer than the output file
	decides to regenerate itself.
1. The updated blog post HTML is written out to the `dist/` directory.
1. The `notify` watcher sees the event
	and passes it over to the server's broadcast channel.
1. The task spawned to manage the connnection to the site
	receives the event from the channel,
	and upon checking what paths it affects
	decides that the web page should reload.
1. The task sends an SSE event to the website which it then receives.
1. The website reloads,
	sending a new request to the server
	and receiving the updated blog post HTML.

## Conclusion { #conclusion }

Overall, I am extremely pleased with how this whole project has turned out.
I now have my own personal website,
designed in exactly the way I like it
able to support exactly the workflow that I like,
with almost everything completely automated with the power of code.

Do I recommend it if you want to start your own website?
Not really, unless you'd do this sort of programming project anyway.
All in all it took about a week to set up,
and I was working on it for several hours each day.
I can probably imagine that using an existing static site generator
is a thousand times easier and faster
and produces just as good output.
But it was an extremely fun project for me do
so I can definitely recommend it in that sense.

If you want to check out the actual code it's [on GitHub][src]
and contains all the things I talked about here,
as well as some more mundane stuff I left out the article for brevity.
Its file structure is located into three main folders:
- `builder`,
	which contains the Rust source code of the crate that builds the site
	and runs the server.
- `template`,
	which contains
	Handlebars templates,
	CSS,
	code themes
	and various other dynamic configuration parts related to the site.
- `src`,
	which contains the source Markdown of the posts I've written
	as well as the favicon of the website.

Anyway, I really hope you enjoyed reading this post
and maybe learnt something you found interesting.
See you next time!

[Gulp]: https://gulpjs.com/
[cargo-make]: https://sagiegurari.github.io/cargo-make/
[`notify`]: https://github.com/notify-rs/notify
[`pulldown_cmark`]: https://docs.rs/pulldown_cmark
[`syntect`]: https://github.com/trishume/syntect
[`ClassedHTMLGenerator`]: https://docs.rs/syntect/4/syntect/html/struct.ClassedHTMLGenerator.html
[my own custom templater]: https://github.com/SabrinaJewson/sabrinajewson.github.io/blob/2df2918523de279720a5854b658864c92fe0671a/builder/src/util/template.rs
[`handlebars`]: https://docs.rs/handlebars
[base.hbs]: https://github.com/SabrinaJewson/sabrinajewson.github.io/blob/fa55487a0d9bd3ee46d68fa51ea32d9a2eaa2772/template/include/base.hbs
[html-minifier-terser]: https://github.com/terser/html-minifier-terser
[clean-css]: https://clean-css.github.io/
[readable stream]: https://nodejs.org/api/stream.html#readable-streams
[`image`]: https://docs.rs/image
[RealFaviconGenerator]: https://realfavicongenerator.net/
[hyper]: https://docs.rs/hyper
[`form_urlencoded`]: https://docs.rs/form_urlencoded
[src]: https://github.com/SabrinaJewson/sabrinajewson.github.io
