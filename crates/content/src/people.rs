//! The people on the platforms (plan ch. 12): every `Platform` device of a line
//! gets a waiting crowd, placed here and drawn by the renderer.
//!
//! Nobody is stored and nobody is sent. A crowd is a pure function of the line —
//! its name is the seed, the device index is mixed in — so every restart and every
//! client of a multiplayer run shows the same people in the same places, and a
//! line of a hundred stations costs a few numbers per station rather than a file
//! of positions. The instances are prepared the way [`crate::Scenery`] prepares
//! its objects: resolved against the track once, bucketed by terrain tile, and
//! placed on the tile's ground when that tile is built, so they stream with it.

use crate::route::{DeviceSource, LineSource};
use crate::terrain::{CellMap, HeightGrid, Rng, TileKey, bucket, model_rotation, to_render};
use glam::{DQuat, DVec2, DVec3};
use track_model::{DeviceKind, PlatformPayload, TrackNetwork};
use world_coords::{EcefPos, EnuFrame, geo};

/// One person per this much platform [m] — a quiet suburban platform, not the
/// rush hour. The count is clamped to [`MIN_CROWD`]..=[`MAX_CROWD`].
pub const PERSON_SPACING: f64 = 6.0;
/// A platform is never empty — a station with nobody on it reads as abandoned.
pub const MIN_CROWD: usize = 1;
/// A long platform stops filling up here: sixty skinned people at one station
/// is what the renderer is budgeted for.
pub const MAX_CROWD: usize = 60;
/// Nearest a person stands to the track centre [m] — the platform edge is at
/// about 1.65 m, the safety line half a metre behind it.
const NEAREST: f64 = 2.3;
/// The crowd spreads at least this far from the track centre [m], further where
/// the device's own offset says the platform is wider.
const FARTHEST: f64 = 3.5;
/// A person facing the track looks up to this far away from it [deg].
const FACING_SPREAD_DEG: f64 = 40.0;
/// Share of the crowd that looks along the platform instead of at the track.
const ALONG_SHARE: f64 = 0.15;

/// How a person stands — which of the model's clips it plays
/// ([`crate::characters`] lists them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pose {
    /// Looping stand with a little life in it.
    Idle,
    /// The second looping stand.
    Idle2,
    /// Single-frame standing poses.
    Stand,
    Stand2,
    Stand3,
    /// Single-frame, feet on the floor, seat about 0.45 m up.
    Sit,
}

impl Pose {
    /// The clip the pose plays.
    pub fn clip(self) -> &'static str {
        match self {
            Pose::Idle => "idle",
            Pose::Idle2 => "idle2",
            Pose::Stand => "stand",
            Pose::Stand2 => "stand2",
            Pose::Stand3 => "stand3",
            Pose::Sit => "sit",
        }
    }

    /// Whether the clip runs (looping) or is a held frame.
    pub fn is_looping(self) -> bool {
        matches!(self, Pose::Idle | Pose::Idle2)
    }
}

/// One person on a tile, in the tile's own frame — what the renderer spawns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PersonInstance {
    /// The origin between the feet in render axes, relative to the tile anchor.
    pub pos: [f32; 3],
    /// Orientation in render axes (`x, y, z, w`): the face along the chosen
    /// direction, up the local vertical.
    pub rotation: [f32; 4],
    /// Index into [`Crowd::characters`].
    pub character: u16,
    pub pose: Pose,
    /// Where in its clip the person starts, 0..1 — so no two move in step.
    pub phase: f32,
}

/// One person, resolved against the track: UTM position for the tile lookup,
/// the rail-plane base at its lateral distance and the two directions that
/// orient it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlacedPerson {
    pos: DVec2,
    /// Point on the rail plane at the person's lateral distance.
    base: EcefPos,
    /// Local vertical at the base.
    up: DVec3,
    /// Where the face points.
    dir: DVec3,
    /// The platform surface above the rail plane [m]; 0 = on the ground.
    height: f64,
    character: u16,
    pose: Pose,
    phase: f32,
}

/// The crowd of a line, prepared for tile builds.
#[derive(Debug, Clone, Default)]
pub struct Crowd {
    characters: Vec<String>,
    placed: Vec<PlacedPerson>,
    by_tile: CellMap<Vec<u32>>,
}

impl Crowd {
    /// The crowd of every `Platform` device of the line whose payload parses,
    /// drawn from `characters` (the passenger-role characters of the installed
    /// mods, `"<mod>:<name>"`). No characters, no crowd.
    pub fn from_line(
        line: &LineSource,
        net: &TrackNetwork,
        zone: u8,
        characters: &[String],
        seed: u64,
    ) -> Self {
        Self::from_parts(&line.devices, net, zone, characters, seed)
    }

