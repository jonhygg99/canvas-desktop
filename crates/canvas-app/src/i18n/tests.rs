use super::*;

#[test]
fn switching_language_changes_ui_but_preserves_unknown_names() {
    set_language(Language::Spanish);
    assert_eq!(tr("Settings"), "Ajustes");
    assert_eq!(tr("family-photo.png"), "family-photo.png");
    set_language(Language::English);
    assert_eq!(tr("Settings"), "Settings");
}

#[test]
fn language_is_persisted_and_old_settings_default_to_english() {
    let mut settings: crate::settings::AppSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.language, Language::English);
    settings.language = Language::Spanish;
    let restored: crate::settings::AppSettings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(restored.language, Language::Spanish);
}

#[test]
fn spanish_settings_render_translated_controls_with_unchanged_enum_values() {
    let ctx = eframe::egui::Context::default();
    let mut settings = crate::settings::AppSettings {
        language: Language::Spanish,
        ..Default::default()
    };
    set_language(settings.language);
    let mut open = true;
    for frame in 0..3 {
        let output = ctx.run_ui(Default::default(), |_| {
            crate::settings::settings_window(&ctx, &mut settings, &mut open, "");
        });
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                eframe::egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        if frame == 2 {
            assert!(text.contains("Idioma"), "rendered settings: {text}");
            assert!(text.contains("Densidad de interfaz"));
            assert!(text.contains("Sistema"));
        }
    }
    assert_eq!(settings.language, Language::Spanish);
    set_language(Language::English);
}

/// Carpetas cuyas cadenas de UI tienen que estar TODAS en el catálogo: los
/// paneles del sidebar que buscan imágenes (la pestaña Images y sus tres
/// fuentes).
///
/// No se escanea el crate entero porque hay literales que legítimamente no se
/// traducen y no hay forma automática de distinguirlos de una frase: símbolos
/// («✕», «—»), teclas («W», «F2») y medidas («9:16»). El escáner los salta
/// porque no tienen ninguna letra ASCII. Los nombres propios que se escriben
/// igual en los dos idiomas van aparte, en `TR_UNTRANSLATED`, para que se
/// puedan leer en vez de quedar diluidos en esa regla.
const TR_SCANNED_DIRS: [&str; 3] = ["layers_panel", "serper", "unsplash"];

/// Marcas que se escriben igual en inglés y en español: NO tienen (ni deben
/// tener) entrada en el catálogo. `tr` devuelve la clave tal cual cuando no la
/// encuentra, así que esa ausencia es justo el texto correcto. Una entrada
/// idéntica en `es.json` tampoco sería un error, pero sería ruido; y si alguien
/// la añade, el assert de abajo avisa de que esta lista quedó obsoleta.
const TR_UNTRANSLATED: [&str; 2] = ["Instagram/Facebook", "Unsplash"];

/// Un `tr("…")` sin clave en el catálogo no da error: pinta el inglés y nadie
/// se entera. Le pasó a la clave «Web Images», renombrada a «Web images» sin
/// que nada se quejara, y el mismo día entraron tres tooltips nuevos a mano.
#[test]
fn every_tr_literal_in_the_sidebar_panels_has_a_key() {
    let catalog: BTreeMap<String, String> =
        serde_json::from_str(include_str!("es.json")).expect("catálogo válido");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut literals = 0;
    let mut brands = std::collections::BTreeSet::new();
    let mut missing = Vec::new();
    for dir in TR_SCANNED_DIRS {
        for path in rust_files(&root.join(dir)) {
            let text = std::fs::read_to_string(&path).expect("leer la fuente");
            for literal in tr_literals(&text) {
                if !literal.chars().any(|c| c.is_ascii_alphabetic()) {
                    continue; // símbolos, separadores y etiquetas vacías
                }
                if TR_UNTRANSLATED.contains(&literal.as_str()) {
                    brands.insert(literal);
                    continue;
                }
                literals += 1;
                if !catalog.contains_key(&literal) {
                    missing.push(format!("{} -> {literal:?}", path.display()));
                }
            }
        }
    }
    // Si el escáner se rompiera (un `tr` con otro nombre, otra forma de
    // escribirlo) el test pasaría sin mirar nada.
    assert!(
        literals >= 60,
        "el escáner solo encontró {literals} cadenas: ¿cambió la forma de llamar a `tr`?"
    );
    // Una marca que ya no se usa en estas carpetas (o cuyo texto cambió) deja
    // la excepción arriba como una puerta abierta de par en par.
    assert_eq!(
        brands.len(),
        TR_UNTRANSLATED.len(),
        "TR_UNTRANSLATED no coincide con lo escaneado: {brands:?}"
    );
    assert!(
        missing.is_empty(),
        "cadenas sin clave en es.json:\n{}",
        missing.join("\n")
    );
}

/// Los `.rs` de una carpeta, recursivo. Falla fuerte si la carpeta no existe:
/// un escaneo vacío dejaría el test de arriba en verde por accidente.
fn rust_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("leer {}: {e}", dir.display()));
    for entry in entries {
        // Sin `flatten()`: una entrada que falla se saltaba en SILENCIO, y su
        // archivo quedaba sin escanear con el test igual de verde.
        let path = entry
            .unwrap_or_else(|e| panic!("entrada de {}: {e}", dir.display()))
            .path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out
}

/// Literales que recibe `tr(…)`: TODOS los de su argumento, no solo el que va
/// pegado al paréntesis. `tr(match source { … => "Web images" })` es una llamada
/// como cualquier otra, y escanear solo el primer token dejaba 72 cadenas de
/// estas carpetas fuera de la guardia —dos de ellas sin clave en el catálogo—,
/// que es justo lo que este test existe para impedir.
///
/// Es un escáner de texto, no un parser: cuenta paréntesis, salta cadenas y
/// desescapa como lo vería el programa (`\n`, `\t`, `\r`, `\"`, `\\`).
fn tr_literals(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= chars.len() {
        let is_call = chars[i] == 't'
            && chars[i + 1] == 'r'
            && chars[i + 2] == '('
            && (i == 0 || !is_wordish(chars[i - 1]));
        if !is_call {
            i += 1;
            continue;
        }
        let (found, next) = tr_argument_literals(&chars, i + 3);
        out.extend(found);
        i = next.max(i + 3);
    }
    out
}

/// Recorre el argumento de la llamada cuyo paréntesis se abre justo antes de
/// `from`: devuelve sus literales y dónde seguir escaneando (tras su paréntesis
/// de cierre). Un argumento sin cerrar (código roto) devuelve lo leído hasta el
/// final; el test prefiere eso a quedarse sin mirar nada.
fn tr_argument_literals(chars: &[char], from: usize) -> (Vec<String>, usize) {
    let mut out = Vec::new();
    let mut depth = 1usize;
    let mut j = from;
    while j < chars.len() {
        match chars[j] {
            '"' => {
                let mut literal = String::new();
                j += 1;
                while j < chars.len() && chars[j] != '"' {
                    if chars[j] == '\\' && j + 1 < chars.len() {
                        literal.push(match chars[j + 1] {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            other => other,
                        });
                        j += 2;
                    } else {
                        literal.push(chars[j]);
                        j += 1;
                    }
                }
                out.push(literal);
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (out, j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    (out, chars.len())
}

/// Un identificador (o una comilla, el caso del propio `"tr(` de este
/// escáner) delante de `tr` significa que no es una llamada.
fn is_wordish(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '"' || c == '\''
}
