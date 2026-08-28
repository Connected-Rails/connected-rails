//! How people are drawn (plan ch. 12): the crowd on a platform, the passengers
//! in the seats and the walker's own body are all the same thing here — a
//! character scene out of the mods' `characters/*.ron`, put down at a pose and
//! finished once its hierarchy exists.
//!
//! A character is a scene instance, not a flattened tree: it is skinned, so its
//! meshes are nothing without the skeleton and the [`AnimationPlayer`] the
//! loader hangs on the scene root. What the loader does not do is the rest —
//! the `_LOD<n>` nodes become [`VisibilityRange`] bands, the clips become an
//! [`AnimationGraph`] (one per glTF, cached), the chosen clip is started at its
//! phase, and the texture atlases get the mip chain the pipeline does not
//! ship. [`dress_people`] does that once per instance; nothing here runs per
//! frame on a person that is finished.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use bevy::animation::RepeatAnimation;
use bevy::animation::graph::AnimationNodeIndex;
use bevy::animation::transition::AnimationTransitions;
use bevy::asset::LoadState;
use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::{Gltf, GltfAssetLabel};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{TextureDimension, TextureFormat};
use bevy::world_serialization::WorldAsset;
use content::CharacterSpec;
use content::people::Pose;
use sim_core::train::{SeatSpec, lod_level};

use crate::{asset_path, with_mipmaps};

/// Past this distance nobody on a platform is drawn [m] — at half a kilometre
/// a person is a pixel, and a station of sixty is sixty skinned draws for it.
pub const PERSON_CULL: f32 = 500.0;
/// Past this distance nobody aboard a train is drawn [m]: a passenger sits
/// behind glass and is smaller in the picture than the same person on a
/// platform, so the seats go before the crowd does.
pub const PASSENGER_CULL: f32 = 300.0;
/// Where one level of detail of a character hands over to the next [m]:
/// `_LOD0` (about 14 000 triangles) up to the first, `_LOD1` (5 000) up to the
/// second, `_LOD2` (1 600) up to the third, `_LOD3` (500) on to the cull
/// distance. A character with fewer levels runs its last one to the end.
pub const PERSON_LOD_BANDS: [f32; 3] = [30.0, 80.0, 200.0];
/// Anisotropic samples on a character's atlases. The atlases are looked at
/// obliquely (a coat seen from the platform, a face seen from the side), and 4
/// is where the cost stops paying for itself on a texture this size.
const TEXTURE_ANISOTROPY: u16 = 4;
/// A looping clip runs between 1 − this and 1 + this of its own speed, picked
/// by the person's phase, so a crowd that shares one clip does not breathe in
/// step.
const IDLE_SPEED_SPREAD: f32 = 0.1;
/// Frames a texture is waited for before it is given up on — a scene whose
/// atlas never arrives is a broken file, not a slow disk.
const TEXTURE_PATIENCE: u32 = 600;
/// Share of the seats a train has that are taken.
pub const SEAT_TAKEN_SHARE: f32 = 0.65;

/// The render assets of one character: its glTF, which is where the clips
/// come from, and the scene that is instantiated per person.
#[derive(Clone)]
pub struct CharacterAssets {
    pub gltf: Handle<Gltf>,
    pub scene: Handle<WorldAsset>,
}

impl CharacterAssets {
    /// Loads the character model at `model` (relative to `mods/`, like every
    /// other model).
    pub fn load(assets: &AssetServer, model: &str) -> Self {
        let path = asset_path(model);
        Self {
            gltf: assets.load(path.clone()),
            scene: assets.load(GltfAssetLabel::Scene(0).from_asset(path)),
        }
    }
}

/// The passenger characters a world is drawn with, in the order the crowd
/// indexes them ([`content::PersonInstance::character`]). A slot is `None`
/// where the name resolved to no installed character — that person is not
/// drawn rather than drawn as something else.
#[derive(Clone, Default)]
pub struct Passengers(Vec<Option<CharacterAssets>>);

impl Passengers {
    /// Resolves the names against the installed mods' `characters/*.ron`,
    /// unknown names logged once.
    pub fn resolve(
        names: &[String],
        registry: &BTreeMap<String, CharacterSpec>,
        assets: &AssetServer,
    ) -> Self {
        Self(
            names
                .iter()
                .map(|name| match registry.get(name) {
                    Some(spec) => Some(CharacterAssets::load(assets, &spec.model)),
                    None => {
                        warn!("people: unknown character {name:?} — not drawn");
                        None
                    }
                })
                .collect(),
        )
    }

