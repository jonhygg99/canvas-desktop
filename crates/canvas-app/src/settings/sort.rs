//! Comparación «natural» de nombres (números dentro del texto). La usa el
//! orden «Name» de la galería y de la baraja.

/// Compara dos nombres tratando cada tramo de dígitos consecutivos como un
/// número, no como texto — así "6.png" ordena antes que "51.png" en vez de
/// después. El `Ord` de `String` puro (byte a byte) es lo que usaba antes
/// `GallerySort::Name`: en una carpeta de fotos numeradas sin ceros de
/// relleno (`1.png`..`51.png`) las de un solo dígito (6,7,8,9) acababan
/// DESPUÉS de la 51 — una sorpresa real, no una preferencia de nadie.
/// Explorador de Windows y Finder ya comparan así por defecto, así que
/// "Name" pasa a significar esto en vez de añadir un tercer criterio.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        let (ac, bc) = match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) => (ac, bc),
        };
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let mut a_run = String::new();
            while let Some(&c) = ai.peek() {
                if !c.is_ascii_digit() {
                    break;
                }
                a_run.push(c);
                ai.next();
            }
            let mut b_run = String::new();
            while let Some(&c) = bi.peek() {
                if !c.is_ascii_digit() {
                    break;
                }
                b_run.push(c);
                bi.next();
            }
            // Sin ceros a la izquierda para comparar el valor, no los
            // dígitos: longitud primero (más dígitos = número mayor, ya sin
            // ceros de relleno), luego lexicográfico como desempate entre
            // tramos de igual longitud.
            let a_trimmed = a_run.trim_start_matches('0');
            let b_trimmed = b_run.trim_start_matches('0');
            let ord = a_trimmed
                .len()
                .cmp(&b_trimmed.len())
                .then_with(|| a_trimmed.cmp(b_trimmed));
            if ord != Ordering::Equal {
                return ord;
            }
            // Mismo valor numérico (p.ej. "007" y "7"): sigue comparando el
            // resto del nombre en vez de darlo por empatado aquí.
        } else {
            let al = ac.to_ascii_lowercase();
            let bl = bc.to_ascii_lowercase();
            if al != bl {
                return al.cmp(&bl);
            }
            ai.next();
            bi.next();
        }
    }
}
