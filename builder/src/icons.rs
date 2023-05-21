// Used in templates
#[derive(Clone, Copy, Serialize)]
pub(crate) struct Paths {
    pub(crate) favicon: &'static str,
    pub(crate) apple_touch_icon: &'static str,
}

pub(crate) const PATHS: Paths = Paths {
    favicon: "favicon.ico",
    apple_touch_icon: "apple-touch-icon.png",
};

pub(crate) fn asset<'a>(
    input_path: &'a Path,
    output_path: &'a Path,
    config: impl Asset<Output = &'a Config> + 'a,
) -> impl Asset<Output = ()> + 'a {
    config
        .map(|config| -> Box<dyn Asset<Output = ()> + 'a> {
            if config.icons {
                Box::new(real_asset(input_path, output_path))
            } else {
                Box::new(asset::Constant::new(()))
            }
        })
        .flatten()
}

fn real_asset<'a>(input_path: &'a Path, output_path: &'a Path) -> impl Asset<Output = ()> + 'a {
    asset::FsPath::new(input_path)
        .map(move |()| -> anyhow::Result<()> {
            let image = image::open(input_path)
                .with_context(|| format!("failed to open {}", input_path.display()))?;

            let filter = image::imageops::FilterType::CatmullRom;

            image
                .resize(APPLE_TOUCH_ICON_SIZE, APPLE_TOUCH_ICON_SIZE, filter)
                .save(output_path.join(PATHS.apple_touch_icon))
                .with_context(|| format!("couldn't save to {}", PATHS.apple_touch_icon))?;

            let favicon_path = output_path.join(PATHS.favicon);
            let mut file = BufWriter::new(
                File::create(&favicon_path)
                    .with_context(|| format!("failed to create {}", favicon_path.display()))?,
            );

            IcoEncoder::new(&mut file)
                .encode_images(
                    &ICO_SIZES
                        .into_iter()
                        .map(|size| {
                            let resized = image.resize(size, size, filter);
                            IcoFrame::as_png(
                                resized.as_bytes(),
                                resized.width(),
                                resized.height(),
                                resized.color(),
                            )
                            .context("failed to encode icon as PNG")
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                )
                .context("failed to write to favicon.ico")?;

            file.flush().context("failed to flush favicon.ico")?;

            log::info!("successfully emitted favicon files");

            Ok(())
        })
        .map(log_errors)
        .modifies_path(output_path.join(PATHS.apple_touch_icon))
        .modifies_path(output_path.join(PATHS.favicon))
}

// The sizes included in the generated `favicon.ico` file.
// I just copied what RealFaviconGenerator does.
const ICO_SIZES: [u32; 3] = [16, 32, 48];

const APPLE_TOUCH_ICON_SIZE: u32 = 180;

use crate::util::asset;
use crate::util::asset::Asset;
use crate::util::log_errors;
use crate::Config;
use anyhow::Context as _;
use image::codecs::ico::IcoEncoder;
use image::codecs::ico::IcoFrame;
use serde::Serialize;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write as _;
use std::path::Path;
