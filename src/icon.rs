//! Icône d'Owlery : une chouette dessinée en code (fenêtre, barre des tâches, zone de notification).

/// Couleurs (RGB).
const BODY: [u8; 3] = [0x5b, 0x6b, 0x8c];
const BELLY: [u8; 3] = [0xc9, 0xd3, 0xe6];
const EYE: [u8; 3] = [0xff, 0xff, 0xff];
const PUPIL: [u8; 3] = [0x1e, 0x24, 0x33];
const BEAK: [u8; 3] = [0xf2, 0xa9, 0x3b];

/// Image RGBA (non prémultipliée) de `size` × `size` pixels.
pub fn owl(size: u32) -> Vec<u8> {
    let n = size.max(1) as usize;
    let mut rgba = vec![0u8; n * n * 4];
    // Une forme : distance signée (négative à l'intérieur), en unités de l'icône (0 à 1).
    type Shape = (Box<dyn Fn(f32, f32) -> f32>, [u8; 3]);
    let shapes: Vec<Shape> = vec![
        (Box::new(|x, y| triangle(x, y, (0.16, 0.10), (0.40, 0.30), (0.18, 0.40))), BODY),
        (Box::new(|x, y| triangle(x, y, (0.84, 0.10), (0.82, 0.40), (0.60, 0.30))), BODY),
        (Box::new(|x, y| ellipse(x, y, (0.5, 0.58), (0.38, 0.40))), BODY),
        (Box::new(|x, y| ellipse(x, y, (0.5, 0.78), (0.22, 0.16))), BELLY),
        (Box::new(|x, y| ellipse(x, y, (0.35, 0.47), (0.14, 0.14))), EYE),
        (Box::new(|x, y| ellipse(x, y, (0.65, 0.47), (0.14, 0.14))), EYE),
        (Box::new(|x, y| ellipse(x, y, (0.37, 0.48), (0.065, 0.065))), PUPIL),
        (Box::new(|x, y| ellipse(x, y, (0.63, 0.48), (0.065, 0.065))), PUPIL),
        (Box::new(|x, y| triangle(x, y, (0.45, 0.58), (0.55, 0.58), (0.5, 0.69))), BEAK),
    ];
    let px = 1.0 / n as f32;
    for row in 0..n {
        for col in 0..n {
            let (x, y) = ((col as f32 + 0.5) * px, (row as f32 + 0.5) * px);
            let mut c = [0f32; 4];
            for (shape, color) in &shapes {
                // Couverture anticrénelée sur un pixel.
                let a = (0.5 - shape(x, y) / px).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let out_a = a + c[3] * (1.0 - a);
                for k in 0..3 {
                    c[k] = (f32::from(color[k]) * a + c[k] * c[3] * (1.0 - a)) / out_a.max(f32::EPSILON);
                }
                c[3] = out_a;
            }
            let i = (row * n + col) * 4;
            rgba[i] = c[0].round() as u8;
            rgba[i + 1] = c[1].round() as u8;
            rgba[i + 2] = c[2].round() as u8;
            rgba[i + 3] = (c[3] * 255.0).round() as u8;
        }
    }
    rgba
}

/// Distance signée approchée à une ellipse.
fn ellipse(x: f32, y: f32, c: (f32, f32), r: (f32, f32)) -> f32 {
    let (dx, dy) = ((x - c.0) / r.0, (y - c.1) / r.1);
    ((dx * dx + dy * dy).sqrt() - 1.0) * r.0.min(r.1)
}

/// Distance signée à un triangle (sommets dans le sens horaire de l'écran).
fn triangle(x: f32, y: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let edge = |p: (f32, f32), q: (f32, f32)| {
        let (ex, ey) = (q.0 - p.0, q.1 - p.1);
        let len = (ex * ex + ey * ey).sqrt().max(f32::EPSILON);
        // Positif à l'extérieur de l'arête.
        ((x - p.0) * ey - (y - p.1) * ex) / len
    };
    let d = edge(a, b).max(edge(b, c)).max(edge(c, a));
    // Sens inverse : on retourne le signe.
    let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    if area < 0.0 { edge(b, a).max(edge(a, c)).max(edge(c, b)) } else { d }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owl_is_opaque_in_the_middle_and_transparent_in_the_corners() {
        for size in [16, 32, 64] {
            let img = owl(size);
            assert_eq!(img.len(), (size * size * 4) as usize);
            let at = |x: u32, y: u32| &img[((y * size + x) * 4) as usize..((y * size + x) * 4 + 4) as usize];
            assert_eq!(at(0, size - 1)[3], 0, "coin");
            assert_eq!(at(size - 1, size - 1)[3], 0, "coin");
            assert_eq!(at(size / 2, size * 3 / 4)[3], 255, "ventre");
            // Les yeux sont blancs à côté de la pupille.
            let eye = at(size * 30 / 100, size * 44 / 100);
            assert!(eye[0] > 200 && eye[3] == 255, "{eye:?}");
        }
    }

    #[test]
    fn triangles_work_in_both_orientations() {
        assert!(triangle(0.5, 0.6, (0.45, 0.58), (0.55, 0.58), (0.5, 0.69)) < 0.0);
        assert!(triangle(0.5, 0.6, (0.55, 0.58), (0.45, 0.58), (0.5, 0.69)) < 0.0);
        assert!(triangle(0.1, 0.9, (0.45, 0.58), (0.55, 0.58), (0.5, 0.69)) > 0.0);
    }
}