    pub fn get(&self, index: u16) -> Option<&CharacterAssets> {
        self.0.get(usize::from(index)).and_then(Option::as_ref)
    }

    /// Slots, resolved or not — what a seat's character pick ranges over.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A person: the root of a character scene instance, waiting to be dressed
/// or dressed. The transform is the caller's — a crowd stands in its tile's
/// frame, a passenger in its vehicle's model space, the walker in render space.
#[derive(Component, Clone)]
pub struct Person {
    /// The glTF the scene came out of — its clips are the animation graph.
    pub gltf: Handle<Gltf>,
    pub pose: Pose,
    /// Where in a looping clip the person starts, 0..1.
    pub phase: f32,
    /// Distance past which the person is not drawn [m].
    pub cull: f32,
}

/// The scene root and the person component, ready to be spawned with a
/// transform and whatever markers the caller wants on it — the one way every
/// person is made, on a platform, in a seat or under the walker.
pub fn person_bundle(
    character: &CharacterAssets,
    pose: Pose,
    phase: f32,
    cull: f32,
) -> impl Bundle {
    (
        WorldAssetRoot(character.scene.clone()),
        Person {
            gltf: character.gltf.clone(),
            pose,
            phase,
            cull,
        },
    )
}

/// The instance has been finished: bands set, clip started. Carries the
/// entity the [`AnimationPlayer`] sits on, so whoever animates the person
/// further (the walker) finds it without walking the hierarchy.
#[derive(Component)]
pub struct Dressed {
    pub player: Option<Entity>,
}

/// The animation graph of one character glTF: every clip as a node under the
/// root, with its name and its duration.
pub struct CharacterGraph {
    pub handle: Handle<AnimationGraph>,
    /// Sorted by name, so the fallback to "any clip" is the same every time.
    clips: Vec<(Box<str>, AnimationNodeIndex, f32)>,
}

impl CharacterGraph {
    /// The node of the clip `name` and the clip's duration [s].
    pub fn clip(&self, name: &str) -> Option<(AnimationNodeIndex, f32)> {
        self.clips
            .iter()
            .find(|(clip, _, _)| &**clip == name)
            .map(|(_, node, duration)| (*node, *duration))
    }

    /// What plays for a pose: its own clip, else the first standing clip the
    /// file has, else any clip at all. `None` for a file without clips.
    fn pick(&self, pose: Pose) -> Option<(&str, AnimationNodeIndex, f32)> {
        std::iter::once(pose.clip())
            .chain(FALLBACK_CLIPS.iter().copied())
            .find_map(|name| {
                self.clip(name)
                    .map(|(node, duration)| (name, node, duration))
            })
            .or_else(|| {
                self.clips
                    .first()
                    .map(|(name, node, duration)| (&**name, *node, *duration))
            })
    }
}

/// Clips tried in this order when the pose's own is missing.
const FALLBACK_CLIPS: [&str; 6] = ["idle", "idle2", "stand", "stand2", "stand3", "sit"];

/// Whether a clip of this name runs on, or is a held frame.
fn looping(clip: &str) -> bool {
    matches!(clip, "idle" | "idle2" | "walk")
}

/// Animation graphs by character glTF, built once each. `None` is a glTF
/// that failed to load — its people stand in their rest pose.
#[derive(Resource, Default)]
pub struct CharacterGraphs {
    resolved: HashMap<AssetId<Gltf>, Option<Arc<CharacterGraph>>>,
}

/// What [`CharacterGraphs::resolve`] found.
enum Resolved {
    /// The graph, or `None` for a file that has none to offer.
    Ready(Option<Arc<CharacterGraph>>),
    /// The glTF is still loading.
    Pending,
}

impl CharacterGraphs {
    /// The graph of a dressed character — for whoever plays a clip of their
    /// own on it.
    pub fn get(&self, id: AssetId<Gltf>) -> Option<&CharacterGraph> {
        self.resolved.get(&id).and_then(|g| g.as_deref())
    }

