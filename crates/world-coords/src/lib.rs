//! Weltkoordinaten für große Strecken (Kap. 4 des Plans).
//!
//! Intern rechnet alles in **ECEF (f64)** auf dem GRS80/ETRS89-Ellipsoid: ein globales,
//! kartesisches System ohne Projektionszonen und ohne Nähte. Gerendert wird relativ zu einem
//! [`RenderOrigin`] (Floating Origin) in f32 — der einzige Ort, an dem `as f32` erlaubt ist.

use glam::{DQuat, DVec3, Quat, Vec3};
use serde::{Deserialize, Serialize};

pub mod geo;

/// Position im ECEF-Frame (Meter, f64). Newtype, damit f32-Pfade nicht versehentlich
/// Weltkoordinaten aufnehmen (siehe Risiko-Tabelle Kap. 19).
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcefPos(pub DVec3);

impl EcefPos {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self(DVec3::new(x, y, z))
    }

    /// Abstand zweier Weltpunkte (Sehne, nicht Bogenlänge).
    pub fn distance(self, other: EcefPos) -> f64 {
        self.0.distance(other.0)
    }
}

impl From<DVec3> for EcefPos {
    fn from(v: DVec3) -> Self {
        Self(v)
    }
}

/// Lokales ENU-Frame (East/North/Up) an einem Punkt der Erdoberfläche.
///
/// Konvention Richtung Renderer (Bevy, Y = oben, -Z = vorn):
/// `x = East`, `y = Up`, `z = -North`.
#[derive(Debug, Clone, Copy)]
pub struct EnuFrame {
    pub origin: DVec3,
    pub east: DVec3,
    pub north: DVec3,
    pub up: DVec3,
}

impl EnuFrame {
    /// ENU-Basis am gegebenen ECEF-Punkt (aus dessen geodätischer Breite/Länge).
    pub fn at(origin: EcefPos) -> Self {
        let (lat, lon, _h) = geo::from_ecef(origin);
        let (sla, cla) = lat.sin_cos();
        let (slo, clo) = lon.sin_cos();
        Self {
            origin: origin.0,
            east: DVec3::new(-slo, clo, 0.0),
            north: DVec3::new(-sla * clo, -sla * slo, cla),
            up: DVec3::new(cla * clo, cla * slo, sla),
        }
    }

    /// ECEF-Punkt → lokale ENU-Koordinaten (f64, Meter).
    pub fn to_local(&self, p: EcefPos) -> DVec3 {
        let d = p.0 - self.origin;
        DVec3::new(d.dot(self.east), d.dot(self.north), d.dot(self.up))
    }

    /// Lokale ENU-Koordinaten → ECEF.
    pub fn to_ecef(&self, local: DVec3) -> EcefPos {
        EcefPos(self.origin + self.east * local.x + self.north * local.y + self.up * local.z)
    }

    /// Wie [`EnuFrame::to_ecef`], aber mit Erdkrümmungskorrektur der Tangentialebene.
    ///
    /// Ohne Korrektur „steigt" die ENU-Ebene mit `d²/(2R)` von der Erdoberfläche weg
    /// (bei 1 km: 8 cm, bei 10 km: 7,8 m). Gleisgeometrie wird eben geplant und mit dieser
    /// Korrektur auf die Kugel gelegt — so bleiben lange Kanten in einem Frame nutzbar.
    pub fn to_ecef_curved(&self, local: DVec3) -> EcefPos {
        let r = self.origin.length().max(1.0);
        let drop = (local.x * local.x + local.y * local.y) / (2.0 * r);
        self.to_ecef(DVec3::new(local.x, local.y, local.z - drop))
    }

    /// ECEF-Richtungsvektor → lokale ENU-Richtung (ohne Translation).
    pub fn dir_to_local(&self, d: DVec3) -> DVec3 {
        DVec3::new(d.dot(self.east), d.dot(self.north), d.dot(self.up))
    }

    /// Lokale ENU-Richtung → ECEF-Richtung.
    pub fn dir_to_ecef(&self, local: DVec3) -> DVec3 {
        self.east * local.x + self.north * local.y + self.up * local.z
    }

    /// Rotation ECEF → ENU als Quaternion (f64).
    pub fn rotation(&self) -> DQuat {
        DQuat::from_mat3(&glam::DMat3::from_cols(self.east, self.north, self.up).transpose())
    }
}

