pub(crate) fn asset() -> impl Asset<Output = ()> {
    asset::FsPath::new("./builder/js/package.json")
        .map(|()| log_errors(npm_install()))
        .modifies_path("./builder/js/package-lock.json")
}

fn npm_install() -> anyhow::Result<()> {
    let status = process::Command::new("npm")
        .arg("install")
        .current_dir("./builder/js")
        .status()
        .context("failed to run `npm install`")?;

    ensure!(
        status.success(),
        "`npm install` exited with a non-zero exit status"
    );

    Ok(())
}

pub(crate) fn minify(file_type: FileType, s: &mut String) {
    let res = match file_type {
        FileType::Html => html(s),
        FileType::Css => css(s),
        FileType::Js => js(s),
    };
    match res {
        Ok(minified) => *s = minified,
        Err(e) => log::error!("{e:?}"),
    }
}

#[derive(Clone, Copy)]
pub(crate) enum FileType {
    Html,
    Css,
    Js,
}

fn html(src: &str) -> anyhow::Result<String> {
    pipe(
        process::Command::new("npx")
            .arg("html-minifier-terser")
            .arg("--collapse-boolean-attributes")
            .arg("--collapse-whitespace")
            .arg("--decode-entities")
            .arg("--no-include-auto-generated-tags")
            .arg("--minify-css")
            .arg("--minify-js")
            .arg("--no-newlines-before-tag-close")
            .arg("--remove-attribute-quotes")
            .arg("--remove-comments")
            .arg("--remove-empty-attributes")
            .arg("--remove-redundant-attributes")
            .arg("--remove-tag-whitespace")
            .arg("--sort-attributes")
            .arg("--sort-class-name")
            .current_dir("./builder/js"),
        src,
    )
    .context("failed to minify HTML with html-minifier-terser")
}

fn css(src: &str) -> anyhow::Result<String> {
    pipe(
        process::Command::new("npx")
            .arg("cleancss")
            .arg("-O2")
            .current_dir("./builder/js"),
        src,
    )
    .context("failed to minify CSS with cleancss")
}

fn js(src: &str) -> anyhow::Result<String> {
    pipe(
        process::Command::new("npx")
            .arg("terser")
            .arg("--mangle")
            .arg("toplevel")
            .arg("--mangle-props")
            .arg("--compress")
            .current_dir("./builder/js"),
        src,
    )
    .context("failed to minify JS with terser")
}

fn pipe(command: &mut process::Command, input: &str) -> anyhow::Result<String> {
    let mut child = command
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .spawn()
        .context("failed to spawn child process")?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .context("failed to write to child process' stdin")?;

    let mut output = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .context("failed to read from child process' stdout")?;

    let status = child.wait().context("failed to wait for child process")?;

    ensure!(
        status.success(),
        "child process exited with a non-zero exit status"
    );

    Ok(output)
}

use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use anyhow::ensure;
use anyhow::Context as _;
use std::io::Read as _;
use std::io::Write as _;
use std::process;