    fn resolve(
        &mut self,
        handle: &Handle<Gltf>,
        assets: &AssetServer,
        gltfs: &Assets<Gltf>,
        clips: &Assets<AnimationClip>,
        graphs: &mut Assets<AnimationGraph>,
    ) -> Resolved {
        if let Some(graph) = self.resolved.get(&handle.id()) {
            return Resolved::Ready(graph.clone());
        }
        let Some(gltf) = gltfs.get(handle) else {
            return match assets.get_load_state(handle.id()) {
                Some(LoadState::Failed(_)) => {
                    warn!(
                        "character {:?} failed to load — not animated",
                        handle.path()
                    );
                    self.resolved.insert(handle.id(), None);
                    Resolved::Ready(None)
                }
                _ => Resolved::Pending,
            };
        };
        let mut graph = AnimationGraph::new();
        let root = graph.root;
        let mut named: Vec<(Box<str>, AnimationNodeIndex, f32)> = gltf
            .named_animations
            .iter()
            .map(|(name, clip)| {
                let node = graph.add_clip(clip.clone(), 1.0, root);
                let duration = clips.get(clip).map(AnimationClip::duration).unwrap_or(0.0);
                (name.clone(), node, duration)
            })
            .collect();
        named.sort_by(|a, b| a.0.cmp(&b.0));
        let names: Vec<&str> = named.iter().map(|(n, _, _)| &**n).collect();
        info!(
            "character {:?}: {} clips ({})",
            handle.path().map(|p| p.to_string()).unwrap_or_default(),
            named.len(),
            names.join(", ")
        );
        let resolved = (!named.is_empty()).then(|| {
            Arc::new(CharacterGraph {
                handle: graphs.add(graph),
                clips: named,
            })
        });
        self.resolved.insert(handle.id(), resolved.clone());
        Resolved::Ready(resolved)
    }
}

/// Which of the characters' atlases have their mip chain, and which are
/// still waited for.
#[derive(Resource, Default)]
pub struct PeopleTextures {
    done: HashSet<AssetId<Image>>,
    pending: Vec<PendingTexture>,
}

struct PendingTexture {
    image: Handle<Image>,
    /// The material that samples it, touched once the chain is built so its
    /// bind group is made anew with the new texture.
    material: Handle<StandardMaterial>,
    frames: u32,
}

impl PeopleTextures {
    fn enqueue(&mut self, image: &Handle<Image>, material: &Handle<StandardMaterial>) {
        if self.done.contains(&image.id())
            || self.pending.iter().any(|p| p.image.id() == image.id())
        {
            return;
        }
        self.pending.push(PendingTexture {
            image: image.clone(),
            material: material.clone(),
            frames: 0,
        });
    }
}

/// Finishes every person whose scene hierarchy has appeared: LOD bands on the
/// meshes, the animation graph on the player and the pose's clip started, and
/// the atlases queued for their mip chain.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn dress_people(
    mut commands: Commands,
    fresh: Query<(Entity, &Person, &WorldAssetRoot), Without<Dressed>>,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>, With<Mesh3d>>,
    mut players: Query<&mut AnimationPlayer>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    materials: Res<Assets<StandardMaterial>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut cache: ResMut<CharacterGraphs>,
    mut textures: ResMut<PeopleTextures>,
) {
    for (root, person, scene) in &fresh {
        // The scene spawns some frames after the entity; until then there is
        // nothing to dress. A scene that will never come is finished as it is.
        let Ok(kids) = children.get(root) else {
            if matches!(
                assets.get_load_state(scene.0.id()),
                Some(LoadState::Failed(_))
            ) {
                warn!("person: scene {:?} failed to load", scene.0.path());
                commands.entity(root).insert(Dressed { player: None });
            }
            continue;
        };
        let graph = match cache.resolve(&person.gltf, &assets, &gltfs, &clips, &mut graphs) {
            Resolved::Ready(graph) => graph,
            Resolved::Pending => continue,
        };

        // Walk the hierarchy once: which meshes sit below which `_LOD<n>` node,
        // where the player is, and which materials the meshes wear.
        let mut stack: Vec<(Entity, Option<u8>)> = kids.iter().map(|e| (e, None)).collect();
        let mut lod_meshes: Vec<(Entity, Option<u8>)> = Vec::new();
        let mut worn: Vec<Handle<StandardMaterial>> = Vec::new();
        let mut player = None;
        while let Some((entity, inherited)) = stack.pop() {
            let level = names
                .get(entity)
                .ok()
                .and_then(|name| lod_level(name.as_str()))
                .or(inherited);
            if players.contains(entity) {
                player = Some(entity);
            }
            if let Ok(material) = meshes.get(entity) {
                lod_meshes.push((entity, level));
                if !worn.iter().any(|m| m.id() == material.id()) {
                    worn.push(material.0.clone());
                }
            }
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter().map(|e| (e, level)));
            }
        }

        // Bands: the levels the model has, in order; a mesh outside any level
        // (or a model without levels) is drawn up to the cull distance.
        let mut levels: Vec<u8> = lod_meshes.iter().filter_map(|(_, l)| *l).collect();
        levels.sort_unstable();
        levels.dedup();
        for (entity, level) in &lod_meshes {
            let (start, end) = match level.and_then(|l| levels.iter().position(|x| *x == l)) {
                Some(rank) => lod_band(rank, levels.len(), person.cull),
                None => (0.0, person.cull),
            };
            commands
                .entity(*entity)
                .insert(VisibilityRange::abrupt(start, end));
        }

        // The clip: started at the person's phase, or held on its frame.
        let mut played = None;
        if let (Some(entity), Some(graph)) = (player, graph.as_deref())
            && let Ok(mut player) = players.get_mut(entity)
            && let Some((name, node, duration)) = graph.pick(person.pose)
        {
            let mut transitions = AnimationTransitions::new();
            let active = transitions.play(&mut player, node, Duration::ZERO);
            if looping(name) {
                active
                    .set_repeat(RepeatAnimation::Forever)
                    .set_speed(clip_speed(person.phase))
                    .seek_to(person.phase * duration);
            } else {
                active.pause();
            }
            commands
                .entity(entity)
                .insert((AnimationGraphHandle(graph.handle.clone()), transitions));
            played = Some(name);
        }
        debug!(
            "person {root}: {} meshes on {} levels, clip {:?} for {:?}",
            lod_meshes.len(),
            levels.len(),
            played,
            person.pose
        );

        for material in &worn {
            let Some(material_asset) = materials.get(material) else {
                continue;
            };
            for image in textures_of(material_asset) {
                textures.enqueue(image, material);
            }
        }
        commands.entity(root).insert(Dressed { player });
    }
}