/// Bezugspunkt des Renderings. Alle `Transform`s sind relativ dazu.
///
/// Springt neu, sobald die Kamera weiter als [`RenderOrigin::REBASE_DISTANCE`] entfernt ist.
/// Beim Sprung wird das ENU-Frame neu bestimmt — dadurch stimmt die Erdkrümmung automatisch,
/// ohne sie je explizit zu modellieren.
#[derive(Debug, Clone, Copy)]
pub struct RenderOrigin {
    frame: EnuFrame,
}

impl RenderOrigin {
    /// Ab dieser Kameradistanz wird der Origin nachgeführt.
    pub const REBASE_DISTANCE: f64 = 4_000.0;

    pub fn new(origin: EcefPos) -> Self {
        Self {
            frame: EnuFrame::at(origin),
        }
    }

    pub fn frame(&self) -> &EnuFrame {
        &self.frame
    }

    pub fn position(&self) -> EcefPos {
        EcefPos(self.frame.origin)
    }

    /// Setzt den Origin auf die Kameraposition, falls sie zu weit weg ist.
    /// Gibt `true` zurück, wenn ein Rebase stattgefunden hat (dann müssen alle
    /// gerenderten Posen neu berechnet werden).
    pub fn rebase_if_needed(&mut self, camera: EcefPos) -> bool {
        if self.position().distance(camera) <= Self::REBASE_DISTANCE {
            return false;
        }
        self.frame = EnuFrame::at(camera);
        true
    }

    /// Weltposition → Renderposition. **Der einzige erlaubte f64→f32-Übergang.**
    pub fn to_render(&self, p: EcefPos) -> Vec3 {
        let l = self.frame.to_local(p);
        Vec3::new(l.x as f32, l.z as f32, -l.y as f32)
    }

    /// Renderposition → Weltposition (für Picking/Editor).
    pub fn from_render(&self, v: Vec3) -> EcefPos {
        self.frame
            .to_ecef(DVec3::new(v.x as f64, -v.z as f64, v.y as f64))
    }

    /// Weltrichtung (ECEF) → Renderrichtung (normiert bleibt normiert).
    pub fn dir_to_render(&self, d: DVec3) -> Vec3 {
        let l = self.frame.dir_to_local(d);
        Vec3::new(l.x as f32, l.z as f32, -l.y as f32)
    }

    /// Transform eines Objekts, dessen Geometrie im ENU-Frame `frame` vorliegt.
    ///
    /// Damit müssen beim Origin-Rebase nur die Transforms neu gesetzt werden, nicht die
    /// Meshes: Gleisgeometrie wird einmal im Frame ihrer Kante gebaut und bleibt gültig.
    pub fn frame_transform(&self, frame: &EnuFrame) -> (Vec3, Quat) {
        let translation = self.to_render(EcefPos(frame.origin));
        let x = self.dir_to_render(frame.east);
        let y = self.dir_to_render(frame.up);
        let z = self.dir_to_render(-frame.north);
        let rotation = Quat::from_mat3(&glam::Mat3::from_cols(x, y, z));
        (translation, rotation)
    }