    pub fn from_parts(
        devices: &[DeviceSource],
        net: &TrackNetwork,
        zone: u8,
        characters: &[String],
        seed: u64,
    ) -> Self {
        let mut placed = Vec::new();
        if !characters.is_empty() {
            for (index, device) in devices.iter().enumerate() {
                if device.kind != DeviceKind::Platform {
                    continue;
                }
                // A payload that is not a platform's is a file defect the editor's
                // rule check reports; here it is simply a platform without a crowd.
                let Some(platform) = PlatformPayload::parse(&device.payload) else {
                    continue;
                };
                // Compile refused dangling indices; a guard keeps a stale file
                // harmless.
                let Some(edge) = net.edges().get(device.edge as usize) else {
                    continue;
                };
                let mut rng = Rng(device_seed(seed, index));
                let count = crowd_size(platform.length);
                for _ in 0..count {
                    let s = (device.s + rng.f64() * platform.length).clamp(0.0, edge.length());
                    let pose = edge.eval(s);
                    let right = pose.tangent.cross(pose.up).normalize();
                    // The device convention: positive = left of increasing arc length.
                    let side = if device.lateral_offset >= 0.0 {
                        -right
                    } else {
                        right
                    };
                    let distance = rng.range(NEAREST, FARTHEST.max(device.lateral_offset.abs()));
                    let base = EcefPos(pose.pos.0 + side * distance);
                    let dir = if rng.f64() < ALONG_SHARE {
                        if rng.f64() < 0.5 {
                            pose.tangent
                        } else {
                            -pose.tangent
                        }
                    } else {
                        let turn = rng
                            .range(-FACING_SPREAD_DEG, FACING_SPREAD_DEG)
                            .to_radians();
                        DQuat::from_axis_angle(pose.up, turn) * -side
                    };
                    let character = rng.below(characters.len()) as u16;
                    let pick = rng.f64();
                    let stand = rng.below(3);
                    let phase = rng.f64() as f32;
                    let (lat, lon, _) = geo::from_ecef(base);
                    let (e, n) = geo::to_utm(lat, lon, zone);
                    placed.push(PlacedPerson {
                        pos: DVec2::new(e, n),
                        base,
                        up: pose.up,
                        dir,
                        height: platform.height.max(0.0),
                        character,
                        pose: standing_pose(pick, stand),
                        phase,
                    });
                }
            }
        }
        Self {
            characters: characters.to_vec(),
            placed,
            by_tile: CellMap::default(),
        }
    }

    /// The character names (`"<mod>:<name>"`) that [`PersonInstance::character`]
    /// indexes.
    pub fn characters(&self) -> &[String] {
        &self.characters
    }