/// The band of the level of rank `rank` out of `count` levels a model has:
/// the first from the camera, the last to the cull distance.
fn lod_band(rank: usize, count: usize, cull: f32) -> (f32, f32) {
    let last = PERSON_LOD_BANDS.len() - 1;
    let start = if rank == 0 {
        0.0
    } else {
        PERSON_LOD_BANDS[(rank - 1).min(last)].min(cull)
    };
    let end = if rank + 1 >= count {
        cull
    } else {
        PERSON_LOD_BANDS[rank.min(last)].min(cull)
    };
    (start, end.max(start))
}

/// Playback speed of a looping clip for a person's phase: spread around 1 so a
/// crowd does not move in step.
fn clip_speed(phase: f32) -> f32 {
    1.0 + IDLE_SPEED_SPREAD * (2.0 * phase.clamp(0.0, 1.0) - 1.0)
}

/// Every texture a material samples.
fn textures_of(material: &StandardMaterial) -> impl Iterator<Item = &Handle<Image>> {
    [
        &material.base_color_texture,
        &material.emissive_texture,
        &material.metallic_roughness_texture,
        &material.normal_map_texture,
        &material.occlusion_texture,
    ]
    .into_iter()
    .flatten()
}

/// Builds the mip chain of every character atlas that has arrived. The
/// pipeline writes plain PNG and JPEG, which the loader takes as one level; a
/// person twenty metres away then samples a 2048² atlas at one texel per
/// pixel and shimmers with every step. Cheap when nothing is waiting.
pub fn mip_people_textures(
    mut textures: ResMut<PeopleTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if textures.pending.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut textures.pending);
    for mut entry in pending {
        let Some(mut image) = images.get_mut(&entry.image) else {
            entry.frames += 1;
            if entry.frames < TEXTURE_PATIENCE {
                textures.pending.push(entry);
            } else {
                debug!("person texture {:?} never arrived", entry.image.path());
            }
            continue;
        };
        if build_mip_chain(&mut image) {
            // The material's bind group holds the old texture view; a touch
            // has it prepared anew with the one that carries the chain.
            materials.get_mut(&entry.material);
        }
        textures.done.insert(entry.image.id());
    }
}

