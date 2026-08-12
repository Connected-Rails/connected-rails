//! World coordinates for long lines (chapter 4 of the plan).
//!
//! Internally everything is computed in **ECEF (f64)** on the GRS80/ETRS89 ellipsoid: a global,
//! cartesian system without projection zones and without seams. Rendering happens relative to a
//! [`RenderOrigin`] (floating origin) in f32 — the only place where `as f32` is allowed.

use glam::{DQuat, DVec3, Quat, Vec3};
use serde::{Deserialize, Serialize};

pub mod geo;

/// Position in the ECEF frame (metres, f64). Newtype so that f32 paths cannot accidentally
/// take world coordinates (see risk table in chapter 19).
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcefPos(pub DVec3);

impl EcefPos {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self(DVec3::new(x, y, z))
    }

    /// Distance between two world points (chord, not arc length).
    pub fn distance(self, other: EcefPos) -> f64 {
        self.0.distance(other.0)
    }
}

impl From<DVec3> for EcefPos {
    fn from(v: DVec3) -> Self {
        Self(v)
    }
}

/// Local ENU frame (east/north/up) at a point on the earth's surface.
///
/// Convention towards the renderer (Bevy, Y = up, -Z = forward):
/// `x = East`, `y = Up`, `z = -North`.
#[derive(Debug, Clone, Copy)]
pub struct EnuFrame {
    pub origin: DVec3,
    pub east: DVec3,
    pub north: DVec3,
    pub up: DVec3,
}

impl EnuFrame {
    /// ENU basis at the given ECEF point (from its geodetic latitude/longitude).
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

    /// ECEF point → local ENU coordinates (f64, metres).
    pub fn to_local(&self, p: EcefPos) -> DVec3 {
        let d = p.0 - self.origin;
        DVec3::new(d.dot(self.east), d.dot(self.north), d.dot(self.up))
    }

    /// Local ENU coordinates → ECEF.
    pub fn to_ecef(&self, local: DVec3) -> EcefPos {
        EcefPos(self.origin + self.east * local.x + self.north * local.y + self.up * local.z)
    }

    /// Like [`EnuFrame::to_ecef`], but with earth curvature correction of the tangent plane.
    ///
    /// Without the correction the ENU plane "rises" away from the earth's surface by `d²/(2R)`
    /// (at 1 km: 8 cm, at 10 km: 7.8 m). Track geometry is planned flat and mapped onto the
    /// sphere with this correction — that keeps long edges usable within a single frame.
    pub fn to_ecef_curved(&self, local: DVec3) -> EcefPos {
        let r = self.origin.length().max(1.0);
        let drop = (local.x * local.x + local.y * local.y) / (2.0 * r);
        self.to_ecef(DVec3::new(local.x, local.y, local.z - drop))
    }

    /// ECEF direction vector → local ENU direction (without translation).
    pub fn dir_to_local(&self, d: DVec3) -> DVec3 {
        DVec3::new(d.dot(self.east), d.dot(self.north), d.dot(self.up))
    }

    /// Local ENU direction → ECEF direction.
    pub fn dir_to_ecef(&self, local: DVec3) -> DVec3 {
        self.east * local.x + self.north * local.y + self.up * local.z
    }

    /// Rotation ECEF → ENU as a quaternion (f64).
    pub fn rotation(&self) -> DQuat {
        DQuat::from_mat3(&glam::DMat3::from_cols(self.east, self.north, self.up).transpose())
    }
}

/// Reference point of the rendering. All `Transform`s are relative to it.
///
/// It jumps as soon as the camera is farther away than [`RenderOrigin::REBASE_DISTANCE`].
/// On a jump the ENU frame is recomputed — this way the earth curvature is correct
/// automatically, without ever modelling it explicitly.
#[derive(Debug, Clone, Copy)]
pub struct RenderOrigin {
    frame: EnuFrame,
}

impl RenderOrigin {
    /// From this camera distance on, the origin is moved along.
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

    /// Sets the origin to the camera position if it is too far away.
    /// Returns `true` if a rebase happened (in that case all rendered poses
    /// have to be recomputed).
    pub fn rebase_if_needed(&mut self, camera: EcefPos) -> bool {
        if self.position().distance(camera) <= Self::REBASE_DISTANCE {
            return false;
        }
        self.frame = EnuFrame::at(camera);
        true
    }

