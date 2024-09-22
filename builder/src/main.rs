#![warn(
    noop_method_call,
    trivial_casts,
    trivial_numeric_casts,
    unused_import_braces,
    unused_lifetimes,
    unused_qualifications,
    clippy::pedantic
)]
#![allow(
    clippy::match_bool,
    clippy::single_component_path_imports, // https://github.com/rust-lang/rust-clippy/issues/7923
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::struct_excessive_bools,
)]

mod blog;
mod common_css;
mod icons;
mod index;
mod not_found;
mod raw;
mod reviews;
#[cfg(feature = "server")]
mod server;
mod templater;

mod config;
use config::Config;

mod util;
use self::util::asset;
use self::util::asset::Asset;
use self::util::minify;

/// Rust program that builds this website.
#[derive(clap::Parser)]
struct Args {
    /// Whether to build drafts.
    #[clap(long)]
    drafts: bool,

    /// Whether to disable icon building.
    #[clap(long)]
    no_icons: bool,

    /// Whether to minify the output.
    #[clap(long)]
    minify: bool,

    /// Whether to watch the directory for changes.
    #[clap(long)]
    watch: bool,

    /// Output directory.
    #[clap(short, default_value = "dist")]
    output: String,

    /// Serve a development server on the given port.
    /// Implies `--watch`.
    #[clap(long, conflicts_with = "watch")]
    serve_port: Option<u16>,
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let args: Args = clap::Parser::parse();

    set_cwd()?;

    ensure!(
        args.serve_port.is_none() || cfg!(feature = "server"),
        "server is not enabled; rebuild with `--features server` and try again"
    );

    let config = Config {
        drafts: args.drafts,
        minify: args.minify,
        icons: !args.no_icons,
        live_reload: args.serve_port.is_some(),
    };

    let bump = Bump::new();
    let asset = asset(&bump, &args.output, asset::Dynamic::new(&config));
    asset.generate();

    if args.watch || args.serve_port.is_some() {
        let (sender, receiver) = channel::bounded::<anyhow::Result<()>>(1);

        #[cfg(feature = "server")]
        let server = if let Some(port) = args.serve_port {
            let server = server::Server::new(Path::new(&args.output));
            std::thread::spawn({
                let sender = sender.clone();
                let server = server.clone();
                move || sender.send(server.listen(port).map(|infallible| match infallible {}))
            });
            Some(server)
        } else {
            None
        };

        let mut watcher = notify::recommended_watcher(move |event_res| {
            // TODO: more fine grained tracking of `notify::Event`s?
            let event: notify::Event = match event_res {
                Ok(event) => event,
                Err(e) => {
                    log::error!("error watching: {}", e);
                    return;
                }
            };
            if matches!(event.kind, notify::event::EventKind::Access(_)) {
                return;
            }

            drop(sender.try_send(Ok(())));

            #[cfg(feature = "server")]
            if let Some(server) = &server {
                server.update(event);
            }
        })
        .context("failed to create file watcher")?;

        watcher
            .watch(".".as_ref(), notify::RecursiveMode::Recursive)
            .context("failed to watch directory")?;

        log::info!("now watching for changes");

        loop {
            receiver.recv().expect("senders are never dropped")?;
            // debounce
            let debounce_deadline = Instant::now() + Duration::from_millis(10);
            while let Ok(msg) = receiver.recv_deadline(debounce_deadline) {
                msg?;
            }
            log::debug!("rebuilding");
            asset.generate();
        }
    }

    Ok(())
}

fn asset<'asset>(
    bump: &'asset Bump,
    output: &'asset str,
    config: impl Asset<Output = &'asset Config> + Copy + 'asset,
) -> impl Asset<Output = ()> + 'asset {
    let templater = Rc::new(templater::asset("template/include".as_ref(), config));

    asset::all((
        // This must come first to initialize minification
        config
            .map(|config| -> Box<dyn Asset<Output = ()>> {
                if config.minify {
                    Box::new(minify::asset())
                } else {
                    Box::new(asset::Constant::new(()))
                }
            })
            .flatten(),
        blog::asset(
            "template/blog".as_ref(),
            "src/blog".as_ref(),
            Path::new(util::bump::alloc_str_concat(bump, &[output, "/blog"])),
            templater.clone(),
            config,
        ),
        //reviews::asset(
        //    "src/reviews.toml".as_ref(),
        //    "template/reviews.hbs".as_ref(),
        //    "template/reviews.css".as_ref(),
        //    "template/reviews.js".as_ref(),
        //    Path::new(output),
        //    templater.clone(),
        //    config,
        //),
        index::asset(
            "template/index.hbs".as_ref(),
            "src/index.md".as_ref(),
            Path::new(util::bump::alloc_str_concat(bump, &[output, "/index.html"])),
            templater.clone(),
        ),
        not_found::asset(
            "template/404.hbs".as_ref(),
            Path::new(util::bump::alloc_str_concat(bump, &[output, "/404.html"])),
            templater,
        ),
        common_css::asset("template/common.css".as_ref(), Path::new(output), config),
        icons::asset("src/icon.png".as_ref(), Path::new(output), config),
        raw::asset("raw".as_ref(), Path::new(output)),
    ))
    .map(|((), (), (), (), (), (), ())| {})
}

#[context("failed to set cwd to project root")]
fn set_cwd() -> anyhow::Result<()> {
    let mut path = env::current_exe().context("couldn't get current executable path")?;
    for _ in 0..4 {
        ensure!(path.pop(), "project root dir doesn't exit");
    }
    env::set_current_dir(&path).context("couldn't set cwd")?;
    Ok(())
}

use anyhow::ensure;
use anyhow::Context as _;
use bumpalo::Bump;
use crossbeam::channel;
use fn_error_context::context;
use notify::Watcher;
use std::env;
use std::path::Path;
use std::rc::Rc;
use std::str;
use std::time::Duration;
use std::time::Instant;