/// Appends the mip chain to a plain RGBA8 image and sets a sampler that uses
/// it. `false` where the image is not one to touch: compressed, layered, or
/// already carrying a chain.
fn build_mip_chain(image: &mut Image) -> bool {
    let descriptor = &image.texture_descriptor;
    let plain = descriptor.mip_level_count <= 1
        && descriptor.dimension == TextureDimension::D2
        && descriptor.size.depth_or_array_layers == 1
        && matches!(
            descriptor.format,
            TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
        );
    if !plain {
        return false;
    }
    let (width, height) = (descriptor.size.width, descriptor.size.height);
    let Some(data) = image.data.take() else {
        return false;
    };
    if data.len() != (width * height * 4) as usize {
        image.data = Some(data);
        return false;
    }
    let (data, levels) = with_mipmaps(data, width, height);
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    if let Some(view) = image.texture_view_descriptor.as_mut() {
        view.mip_level_count = None;
    }
    // The glTF's own wrap modes are kept; the filtering is what changes.
    let (address_u, address_v) = match &image.sampler {
        ImageSampler::Descriptor(sampler) => (sampler.address_mode_u, sampler.address_mode_v),
        _ => (ImageAddressMode::Repeat, ImageAddressMode::Repeat),
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address_u,
        address_mode_v: address_v,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: TEXTURE_ANISOTROPY,
        ..default()
    });
    true
}