    /// Orientierung aus Vorwärts- und Aufwärtsrichtung (beide ECEF) für den Renderer.
    /// `forward` zeigt in Fahrtrichtung, das Ergebnis folgt Bevys Konvention (-Z = vorn).
    pub fn look_rotation(&self, forward: DVec3, up: DVec3) -> Quat {
        let f = self.dir_to_render(forward).normalize_or_zero();
        let u = self.dir_to_render(up).normalize_or_zero();
        if f.length_squared() < 0.5 || u.length_squared() < 0.5 {
            return Quat::IDENTITY;
        }
        let right = f.cross(u).normalize_or_zero();
        if right.length_squared() < 0.5 {
            return Quat::IDENTITY;
        }
        let up = right.cross(f);
        Quat::from_mat3(&glam::Mat3::from_cols(right, up, -f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{from_ecef, to_ecef};

    const BERLIN: (f64, f64, f64) = (52.5200, 13.4050, 40.0);
    const HANNOVER: (f64, f64, f64) = (52.3759, 9.7320, 55.0);

    fn deg(p: (f64, f64, f64)) -> EcefPos {
        to_ecef(p.0.to_radians(), p.1.to_radians(), p.2)
    }

    #[test]
    fn geodetic_roundtrip_is_sub_millimeter() {
        for p in [BERLIN, HANNOVER, (0.0, 0.0, 0.0), (89.0, -179.0, 3000.0)] {
            let e = deg(p);
            let (lat, lon, h) = from_ecef(e);
            assert!((lat.to_degrees() - p.0).abs() < 1e-9, "lat {p:?}");
            assert!((lon.to_degrees() - p.1).abs() < 1e-9, "lon {p:?}");
            assert!((h - p.2).abs() < 1e-3, "h {p:?}");
        }
    }

    #[test]
    fn enu_axes_are_orthonormal_and_up_points_outward() {
        let f = EnuFrame::at(deg(BERLIN));
        for v in [f.east, f.north, f.up] {
            assert!((v.length() - 1.0).abs() < 1e-12);
        }
        assert!(f.east.dot(f.north).abs() < 1e-12);
        assert!(f.east.dot(f.up).abs() < 1e-12);
        assert!(f.north.dot(f.up).abs() < 1e-12);
        // Up zeigt vom Erdmittelpunkt weg.
        assert!(f.up.dot(f.origin.normalize()) > 0.999);
        // Nord zeigt auf der Nordhalbkugel Richtung Pol (+Z).
        assert!(f.north.z > 0.0);
    }

    #[test]
    fn berlin_hannover_distance_matches_reality() {
        // Luftlinie ~ 249 km.
        let d = deg(BERLIN).distance(deg(HANNOVER));
        assert!((d - 249_000.0).abs() < 3_000.0, "d = {d}");
    }

    /// Abnahmetest M0: 300 km Distanz, kein Präzisionsverlust, kein Sprung beim Rebase.
    #[test]
    fn no_jitter_over_300km_flight_with_rebasing() {
        let start = deg(BERLIN);
        let frame = EnuFrame::at(start);
        // Zielpunkt 300 km östlich auf gleicher Höhe.
        let target = frame.to_ecef(DVec3::new(300_000.0, 0.0, 0.0));
        // Ein ortsfester Prüfpfahl 10 m neben dem Ziel.
        let post = frame.to_ecef(DVec3::new(300_010.0, 0.0, 0.0));

        let mut origin = RenderOrigin::new(start);
        let mut last_render: Option<f32> = None;
        let mut rebases = 0;

        for i in 0..=3000 {
            let t = i as f64 / 3000.0;
            let cam = EcefPos(start.0.lerp(target.0, t));
            if origin.rebase_if_needed(cam) {
                rebases += 1;
            }
            // Relativvektor Kamera→Pfahl muss stetig und exakt bleiben, egal welcher Origin.
            // Der Abstand zum fernen Prüfpfahl schrumpft gleichmäßig um die Schrittweite.
            // Ein Origin-Rebase (inkl. neuer ENU-Rotation) darf daran nichts ändern.
            let r = (origin.to_render(post) - origin.to_render(cam)).length();
            if let Some(prev) = last_render {
                let delta = prev - r;
                // Toleranz 0,5 m: der ferne Pfahl selbst liegt bei ~300 km in f32 und ist
                // dort auf ~3 cm quantisiert — für Fernsicht irrelevant, im Nahfeld unten
                // wird auf 1 mm geprüft. Ein Rebase-Sprung wäre hier dreistellig.
                assert!(
                    (delta - 100.0).abs() < 0.5,
                    "Sprung bei i={i}: Δ = {delta} m"
                );
            }
            last_render = Some(r);

            // Nahfeld: ein Pfahl 10 m neben der Kamera bleibt millimetergenau.
            let near = EcefPos(cam.0 + EnuFrame::at(cam).east * 10.0);
            let dn = (origin.to_render(near) - origin.to_render(cam)).length();
            assert!((dn - 10.0).abs() < 1e-3, "Nahfeld-Jitter bei i={i}: {dn}");

            // Renderkoordinaten bleiben klein → f32 behält Millimeterauflösung.
            let c = origin.to_render(cam);
            assert!(
                c.length() <= RenderOrigin::REBASE_DISTANCE as f32 + 200.0,
                "Origin zu weit weg: {c:?}"
            );
        }
        assert!(rebases > 50, "Rebase hat nicht gegriffen ({rebases})");

        // Am Ziel: Abstand zum Pfahl exakt 10 m, in f32-Renderkoordinaten.
        let d = (origin.to_render(post) - origin.to_render(target)).length();
        assert!((d - 10.0).abs() < 1e-3, "d = {d}");
    }

    #[test]
    fn render_roundtrip() {
        let origin = RenderOrigin::new(deg(BERLIN));
        let p = EcefPos(deg(BERLIN).0 + DVec3::new(123.0, -456.0, 789.0));
        let back = origin.from_render(origin.to_render(p));
        assert!(back.distance(p) < 1e-2);
    }
}
