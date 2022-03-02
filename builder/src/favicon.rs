use crate::asset::{self, Asset};
use ::{
    anyhow::Context as _,
    image::codecs::ico::{IcoEncoder, IcoFrame},
    std::{
        fs::File,
        io::{BufWriter, Write as _},
        path::Path,
    },
};

pub(crate) const FAVICON_PATH: &str = "favicon.ico";
pub(crate) const APPLE_TOUCH_ICON_PATH: &str = "apple-touch-icon.png";

pub(crate) fn asset<'a>(
    input_path: &'a Path,
    output_path: &'a Path,
) -> impl Asset<Output = ()> + 'a {
    asset::FsPath::new(input_path)
        .map(move |()| -> anyhow::Result<()> {
            let image = image::open(input_path)
                .with_context(|| format!("failed to open {}", input_path.display()))?;

            let filter = image::imageops::FilterType::CatmullRom;

            image
                .resize(APPLE_TOUCH_ICON_SIZE, APPLE_TOUCH_ICON_SIZE, filter)
                .save(output_path.join(APPLE_TOUCH_ICON_PATH))
                .with_context(|| format!("couldn't save to {APPLE_TOUCH_ICON_PATH}"))?;

            let favicon_path = output_path.join(FAVICON_PATH);
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
        .modifies_path(output_path.join(APPLE_TOUCH_ICON_PATH))
        .modifies_path(output_path.join(FAVICON_PATH))
        .map(|res| {
            if let Err(e) = res {
                log::error!("{e:?}");
            }
        })
}

// The sizes included in the generated `favicon.ico` file.
// I just copied what RealFaviconGenerator does.
const ICO_SIZES: [u32; 3] = [16, 32, 48];

const APPLE_TOUCH_ICON_SIZE: u32 = 180;
