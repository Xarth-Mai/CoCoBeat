use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::LazyLock};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum Locale {
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[default]
    #[serde(rename = "en-US")]
    EnUs,
    #[serde(rename = "en-GB")]
    EnGb,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "ko")]
    Ko,
    #[serde(rename = "zh-TW")]
    ZhTw,
    #[serde(rename = "zh-HK")]
    ZhHk,
    #[serde(rename = "es-419")]
    Es419,
    #[serde(rename = "pt-BR")]
    PtBr,
    #[serde(rename = "fr")]
    Fr,
    #[serde(rename = "de")]
    De,
    #[serde(rename = "ru")]
    Ru,
    #[serde(rename = "uk")]
    Uk,
}

impl Locale {
    pub const ALL: [Self; 13] = [
        Self::ZhCn,
        Self::EnUs,
        Self::EnGb,
        Self::Ja,
        Self::Ko,
        Self::ZhTw,
        Self::ZhHk,
        Self::Es419,
        Self::PtBr,
        Self::Fr,
        Self::De,
        Self::Ru,
        Self::Uk,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en-US",
            Self::EnGb => "en-GB",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::ZhTw => "zh-TW",
            Self::ZhHk => "zh-HK",
            Self::Es419 => "es-419",
            Self::PtBr => "pt-BR",
            Self::Fr => "fr",
            Self::De => "de",
            Self::Ru => "ru",
            Self::Uk => "uk",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Self::ZhCn => "简体中文",
            Self::EnUs => "English (United States)",
            Self::EnGb => "English (United Kingdom)",
            Self::Ja => "日本語",
            Self::Ko => "한국어",
            Self::ZhTw => "繁體中文（台灣）",
            Self::ZhHk => "繁體中文（香港）",
            Self::Es419 => "Español (Latinoamérica)",
            Self::PtBr => "Português (Brasil)",
            Self::Fr => "Français",
            Self::De => "Deutsch",
            Self::Ru => "Русский",
            Self::Uk => "Українська",
        }
    }

    pub fn font_index(self) -> usize {
        match self {
            Self::ZhCn => 1,
            Self::ZhTw => 2,
            Self::ZhHk => 3,
            Self::Ja => 4,
            Self::Ko => 5,
            _ => 0,
        }
    }

    pub fn flag_code(self) -> &'static str {
        match self {
            Self::ZhCn => "cn",
            Self::EnUs => "us",
            Self::EnGb => "gb",
            Self::Ja => "jp",
            Self::Ko => "kr",
            Self::ZhTw => "tw",
            Self::ZhHk => "hk",
            Self::Es419 => "mx",
            Self::PtBr => "br",
            Self::Fr => "fr",
            Self::De => "de",
            Self::Ru => "ru",
            Self::Uk => "ua",
        }
    }

    pub fn system_default() -> Self {
        sys_locale::get_locale().map_or(Self::EnUs, |tag| Self::resolve(&tag))
    }

    pub fn resolve(tag: &str) -> Self {
        let tag = tag
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .replace('_', "-")
            .to_ascii_lowercase();
        let mut parts = tag.split('-');
        let language = parts.next().unwrap_or_default();
        // Region-like values in Unicode or private extensions are not the locale region
        let subtags: Vec<_> = parts.take_while(|part| part.len() > 1).collect();
        match language {
            "zh" if subtags.contains(&"hans") => Self::ZhCn,
            "zh" if subtags.contains(&"hk") || subtags.contains(&"mo") => Self::ZhHk,
            "zh" if subtags.contains(&"hant") || subtags.contains(&"tw") => Self::ZhTw,
            "zh" => Self::ZhCn,
            "en" if subtags.contains(&"gb") => Self::EnGb,
            "en" => Self::EnUs,
            "ja" => Self::Ja,
            "ko" => Self::Ko,
            "es" => Self::Es419,
            "pt" => Self::PtBr,
            "fr" => Self::Fr,
            "de" => Self::De,
            "ru" => Self::Ru,
            "uk" => Self::Uk,
            _ => Self::EnUs,
        }
    }

    pub fn text(self, key: &'static str) -> &'static str {
        if key.is_empty() {
            return "";
        }
        CATALOGS[self as usize]
            .get(key)
            .map(String::as_str)
            .expect("Message key must exist in the bundled catalog")
    }
}

static CATALOGS: LazyLock<[HashMap<String, String>; 13]> = LazyLock::new(|| {
    [
        include_str!("../../../assets/i18n/zh-CN.json"),
        include_str!("../../../assets/i18n/en-US.json"),
        include_str!("../../../assets/i18n/en-GB.json"),
        include_str!("../../../assets/i18n/ja.json"),
        include_str!("../../../assets/i18n/ko.json"),
        include_str!("../../../assets/i18n/zh-TW.json"),
        include_str!("../../../assets/i18n/zh-HK.json"),
        include_str!("../../../assets/i18n/es-419.json"),
        include_str!("../../../assets/i18n/pt-BR.json"),
        include_str!("../../../assets/i18n/fr.json"),
        include_str!("../../../assets/i18n/de.json"),
        include_str!("../../../assets/i18n/ru.json"),
        include_str!("../../../assets/i18n/uk.json"),
    ]
    .map(|json| serde_json::from_str(json).expect("Bundled catalog must be valid JSON"))
});

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Message {
    pub key: &'static str,
    pub args: Vec<(&'static str, String)>,
}

impl Message {
    pub const fn new(key: &'static str) -> Self {
        Self {
            key,
            args: Vec::new(),
        }
    }

    pub fn with(key: &'static str, args: impl IntoIterator<Item = (&'static str, String)>) -> Self {
        Self {
            key,
            args: args.into_iter().collect(),
        }
    }

    pub fn render(&self, locale: Locale) -> String {
        interpolate(locale.text(self.key), &self.args)
    }
}

