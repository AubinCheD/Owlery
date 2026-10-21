//! Capture de la fenêtre pour les essais : avec `OWLERY_CAPTURE=<image.png>`, chaque fois que le
//! fichier `<image.png>.demande` apparaît, Owlery enregistre l'image de sa fenêtre puis efface la
//! demande. L'image vient du rendu de Owlery : elle marche fenêtre cachée ou écran en veille, là où
//! PrintWindow ne rend que du blanc. Rien ne se passe sans la variable.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;

/// L'image demandée par `OWLERY_CAPTURE`, s'il y en a une.
pub fn target() -> Option<PathBuf> {
    std::env::var_os("OWLERY_CAPTURE").map(PathBuf::from)
}

/// Appelé avant chaque image : enregistre la fenêtre si une demande attend.
pub fn poll(ctx: &egui::Context, target: Option<&PathBuf>) {
    let Some(path) = target else { return };
    ctx.request_repaint_after(Duration::from_millis(200));
    let request = request_path(path);
    if std::fs::remove_file(&request).is_err() {
        return;
    }
    let path = path.clone();
    ctx.request_screenshot(move |img| {
        // Écrite à côté puis renommée : qui attend l'image ne la lit jamais à moitié.
        let tmp = path.with_extension("tmp");
        let written = std::fs::write(&tmp, png(img.size[0] as u32, img.size[1] as u32, img.as_raw()))
            .and_then(|_| std::fs::rename(&tmp, &path));
        if let Err(e) = written {
            eprintln!("capture {} : {e}", path.display());
        }
    });
}

fn request_path(path: &std::path::Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".demande");
    PathBuf::from(p)
}

/// Encode une image RGBA en PNG, sans compression (blocs « stored ») : assez pour des captures d'essai.
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(b) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        let len = b.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.write_all(&(data.len() as u32).to_be_bytes()).unwrap();
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for b in data {
        c ^= *b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for x in data {
        a = (a + *x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_checksums() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    /// Une image plus grande qu'un bloc (65 535 octets) : signature, en-tête et fin en place.
    #[test]
    fn png_spans_several_blocks() {
        let p = png(200, 100, &vec![255; 200 * 100 * 4]);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(&p[16..24], &[0, 0, 0, 200, 0, 0, 0, 100]);
        assert_eq!(&p[p.len() - 8..p.len() - 4], b"IEND");
    }

    #[test]
    fn requests_sit_next_to_the_image() {
        assert_eq!(request_path(std::path::Path::new(r"C:\essai\a.png")), PathBuf::from(r"C:\essai\a.png.demande"));
    }
}
