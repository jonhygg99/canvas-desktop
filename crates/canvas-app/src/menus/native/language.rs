use super::AppMenus;
use muda::MenuItemKind;

pub(super) fn collect(items: Vec<MenuItemKind>) -> Vec<(MenuItemKind, String)> {
    let mut labels = Vec::new();
    for item in items {
        match &item {
            MenuItemKind::MenuItem(value) => labels.push((item.clone(), value.text())),
            MenuItemKind::Submenu(value) => {
                labels.push((item.clone(), value.text()));
                labels.extend(collect(value.items()));
            }
            _ => {}
        }
    }
    labels
}

impl AppMenus {
    pub fn set_language(&mut self, language: crate::i18n::Language) {
        if self.language == Some(language) {
            return;
        }
        self.language = Some(language);
        crate::i18n::set_language(language);
        for (item, key) in &self.translated_items {
            match item {
                MenuItemKind::MenuItem(value) => value.set_text(crate::i18n::tr(key)),
                MenuItemKind::Submenu(value) => value.set_text(crate::i18n::tr(key)),
                _ => {}
            }
        }
    }
}