    /// World position → render position. **The only allowed f64→f32 transition.**
    pub fn to_render(&self, p: EcefPos) -> Vec3 {
        let l = self.frame.to_local(p);
        Vec3::new(l.x as f32, l.z as f32, -l.y as f32)
    }

    /// Render position → world position (for picking/editor).
    pub fn from_render(&self, v: Vec3) -> EcefPos {
        self.frame
            .to_ecef(DVec3::new(v.x as f64, -v.z as f64, v.y as f64))
    }

    /// World direction (ECEF) → render direction (normalised stays normalised).
    pub fn dir_to_render(&self, d: DVec3) -> Vec3 {
        let l = self.frame.dir_to_local(d);
        Vec3::new(l.x as f32, l.z as f32, -l.y as f32)
    }

    /// Transform of an object whose geometry is given in the ENU frame `frame`.
    ///
    /// This way an origin rebase only requires resetting the transforms, not the
    /// meshes: track geometry is built once in the frame of its edge and stays valid.
    pub fn frame_transform(&self, frame: &EnuFrame) -> (Vec3, Quat) {
        let translation = self.to_render(EcefPos(frame.origin));
        let x = self.dir_to_render(frame.east);
        let y = self.dir_to_render(frame.up);
        let z = self.dir_to_render(-frame.north);
        let rotation = Quat::from_mat3(&glam::Mat3::from_cols(x, y, z));
        (translation, rotation)
    }

    /// Orientation from forward and up direction (both ECEF) for the renderer.
    /// `forward` points in the direction of travel, the result follows Bevy's
    /// convention (-Z = forward).
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
        // Up points away from the centre of the earth.
        assert!(f.up.dot(f.origin.normalize()) > 0.999);
        // In the northern hemisphere north points towards the pole (+Z).
        assert!(f.north.z > 0.0);
    }

    #[test]
    fn berlin_hannover_distance_matches_reality() {
        // Straight-line distance ~ 249 km.
        let d = deg(BERLIN).distance(deg(HANNOVER));
        assert!((d - 249_000.0).abs() < 3_000.0, "d = {d}");
    }

    /// Acceptance test M0: 300 km distance, no loss of precision, no jump on rebase.
    #[test]
    fn no_jitter_over_300km_flight_with_rebasing() {
        let start = deg(BERLIN);
        let frame = EnuFrame::at(start);
        // Target point 300 km to the east at the same height.
        let target = frame.to_ecef(DVec3::new(300_000.0, 0.0, 0.0));
        // A stationary reference post 10 m next to the target.
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
            // The relative vector camera→post must stay continuous and exact, whichever
            // origin is active. The distance to the far reference post shrinks evenly by
            // the step size. An origin rebase (incl. new ENU rotation) must not change that.
            let r = (origin.to_render(post) - origin.to_render(cam)).length();
            if let Some(prev) = last_render {
                let delta = prev - r;
                // Tolerance 0.5 m: the far post itself sits at ~300 km in f32 and is
                // quantised to ~3 cm there — irrelevant for distant view, the near field
                // below is checked to 1 mm. A rebase jump would be three digits here.
                assert!((delta - 100.0).abs() < 0.5, "jump at i={i}: Δ = {delta} m");
            }
            last_render = Some(r);

            // Near field: a post 10 m next to the camera stays millimetre-accurate.
            let near = EcefPos(cam.0 + EnuFrame::at(cam).east * 10.0);
            let dn = (origin.to_render(near) - origin.to_render(cam)).length();
            assert!((dn - 10.0).abs() < 1e-3, "near-field jitter at i={i}: {dn}");

            // Render coordinates stay small → f32 keeps millimetre resolution.
            let c = origin.to_render(cam);
            assert!(
                c.length() <= RenderOrigin::REBASE_DISTANCE as f32 + 200.0,
                "origin too far away: {c:?}"
            );
        }
        assert!(rebases > 50, "rebase did not kick in ({rebases})");

        // At the target: distance to the post exactly 10 m, in f32 render coordinates.
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
