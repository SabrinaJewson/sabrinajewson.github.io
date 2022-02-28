use ::{
    anyhow::{ensure, Context as _},
    fn_error_context::context,
    std::{
        io::{Read as _, Write as _},
        process,
    },
};

pub(crate) fn init() -> anyhow::Result<()> {
    let status = process::Command::new("npm")
        .arg("install")
        .arg("--silent")
        .current_dir("./builder/js")
        // disable the progress bar
        .stderr(process::Stdio::null())
        .status()
        .context("failed to run `npm install`")?;

    ensure!(
        status.success(),
        "`npm install` exited with a non-zero exit status"
    );

    Ok(())
}

#[context("failed to minify HTML")]
pub(crate) fn html(html: &mut String) -> anyhow::Result<()> {
    let mut child = process::Command::new("npx")
        .arg("html-minifier-terser")
        .arg("--collapse-boolean-attributes")
        .arg("--collapse-inline-tag-whitespace")
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
        .current_dir("./builder/js")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .spawn()
        .context("failed to run `npx html-minifier-terser`")?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(html.as_bytes())
        .context("failed to write to html-minifier-terser stdin")?;

    html.clear();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(html)
        .context("failed to read from html-minifier-terser stdout")?;

    let status = child
        .wait()
        .context("failed to wait for html-minifier-terser")?;

    ensure!(
        status.success(),
        "html-minifier-terser exited with a non-zero exit status"
    );

    Ok(())
}