/// SplitMix64 of three indices — the one hash every seat decision comes out
/// of, so every client fills the same seats with the same people.
fn seat_hash(train: usize, vehicle: usize, seat: usize, salt: u64) -> u64 {
    let mut z = (train as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((vehicle as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93))
        .wrapping_add((seat as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(salt);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The hash as a number in [0, 1).
fn unit(hash: u64) -> f32 {
    (hash >> 40) as f32 / (1u64 << 24) as f32
}

/// Whether somebody sits in this seat.
pub fn seat_taken(train: usize, vehicle: usize, seat: usize) -> bool {
    unit(seat_hash(train, vehicle, seat, 1)) < SEAT_TAKEN_SHARE
}

/// Which of `count` characters sits there; `None` with nobody to choose from.
pub fn seat_character(train: usize, vehicle: usize, seat: usize, count: usize) -> Option<usize> {
    (count > 0).then(|| (unit(seat_hash(train, vehicle, seat, 2)) * count as f32) as usize % count)
}

/// Where a seated person goes in the vehicle's model space: the seat's floor
/// point, turned by its yaw (clockwise seen from above, 0 = ahead).
pub fn seat_transform(seat: &SeatSpec) -> Transform {
    Transform::from_translation(Vec3::from(seat.pos))
        .with_rotation(Quat::from_rotation_y(-seat.yaw_deg.to_radians()))
}

/// Fills a vehicle's seats: every taken seat gets its hashed character in the
/// `sit` pose, as children of the vehicle's view entity.
pub fn spawn_seated(
    parent: &mut ChildSpawnerCommands,
    passengers: &Passengers,
    seats: &[SeatSpec],
    train: usize,
    vehicle: usize,
) -> usize {
    let mut seated = 0;
    for (index, seat) in seats.iter().enumerate() {
        if !seat_taken(train, vehicle, index) {
            continue;
        }
        let Some(character) = seat_character(train, vehicle, index, passengers.len())
            .and_then(|i| passengers.get(i as u16))
        else {
            continue;
        };
        let phase = unit(seat_hash(train, vehicle, index, 3));
        parent.spawn((
            person_bundle(character, Pose::Sit, phase, PASSENGER_CULL),
            seat_transform(seat),
        ));
        seated += 1;
    }
    seated
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four levels hand over at the bands and the last runs to the cull
    /// distance; a single level covers everything; a lower cull cuts the
    /// bands short rather than leaving one that starts past it.
    #[test]
    fn lod_bands_butt_together_and_end_at_the_cull() {
        let four: Vec<(f32, f32)> = (0..4).map(|i| lod_band(i, 4, PERSON_CULL)).collect();
        assert_eq!(four[0], (0.0, 30.0));
        assert_eq!(four[3], (200.0, PERSON_CULL));
        for pair in four.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "levels butt together");
        }
        assert_eq!(lod_band(0, 1, PERSON_CULL), (0.0, PERSON_CULL));
        assert_eq!(lod_band(1, 2, PASSENGER_CULL), (30.0, PASSENGER_CULL));
        let (start, end) = lod_band(3, 4, 100.0);
        assert!(start <= end && end == 100.0);
    }

    /// The crowd's clip speeds spread around one, so it does not move in step.
    #[test]
    fn clip_speed_spreads_around_one() {
        assert!((clip_speed(0.0) - 0.9).abs() < 1e-6);
        assert!((clip_speed(0.5) - 1.0).abs() < 1e-6);
        assert!((clip_speed(1.0) - 1.1).abs() < 1e-6);
    }

    /// A seat is taken or not by its indices alone, about two thirds of them
    /// are, and the character is a pick over what is installed.
    #[test]
    fn seats_are_filled_by_hash() {
        assert_eq!(seat_taken(0, 1, 2), seat_taken(0, 1, 2));
        let taken = (0..1000usize)
            .filter(|&seat| seat_taken(seat / 100, seat / 10 % 10, seat % 10))
            .count();
        assert!((600..=700).contains(&taken), "{taken} of 1000 taken");
        assert_eq!(seat_character(0, 0, 0, 0), None);
        let mut picks = HashSet::new();
        for seat in 0..200 {
            let pick = seat_character(3, 1, seat, 24).unwrap();
            assert!(pick < 24);
            picks.insert(pick);
        }
        assert!(picks.len() > 12, "the picks spread over the roster");
        // Neighbouring seats do not share one fate.
        let row: Vec<bool> = (0..8).map(|seat| seat_taken(0, 0, seat)).collect();
        assert!(row.iter().any(|t| *t) && row.iter().any(|t| !*t));
    }

    /// A seat's yaw turns the person clockwise seen from above: 90° faces +X.
    #[test]
    fn a_seat_turns_its_person() {
        let seat = SeatSpec {
            pos: [0.5, 1.2, -3.0],
            yaw_deg: 90.0,
        };
        let transform = seat_transform(&seat);
        assert_eq!(transform.translation, Vec3::new(0.5, 1.2, -3.0));
        let facing = transform.rotation * Vec3::NEG_Z;
        assert!((facing - Vec3::X).length() < 1e-5, "{facing:?}");
    }

    /// The pose's own clip, then the standing fallbacks, then anything — and
    /// nothing for a file without clips.
    #[test]
    fn the_clip_pick_falls_back_without_panicking() {
        let graph = |names: &[&str]| CharacterGraph {
            handle: Handle::default(),
            clips: names
                .iter()
                .enumerate()
                .map(|(i, n)| ((*n).into(), AnimationNodeIndex::new(i + 1), 1.0))
                .collect(),
        };
        let full = graph(&["idle", "idle2", "sit", "stand", "walk"]);
        assert_eq!(full.pick(Pose::Sit).map(|p| p.0), Some("sit"));
        assert_eq!(full.pick(Pose::Stand2).map(|p| p.0), Some("idle"));
        let odd = graph(&["wave"]);
        assert_eq!(odd.pick(Pose::Idle).map(|p| p.0), Some("wave"));
        assert!(graph(&[]).pick(Pose::Idle).is_none());
        assert!(looping("walk") && !looping("sit"));
    }

    /// A plain image gets its chain and a mipmapped sampler; one that has a
    /// chain, or is not RGBA8, is left alone.
    #[test]
    fn a_mip_chain_is_built_once_for_rgba8() {
        use bevy::asset::RenderAssetUsages;
        use bevy::render::render_resource::Extent3d;
        let mut image = Image::new(
            Extent3d {
                width: 4,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![200; 4 * 2 * 4],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        assert!(build_mip_chain(&mut image));
        assert_eq!(image.texture_descriptor.mip_level_count, 3);
        assert_eq!(image.data.as_ref().map(Vec::len), Some((8 + 2 + 1) * 4));
        match &image.sampler {
            ImageSampler::Descriptor(sampler) => {
                assert_eq!(sampler.anisotropy_clamp, TEXTURE_ANISOTROPY);
                assert_eq!(sampler.mipmap_filter, ImageFilterMode::Linear);
            }
            other => panic!("{other:?}"),
        }
        assert!(!build_mip_chain(&mut image), "not twice");
        let mut float = Image::new(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![0; 2 * 2 * 8],
            TextureFormat::Rgba16Float,
            RenderAssetUsages::default(),
        );
        assert!(!build_mip_chain(&mut float));
        assert_eq!(float.texture_descriptor.mip_level_count, 1);
    }
}