fn interpolate(mut template: &str, args: &[(&str, String)]) -> String {
    let mut output = String::with_capacity(template.len());
    while let Some(open) = template.find('{') {
        let Some(close) = template[open..].find('}').map(|index| open + index) else {
            break;
        };
        output.push_str(&template[..open]);
        let name = &template[open + 1..close];
        let (_, value) = args
            .iter()
            .find(|(key, _)| *key == name)
            .expect("Message placeholder must have an argument");
        output.push_str(value);
        template = &template[close + 1..];
    }
    output.push_str(template);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn locale_matching_preserves_regions_and_persisted_choices() {
        for (tag, locale) in [
            ("zh", Locale::ZhCn),
            ("zh_CN.UTF-8", Locale::ZhCn),
            ("zh-SG", Locale::ZhCn),
            ("zh-Hans-HK", Locale::ZhCn),
            ("zh-Hant", Locale::ZhTw),
            ("zh-Hant-CN", Locale::ZhTw),
            ("zh-TW", Locale::ZhTw),
            ("zh-Hant-HK", Locale::ZhHk),
            ("zh-MO", Locale::ZhHk),
            ("en", Locale::EnUs),
            ("en-US", Locale::EnUs),
            ("EN_gb.UTF-8", Locale::EnGb),
            ("en-US-x-gb", Locale::EnUs),
            ("en-US-u-rg-gbzzzz", Locale::EnUs),
            ("ja-JP", Locale::Ja),
            ("ko-KR", Locale::Ko),
            ("es-419", Locale::Es419),
            ("es-MX", Locale::Es419),
            ("es-AR", Locale::Es419),
            ("pt-PT", Locale::PtBr),
            ("fr-CA", Locale::Fr),
            ("de-DE", Locale::De),
            ("ru-RU", Locale::Ru),
            ("uk-UA", Locale::Uk),
            ("und", Locale::EnUs),
            ("C.UTF-8", Locale::EnUs),
            ("", Locale::EnUs),
        ] {
            assert_eq!(Locale::resolve(tag), locale, "{tag}");
        }
        assert!(Locale::ALL.contains(&Locale::system_default()));
        let mut codes = BTreeSet::new();
        let mut flags = BTreeSet::new();
        for (index, locale) in Locale::ALL.into_iter().enumerate() {
            assert_eq!(locale as usize, index);
            let encoded = serde_json::to_string(&locale).unwrap();
            assert_eq!(encoded, format!("\"{}\"", locale.code()));
            assert_eq!(serde_json::from_str::<Locale>(&encoded).unwrap(), locale);
            assert!(codes.insert(locale.code()));
            assert!(flags.insert(locale.flag_code()));
            assert!(!locale.native_name().is_empty());
            assert!(locale.font_index() < 6);
        }
        for invalid in ["en", "zh", "zh-Hant", "en-us", "unsupported"] {
            assert!(serde_json::from_value::<Locale>(serde_json::json!(invalid)).is_err());
        }
    }

    fn placeholders(mut template: &str) -> BTreeSet<&str> {
        let mut names = BTreeSet::new();
        while let Some(open) = template.find('{') {
            assert!(
                !template[..open].contains('}'),
                "Unexpected closing brace: {template}"
            );
            let close = template[open..]
                .find('}')
                .map(|index| open + index)
                .expect("Catalog placeholder requires a closing brace");
            let name = &template[open + 1..close];
            assert!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "Invalid catalog placeholder: {name}"
            );
            names.insert(name);
            template = &template[close + 1..];
        }
        assert!(
            !template.contains('}'),
            "Unexpected closing brace: {template}"
        );
        names
    }

    #[test]
    fn catalogs_have_matching_keys_and_placeholders() {
        let english = &CATALOGS[Locale::EnUs as usize];
        let keys: BTreeSet<_> = english.keys().collect();
        for locale in Locale::ALL {
            let catalog = &CATALOGS[locale as usize];
            assert_eq!(
                catalog.keys().collect::<BTreeSet<_>>(),
                keys,
                "{}",
                locale.code()
            );
            for (key, template) in catalog {
                assert!(
                    !template.trim().is_empty(),
                    "Empty {} message: {key}",
                    locale.code()
                );
                let names = placeholders(template);
                assert_eq!(
                    names,
                    placeholders(&english[key]),
                    "{} {key}",
                    locale.code()
                );
                let args: Vec<_> = names
                    .into_iter()
                    .map(|name| (name, format!("<{name}>")))
                    .collect();
                let rendered = interpolate(template, &args);
                assert!(
                    !rendered.contains(['{', '}']),
                    "Unresolved {} message: {key}",
                    locale.code()
                );
            }
        }
    }

    #[test]
    fn message_interpolation_keeps_argument_braces_literal() {
        let message = Message::with("game.saved", [("path", "replays/{player}.json".into())]);
        assert_eq!(message.render(Locale::EnUs), "Saved replays/{player}.json");
        assert_eq!(Message::default().render(Locale::EnUs), "");
        assert_eq!(
            Message::new("menu.settings").render(Locale::EnUs),
            "Settings"
        );
        assert_eq!(
            interpolate("{name}: {name} tail", &[("name", "{missing}你好".into())]),
            "{missing}你好: {missing}你好 tail"
        );
        assert!(std::panic::catch_unwind(|| Locale::EnUs.text("missing.key")).is_err());
        assert!(
            std::panic::catch_unwind(|| Message::new("game.saved").render(Locale::EnUs)).is_err()
        );
    }
}
