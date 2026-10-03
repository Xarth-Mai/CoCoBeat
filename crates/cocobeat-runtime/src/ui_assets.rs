use crate::i18n::Locale;
use bevy::{
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    text::{FontCx, detect_text_needs_rerender, load_font_assets_into_font_collection},
};
use fontique::{FamilyId, Script};
use std::collections::HashMap;

const FONT_FAMILIES: [&str; 6] = [
    "Noto Sans",
    "Noto Sans SC",
    "Noto Sans TC",
    "Noto Sans HK",
    "Noto Sans JP",
    "Noto Sans KR",
];

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
    app.insert_resource(UiAssets { fonts, flags }).add_systems(
        PostUpdate,
        configure_fallbacks
            .after(load_font_assets_into_font_collection)
            .before(detect_text_needs_rerender),
    );
    Ok(())
}

fn configure_fallbacks(
    mut font_cx: ResMut<FontCx>,
    mut configured: Local<Option<[FamilyId; 6]>>,
    mut text_fonts: Query<&mut TextFont>,
) {
    let [
        Some(latin),
        Some(sc),
        Some(tc),
        Some(hk),
        Some(jp),
        Some(kr),
    ] = FONT_FAMILIES.map(|name| font_cx.collection.family_id(name))
    else {
        return;
    };
    let families = [latin, sc, tc, hk, jp, kr];
    if *configured == Some(families) {
        return;
    }

    // Keep each TextFont's regional face first; Parley resolves missing scripts
    // New family IDs also detect Bevy rebuilding the collection after font removal
    for (script, fallbacks) in [
        (*b"Latn", &[latin][..]),
        (*b"Cyrl", &[latin][..]),
        (*b"Grek", &[latin][..]),
        (*b"Hani", &[sc, tc, hk, jp, kr][..]),
        (*b"Hira", &[jp][..]),
        (*b"Kana", &[jp][..]),
        (*b"Hang", &[kr][..]),
    ] {
        font_cx
            .collection
            .set_fallbacks(Script::from_bytes(script), fallbacks.iter().copied());
    }
    *configured = Some(families);
    // set_fallbacks clears Fontique's lookup cache; existing Bevy layouts also need reshaping
    for mut font in &mut text_fonts {
        font.set_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::text::{ComputedTextBlock, LayoutCx, TextBounds, TextPipeline};
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
        for (handle, family) in ui.fonts.iter().zip(FONT_FAMILIES) {
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

    fn shape(
        app: &mut App,
        layout: &mut LayoutCx,
        locale: Locale,
        text: &str,
    ) -> ComputedTextBlock {
        let font =
            TextFont::from(app.world().resource::<UiAssets>().font(locale)).with_font_size(18.0);
        let mut computed = ComputedTextBlock::default();
        app.world_mut()
            .resource_scope(|world, mut font_cx: Mut<FontCx>| {
                TextPipeline::default()
                    .update_buffer(
                        world.resource::<Assets<Font>>(),
                        std::iter::once((
                            Entity::PLACEHOLDER,
                            0,
                            text,
                            &font,
                            Color::WHITE,
                            default(),
                            default(),
                        )),
                        LineBreak::NoWrap,
                        Justify::Left,
                        TextBounds::UNBOUNDED,
                        1.0,
                        &mut computed,
                        &mut font_cx,
                        layout,
                        Vec2::new(1280.0, 720.0),
                        16.0,
                    )
                    .unwrap();
            });
        computed
    }

    #[test]
    fn fallback_shapes_bundled_scripts_after_registration_without_dirtying_every_frame() {
        #[derive(Resource, Default)]
        struct ChangedFonts(usize);

        let mut app = App::new();
        app.init_resource::<Assets<Font>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<FontCx>()
            .init_resource::<ChangedFonts>()
            .add_systems(PostUpdate, load_font_assets_into_font_collection);
        install(&mut app).unwrap();
        app.add_systems(
            PostUpdate,
            (|fonts: Query<(), Changed<TextFont>>, mut changed: ResMut<ChangedFonts>| {
                changed.0 = fonts.iter().count();
            })
            .after(configure_fallbacks),
        );
        let korean = app.world().resource::<UiAssets>().font(Locale::Ko);
        let held_font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .remove(korean.id())
            .unwrap();
        let primary = app.world().resource::<UiAssets>().font(Locale::EnUs);
        app.world_mut().spawn(TextFont::from(primary));
        app.update();

        let mut layout = LayoutCx::default();
        let missing = shape(&mut app, &mut layout, Locale::EnUs, "한국어");
        assert!(missing.buffer().lines().any(|line| line.runs().any(|run| {
            run.clusters()
                .any(|cluster| cluster.glyphs().any(|glyph| glyph.id == 0))
        })));
        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .insert(korean.id(), held_font)
            .unwrap();
        app.update();
        assert_eq!(app.world().resource::<ChangedFonts>().0, 1);

        for (locale, text, expected_font) in [
            (Locale::EnUs, "雨夜", 1),
            (Locale::EnUs, "日本語", 1),
            (Locale::EnUs, "ひらがなカタカナ", 4),
            (Locale::EnUs, "한국어", 5),
            (Locale::ZhCn, "ҐЇЄґїє", 0),
            (Locale::ZhTw, "ҐЇЄґїє", 0),
            (Locale::ZhHk, "ҐЇЄґїє", 0),
            (Locale::Ja, "ҐЇЄґїє", 0),
            (Locale::Ko, "ҐЇЄґїє", 0),
            (Locale::ZhCn, "骨雨夜", 1),
            (Locale::ZhTw, "骨雨夜", 2),
            (Locale::ZhHk, "骨雨夜", 3),
            (Locale::Ja, "骨雨夜", 4),
            (Locale::Ko, "骨雨夜", 5),
        ] {
            let shaped = shape(&mut app, &mut layout, locale, text);
            let mut glyph_count = 0;
            for line in shaped.buffer().lines() {
                for run in line.runs() {
                    assert!(
                        run.font().data.as_ref() == FONT_BYTES[expected_font],
                        "{locale:?}: {text} expected {}",
                        FONT_FAMILIES[expected_font]
                    );
                    for cluster in run.clusters() {
                        for glyph in cluster.glyphs() {
                            assert_ne!(glyph.id, 0, "{locale:?}: {text}");
                            glyph_count += 1;
                        }
                    }
                }
            }
            assert!(glyph_count > 0, "{locale:?}: {text}");
        }
        let text = "Co 雨夜 한국어 ҐЇЄ";
        let mixed = shape(&mut app, &mut layout, Locale::EnUs, text);
        let mut used_fonts = HashSet::new();
        for line in mixed.buffer().lines() {
            for run in line.runs() {
                used_fonts.insert(
                    FONT_BYTES
                        .iter()
                        .position(|bytes| *bytes == run.font().data.as_ref())
                        .unwrap(),
                );
                assert!(
                    run.clusters()
                        .all(|cluster| cluster.glyphs().all(|glyph| glyph.id != 0))
                );
            }
        }
        assert_eq!(used_fonts, HashSet::from([0, 1, 5]));

        let unsupported = shape(&mut app, &mut layout, Locale::EnUs, "🎵𠀀𝄞");
        assert!(
            unsupported
                .buffer()
                .lines()
                .any(|line| line.runs().any(|run| {
                    run.clusters()
                        .any(|cluster| cluster.glyphs().any(|glyph| glyph.id == 0))
                }))
        );
        app.update();
        assert_eq!(app.world().resource::<ChangedFonts>().0, 0);
        app.update();
        assert_eq!(app.world().resource::<ChangedFonts>().0, 0);

        let held_font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .remove(korean.id())
            .unwrap();
        app.update();
        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .insert(korean.id(), held_font)
            .unwrap();
        app.update();
        assert_eq!(app.world().resource::<ChangedFonts>().0, 1);
        let rebuilt = shape(&mut app, &mut layout, Locale::EnUs, "한국어");
        assert!(rebuilt.buffer().lines().all(|line| line.runs().all(|run| {
            run.font().data.as_ref() == FONT_BYTES[5]
                && run
                    .clusters()
                    .all(|cluster| cluster.glyphs().all(|glyph| glyph.id != 0))
        })));
        app.update();
        assert_eq!(app.world().resource::<ChangedFonts>().0, 0);
    }
}
