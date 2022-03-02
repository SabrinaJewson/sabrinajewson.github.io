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

use ::{
    anyhow::Context as _,
    crossbeam::channel,
    fn_error_context::context,
    notify::Watcher,
    std::{
        env,
        time::{Duration, Instant},
    },
};

mod blog;
mod common_css;
mod icons;
mod templater;

mod util;
use self::util::{
    asset::{self, Asset},
    minify,
};

/// Rust program that builds this website.
#[derive(clap::Parser)]
struct Args {
    /// Whether to build drafts.
    #[clap(long)]
    drafts: bool,

    /// Whether to watch the directory for changes.
    #[clap(long)]
    watch: bool,
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let args: Args = clap::Parser::parse();

    set_cwd()?;

    let asset = asset(args.drafts);
    asset.generate();

    if args.watch {
        let (sender, receiver) = channel::bounded(1);

        let mut watcher = notify::recommended_watcher(move |event_res| {
            // TODO: more fine grained tracking of `notify::Event`s?
            let event: notify::Event = match event_res {
                Ok(event) => event,
                Err(e) => {
                    log::error!("error watching: {}", e);
                    return;
                }
            };
            if !matches!(event.kind, notify::event::EventKind::Access(_)) {
                let _ = sender.try_send(());
            }
        })
        .context("failed to create file watcher")?;

        watcher
            .watch(".".as_ref(), notify::RecursiveMode::Recursive)
            .context("failed to watch directory")?;

        log::info!("now watching for changes");

        loop {
            let _ = receiver.recv();
            // debounce
            let debounce_deadline = Instant::now() + Duration::from_millis(10);
            while receiver.recv_deadline(debounce_deadline).is_ok() {}

            log::info!("rebuilding");
            asset.generate();
        }
    }

    Ok(())
}

fn asset(drafts: bool) -> impl Asset<Output = ()> {
    asset::all((
        // This must come first to initialize minification
        minify::asset(),
        blog::asset("blog".as_ref(), "dist/blog".as_ref(), drafts),
        icons::asset("icon.png".as_ref(), "dist".as_ref()),
        common_css::asset("common.css".as_ref(), "dist".as_ref()),
    ))
    .map(|((), (), (), ())| {})
}

#[context("failed to set cwd to project root")]
fn set_cwd() -> anyhow::Result<()> {
    let path = env::current_exe().context("couldn't get current executable path")?;
    let cwd = (|| {
        let profile_dir = path.parent()?;
        let target_dir = profile_dir.parent()?;
        let package_dir = target_dir.parent()?;
        let root_dir = package_dir.parent()?;
        Some(root_dir)
    })()
    .context("project root dir doesn't exist")?;

    env::set_current_dir(cwd).context("couldn't set cwd")?;
    Ok(())
}
