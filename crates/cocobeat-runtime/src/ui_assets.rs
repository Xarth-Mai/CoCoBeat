use crate::i18n::Locale;
use bevy::{
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
};
use std::collections::HashMap;

const FONT_BYTES: [&[u8]; 6] = [
    include_bytes!("../../../assets/fonts/NotoSans[wdth,wght].ttf"),
    include_bytes!("../../../assets/fonts/NotoSansSC-Regular.otf"),
    include_bytes!("../../../assets/fonts/NotoSansTC-Regular.otf"),
    include_bytes!("../../../assets/fonts/NotoSansHK-Regular.otf"),
    include_bytes!("../../../assets/fonts/NotoSansJP-Regular.otf"),
    include_bytes!("../../../assets/fonts/NotoSansKR-Regular.otf"),
];

const FLAG_BYTES: [(&str, &[u8]); 13] = [
    ("cn", include_bytes!("../../../assets/flags/cn.png")),
    ("us", include_bytes!("../../../assets/flags/us.png")),
    ("gb", include_bytes!("../../../assets/flags/gb.png")),
    ("jp", include_bytes!("../../../assets/flags/jp.png")),
    ("kr", include_bytes!("../../../assets/flags/kr.png")),
    ("tw", include_bytes!("../../../assets/flags/tw.png")),
    ("hk", include_bytes!("../../../assets/flags/hk.png")),
    ("mx", include_bytes!("../../../assets/flags/mx.png")),
    ("br", include_bytes!("../../../assets/flags/br.png")),
    ("fr", include_bytes!("../../../assets/flags/fr.png")),
    ("de", include_bytes!("../../../assets/flags/de.png")),
    ("ru", include_bytes!("../../../assets/flags/ru.png")),
    ("ua", include_bytes!("../../../assets/flags/ua.png")),
];

#[derive(Resource)]
pub(crate) struct UiAssets {
    fonts: [Handle<Font>; 6],
    flags: HashMap<&'static str, Handle<Image>>,
}

impl UiAssets {
    pub fn font(&self, locale: Locale) -> Handle<Font> {
        self.fonts[locale.font_index()].clone()
    }

    pub fn flag(&self, locale: Locale) -> Handle<Image> {
        self.flags[locale.flag_code()].clone()
    }
}

pub fn install(app: &mut App) -> Result<(), String> {
    let fonts = {
        let mut assets = app.world_mut().resource_mut::<Assets<Font>>();
        FONT_BYTES.map(|bytes| assets.add(Font::from_bytes(bytes.to_vec())))
    };
    let flags = {
        let mut assets = app.world_mut().resource_mut::<Assets<Image>>();
        FLAG_BYTES
            .into_iter()
            .map(|(code, bytes)| {
                let image = Image::from_buffer(
                    bytes,
                    ImageType::Extension("png"),
                    CompressedImageFormats::NONE,
                    true,
                    ImageSampler::linear(),
                    default(),
                )
                .map_err(|error| format!("Flag {code}: {error}"))?;
                Ok((code, assets.add(image)))
            })
            .collect::<Result<HashMap<_, _>, String>>()?
    };
    app.insert_resource(UiAssets { fonts, flags });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::text::FontCx;
    use std::collections::HashSet;

    #[test]
    fn bundled_ui_assets_decode_and_keep_locale_faces_distinct() {
        let mut app = App::new();
        app.init_resource::<Assets<Font>>()
            .init_resource::<Assets<Image>>();
        install(&mut app).unwrap();
        let ui = app.world().resource::<UiAssets>();
        let fonts = app.world().resource::<Assets<Font>>();
        let images = app.world().resource::<Assets<Image>>();
        assert_eq!(fonts.len(), 6);
        assert_eq!(images.len(), 13);

        let mut font_cx = FontCx::default();
        for (handle, family) in ui.fonts.iter().zip([
            "Noto Sans",
            "Noto Sans SC",
            "Noto Sans TC",
            "Noto Sans HK",
            "Noto Sans JP",
            "Noto Sans KR",
        ]) {
            let font = fonts.get(handle).unwrap();
            let registered = font_cx.collection.register_fonts(font.data.clone(), None);
            assert_eq!(registered.len(), 1, "{family}");
            assert_eq!(
                font_cx.collection.family_name(registered[0].0),
                Some(family)
            );
        }

        let mut font_ids = HashSet::new();
        let mut flag_ids = HashSet::new();
        for locale in Locale::ALL {
            let font = ui.font(locale);
            font_ids.insert(font.id());
            assert_eq!(
                fonts.get(&font).unwrap().data.as_ref(),
                FONT_BYTES[locale.font_index()]
            );
            let flag = ui.flag(locale);
            assert!(flag_ids.insert(flag.id()));
            assert_eq!(images.get(&flag).unwrap().size(), UVec2::new(96, 72));
        }
        assert_eq!(font_ids.len(), 6);
        assert_eq!(flag_ids.len(), 13);
    }
}