    /// How many people the line's platforms carry altogether.
    pub fn len(&self) -> usize {
        self.placed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// Sorts the people into the tile grid.
    pub(crate) fn bucket(&mut self, tile_size: f64) {
        self.by_tile = bucket(self.placed.iter().map(|p| p.pos), tile_size);
    }
}

/// The seed of a line: its name hashed (FNV-1a), so it is the same on every
/// machine and every start, whatever else changes.
pub fn line_seed(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// The line's seed mixed with the device index — each platform its own stream,
/// so adding a station leaves the others' crowds where they were.
fn device_seed(seed: u64, index: usize) -> u64 {
    seed ^ (index as u64 + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93)
}

/// How many people a platform of this length gets.
fn crowd_size(length: f64) -> usize {
    ((length / PERSON_SPACING).round().max(0.0) as usize).clamp(MIN_CROWD, MAX_CROWD)
}

/// The standing pose for a draw `pick` in 0..1 and a choice `stand` in 0..3:
/// 45 % `idle`, 25 % `idle2`, the rest the three held stands.
fn standing_pose(pick: f64, stand: usize) -> Pose {
    if pick < 0.45 {
        Pose::Idle
    } else if pick < 0.70 {
        Pose::Idle2
    } else {
        [Pose::Stand, Pose::Stand2, Pose::Stand3][stand % 3]
    }
}

/// Places the crowd of the tile: on the platform's height above the rail
/// plane, or — where the line models no platform (`height` 0) — on the height
/// grid, which near the track is the formation beside the ballast.
pub(crate) fn scatter_people(
    k: TileKey,
    grid: &HeightGrid,
    frame: &EnuFrame,
    crowd: &Crowd,
) -> Vec<PersonInstance> {
    let Some(indices) = crowd.by_tile.get(&k) else {
        return Vec::new();
    };
    indices
        .iter()
        .map(|&i| {
            let person = &crowd.placed[i as usize];
            let anchor = if person.height > 0.0 {
                EcefPos(person.base.0 + person.up * person.height)
            } else {
                let ground = grid.at(person.pos);
                let (lat, lon, _) = geo::from_ecef(person.base);
                geo::to_ecef(lat, lon, ground)
            };
            PersonInstance {
                pos: to_render(frame.to_local(anchor)),
                rotation: model_rotation(frame, person.dir, person.up).to_array(),
                character: person.character,
                pose: person.pose,
                phase: person.phase,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{TerrainBuilder, TerrainOptions, TerrainStats, TerrainTile, key};
    use track_model::{EdgeId, Facing, NodeKind, Segment, TrackEdge};

    /// Straight 1 km test line at 52° N, 10° E, heading east.
    fn test_net() -> TrackNetwork {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            geo::to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        net
    }

    fn platform(s: f64, lateral_offset: f64, payload: &str) -> DeviceSource {
        DeviceSource {
            kind: DeviceKind::Platform,
            edge: 0,
            s,
            facing: Facing::Both,
            lateral_offset,
            payload: payload.to_string(),
        }
    }

    fn characters() -> Vec<String> {
        vec!["people:a".into(), "people:b".into(), "people:c".into()]
    }

    /// The person's place along the edge and beside it [m]: `(s, left)`,
    /// positive `left` on the left of increasing arc length.
    fn along_and_beside(net: &TrackNetwork, person: &PlacedPerson) -> (f64, f64) {
        let edge = &net.edges()[0];
        let start = edge.eval(0.0);
        let frame = EnuFrame::at(start.pos);
        let local = frame.to_local(person.base);
        // Heading east: east is `s`, north is left.
        (local.x, local.y)
    }

    #[test]
    fn the_crowd_is_deterministic_in_its_seed() {
        let net = test_net();
        let devices = vec![platform(200.0, 5.0, "(name:\"A\",length:210.0)")];
        let seed = line_seed("Beispielstrecke");
        let once = Crowd::from_parts(&devices, &net, 32, &characters(), seed);
        let again = Crowd::from_parts(&devices, &net, 32, &characters(), seed);
        assert_eq!(once.placed, again.placed);
        assert_eq!(once.len(), 35, "one person per six metres");
        let other = Crowd::from_parts(&devices, &net, 32, &characters(), seed ^ 1);
        assert_ne!(once.placed, other.placed);
        // The seed of a name is the same on every machine.
        assert_eq!(line_seed("Beispielstrecke"), line_seed("Beispielstrecke"));
        assert_ne!(line_seed("Beispielstrecke"), line_seed("Musterbahn"));
    }

    #[test]
    fn everyone_stands_within_the_platform_on_its_side() {
        let net = test_net();
        for (offset, sign) in [(5.0, 1.0), (-2.0, -1.0)] {
            let devices = vec![platform(300.0, offset, "(name:\"A\",length:120.0)")];
            let crowd = Crowd::from_parts(&devices, &net, 32, &characters(), 7);
            assert_eq!(crowd.len(), 20);
            let spread = FARTHEST.max(offset.abs());
            for person in &crowd.placed {
                let (s, left) = along_and_beside(&net, person);
                assert!((300.0 - 0.01..=420.01).contains(&s), "s = {s}");
                let beside = left * sign;
                assert!(
                    (NEAREST - 0.01..=spread + 0.01).contains(&beside),
                    "beside = {beside}"
                );
                assert!((person.character as usize) < characters().len());
                assert!((0.0..1.0).contains(&person.phase));
                // Facing the track (give or take), or along it.
                let towards = -(person.dir.dot(person.base.0 - net.edges()[0].eval(s).pos.0));
                let along = person.dir.dot(net.edges()[0].eval(s).tangent).abs();
                assert!(
                    towards > 0.0 || along > 0.99,
                    "looks away from the track: {:?}",
                    person.dir
                );
            }
            // A mix of poses, not one of them.
            let idle = crowd.placed.iter().filter(|p| p.pose == Pose::Idle).count();
            assert!(idle > 0 && idle < crowd.len());
        }
    }

    #[test]
    fn a_platform_without_a_payload_is_skipped_and_nothing_else_makes_a_crowd() {
        let net = test_net();
        let none = Crowd::from_parts(&[], &net, 32, &characters(), 1);
        assert!(none.is_empty());
        let devices = vec![
            DeviceSource {
                kind: DeviceKind::Signal,
                edge: 0,
                s: 100.0,
                facing: Facing::Forward,
                lateral_offset: 3.0,
                payload: "(signal:Some(0))".into(),
            },
            platform(100.0, 5.0, "(frequency:Hz1000)"),
            platform(100.0, 5.0, "garbage"),
            platform(100.0, 5.0, "(name:\"Short\",length:2.0)"),
        ];
        let crowd = Crowd::from_parts(&devices, &net, 32, &characters(), 1);
        assert_eq!(
            crowd.len(),
            1,
            "the broken payloads are skipped, the short platform gets one"
        );
        // A platform on an edge the network does not have is skipped too.
        let stale = vec![DeviceSource {
            edge: 9,
            ..platform(100.0, 5.0, "(name:\"X\",length:60.0)")
        }];
        assert!(Crowd::from_parts(&stale, &net, 32, &characters(), 1).is_empty());
        // Without characters there is nobody to place.
        let devices = vec![platform(100.0, 5.0, "(name:\"A\",length:60.0)")];
        assert!(Crowd::from_parts(&devices, &net, 32, &[], 1).is_empty());
    }

    #[test]
    fn the_crowd_stands_on_the_platform_or_on_the_ground() {
        let net = test_net();
        let devices = vec![
            platform(100.0, 5.0, "(name:\"Ground\",length:60.0)"),
            platform(700.0, -5.0, "(name:\"Built\",length:60.0,height:0.76)"),
        ];
        let crowd = Crowd::from_parts(&devices, &net, 32, &characters(), 3);
        // Flat ground at rail height (the test line is at 100 m ellipsoidal): the
        // formation beside the ballast then lies `rail_offset` below the rail.
        let options = TerrainOptions {
            radius: 400.0,
            fallback_height: 100.0 - 46.0,
            ..Default::default()
        };
        let builder = TerrainBuilder::new(&net, vec![], options).with_crowd(crowd.clone());
        assert_eq!(builder.crowd_characters(), characters().as_slice());
        let mut stats = TerrainStats::default();
        let tiles: Vec<TerrainTile> = builder
            .corridor_keys()
            .into_iter()
            .filter_map(|k| builder.build_key(k, &mut stats))
            .collect();
        let placed: Vec<(&TerrainTile, &PersonInstance)> = tiles
            .iter()
            .flat_map(|t| t.people.iter().map(move |p| (t, p)))
            .collect();
        assert_eq!(placed.len(), 20, "every person stands on exactly one tile");
        let world = |tile: &TerrainTile, p: &PersonInstance| {
            let frame = EnuFrame::at(tile.anchor);
            let q = p.pos;
            frame.to_ecef(DVec3::new(q[0] as f64, -q[2] as f64, q[1] as f64))
        };
        let rail = |s: f64| geo::from_ecef(net.edges()[0].eval(s).pos).2;
        let mut on_ground = 0;
        let mut on_platform = 0;
        for (tile, person) in &placed {
            let foot = world(tile, person);
            let (_, _, h) = geo::from_ecef(foot);
            let frame = EnuFrame::at(net.edges()[0].eval(0.0).pos);
            let s = frame.to_local(foot).x;
            if s < 400.0 {
                // No platform body: on the ground as the tile draws it, which
                // beside the ballast is the formation below the rail head. The
                // tile is a 4 m grid, so the exact surface is only nearly it.
                let ground = builder.surface_height(foot);
                assert!((h - ground).abs() < 0.25, "{h:.2} on ground {ground:.2}");
                assert!(h <= rail(s) + 0.01 && h >= rail(s) - 0.41, "{h:.2} vs rail");
                on_ground += 1;
            } else {
                assert!((h - rail(s) - 0.76).abs() < 0.05, "{h:.2} on platform");
                on_platform += 1;
            }
            // Upright: the rotation's up is the local vertical.
            let up = glam::Quat::from_array(person.rotation) * glam::Vec3::Y;
            assert!(up.y > 0.999, "leaning: {up:?}");
        }
        assert_eq!((on_ground, on_platform), (10, 10));

        // The tile a person is filed under is the tile it lands on.
        let mut bucketed = crowd.clone();
        bucketed.bucket(options.tile_size);
        for person in &bucketed.placed {
            let k = key(person.pos, options.tile_size);
            assert!(!bucketed.by_tile[&k].is_empty());
        }
        // Rescattering onto the built tile gives the same placement.
        let tile = placed[0].0;
        let (_, _, again) = builder.rescatter(tile);
        assert_eq!(again, tile.people);
    }

    #[test]
    fn sizes_and_poses_follow_their_shares() {
        assert_eq!(crowd_size(210.0), 35);
        assert_eq!(crowd_size(2.0), MIN_CROWD);
        assert_eq!(crowd_size(1_000.0), MAX_CROWD);
        assert_eq!(standing_pose(0.1, 0), Pose::Idle);
        assert_eq!(standing_pose(0.5, 0), Pose::Idle2);
        assert_eq!(standing_pose(0.8, 1), Pose::Stand2);
        assert!(Pose::Idle.is_looping() && !Pose::Stand3.is_looping());
        assert_eq!(Pose::Sit.clip(), "sit");
    }
}
