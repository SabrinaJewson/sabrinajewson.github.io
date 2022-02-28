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
)]

use ::{
    anyhow::Context as _,
    fn_error_context::context,
    notify::Watcher,
    std::{env, fs, thread},
};

mod asset;
use asset::Asset;

mod blog;
mod markdown;
mod push_str;

/// Rust program that builds this website.
#[derive(clap::Parser)]
struct Args {
    /// Whether to watch the directory for changes.
    #[clap(long)]
    watch: bool,
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let args: Args = clap::Parser::parse();

    set_cwd()?;

    fs::create_dir_all("dist").context("couldn't create `dist` directory")?;

    let blog = blog::asset("./blog", "./dist/blog");
    blog.generate()?;

    if args.watch {
        // TODO: a better synchronization mechanism than thread parking?
        let main_thread = thread::current();

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
                main_thread.unpark();
            }
        })
        .context("failed to create file watcher")?;

        watcher
            .watch(".".as_ref(), notify::RecursiveMode::Recursive)
            .context("failed to watch directory")?;

        loop {
            thread::park();
            log::info!("rebuilding");
            if let Err(e) = blog.generate() {
                log::error!("{:?}", e);
            }
        }
    }

    Ok(())
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
