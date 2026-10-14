//! Identidad visible de los lienzos de vídeo: el nombre provisional nunca
//! renombra la fuente. El disco se reserva de verdad solo al guardar.

use std::path::{Path, PathBuf};

use crate::{deck, editor, settings::NewCanvasFormat, ytdlp::VideoAccept};

/// ¿Está libre `path` en disco y en las ranuras salvo `except`?
fn slot_path_free(deck: &deck::Deck, except: Option<usize>, path: &Path) -> bool {
    !path.exists()
        && !deck
            .slots
            .iter()
            .enumerate()
            .any(|(i, s)| except != Some(i) && s.path.as_path() == path)
}

/// Reserva `{stem}-{n:02}.{ext}` (`clip-edwin-luna-01.mp4`…) libre en
/// disco y en las ranuras del deck. El nombre sin número cuenta como el 01
/// de la serie: si ya está ocupado, la serie empieza en el 02. Mismo
/// esquema que el resto de la app; el disco solo se mira, la reserva real
/// la hace el guardado.
pub(super) fn reserve_clip_slot_path(
    deck: &deck::Deck,
    folder: &Path,
    stem: &str,
    ext: &str,
) -> Result<PathBuf, String> {
    let bare = folder.join(format!("{stem}.{ext}"));
    let bare_taken = !slot_path_free(deck, None, &bare);
    for n in 1..=10_000u32 {
        if n == 1 && bare_taken {
            continue;
        }
        let candidate = folder.join(format!("{stem}-{n:02}.{ext}"));
        if slot_path_free(deck, None, &candidate) {
            return Ok(candidate);
        }
    }
    Err("No free canvas name remains; nothing was overwritten.".into())
}

pub(super) fn configure_video_slot(
    deck: &mut deck::Deck,
    index: usize,
    doc: &mut deck::SlotDoc,
    accept: &VideoAccept,
    format: NewCanvasFormat,
    multiple: bool,
) -> Result<(), String> {
    let ext = super::video_canvases::slot_ext(accept, format);
    let name = super::video_canvases::clip_slot_name(accept, multiple);
    // En una carpeta con nombre, el tallo es la carpeta (`clip-edwin-luna`)
    // y el número lo pone la reserva; sin carpeta se conserva el nombre del
    // clip, como antes.
    let legacy = if deck.folder.is_some() || !multiple {
        accept
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("clip")
    } else {
        &name
    };
    let stem =
        crate::naming::clip_prefix(deck.folder.as_deref()).unwrap_or_else(|| legacy.to_owned());
    let folder = deck.folder.clone().unwrap_or_default();
    // Sin carpeta se conserva el nombre del clip tal cual cuando está
    // libre; el número solo aparece al colisionar. Con carpeta manda la
    // serie numerada (el tallo es la carpeta, no el clip).
    let path = if deck.folder.is_none() {
        let bare = folder.join(format!("{stem}.{ext}"));
        if slot_path_free(deck, Some(index), &bare) {
            bare
        } else {
            reserve_clip_slot_path(deck, &folder, &stem, ext)?
        }
    } else {
        reserve_clip_slot_path(deck, &folder, &stem, ext)?
    };
    let slot = &mut deck.slots[index];
    slot.name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    slot.path = path.clone();
    slot.is_placeholder = true;
    slot.kind = crate::gallery::ItemKind::Video;
    doc.canvas_name = Some(slot.name.clone());
    if format == NewCanvasFormat::Canvas {
        doc.doc.source_path = deck.folder.as_ref().map(|_| path);
    }
    Ok(())
}

/// Solo cambia la provisional, incluso si su documento todavía lee Download.
pub(crate) fn rename_provisional(
    deck: &mut deck::Deck,
    state: &mut editor::EditorState,
    id: u64,
    stem: &str,
) -> Result<bool, String> {
    let Some(index) = deck.find_by_id(id) else {
        return Ok(false);
    };
    if !deck.slots[index].is_placeholder {
        return Ok(false);
    }
    if stem.trim().is_empty() || stem.contains(['/', '\\', ':']) || stem == "." || stem == ".." {
        return Err("Please use a non-empty file name without path separators.".into());
    }
    let old = &deck.slots[index].path;
    let folder = old.parent().unwrap_or_else(|| Path::new(""));
    let ext = old.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    let stem = stem.trim();
    // Un renombrado explícito conserva el nombre pelado cuando está libre;
    // la serie numerada solo entra al colisionar (la propia ranura no
    // cuenta: renombrar al mismo nombre lo conserva, no lo duplica).
    let bare = folder.join(format!("{stem}.{ext}"));
    let path = if slot_path_free(deck, Some(index), &bare) {
        bare
    } else {
        reserve_clip_slot_path(deck, folder, stem, ext)?
    };
    let slot = &mut deck.slots[index];
    slot.name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    slot.path = path.clone();
    if index == deck.active {
        state.canvas_name = Some(slot.name.clone());
        if state.is_design {
            state.doc.source_path = deck.folder.as_ref().map(|_| path);
        }
    } else if let deck::SlotContent::Ready(doc) = &mut slot.content {
        doc.canvas_name = Some(slot.name.clone());
        if doc.is_design {
            doc.doc.source_path = deck.folder.as_ref().map(|_| path);
        }
    }
    Ok(true)
}
