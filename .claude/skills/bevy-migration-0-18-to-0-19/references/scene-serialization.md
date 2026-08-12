# Scene system reorganization — 0.18 → 0.19

0.19 introduces **BSN** (Bevy's Next-Generation Scene system) and **reuses the
`bevy_scene` crate name for it.** The classic reflection-based serialization system was
renamed to **`bevy_world_serialization`**. This is a silent wrong-crate trap: code that
still imports `bevy_scene::*` / `bevy::scene::*` for `DynamicScene` round-tripping now
compiles against BSN types or fails to resolve, rather than doing the old thing.

## Crate / module renames

| 0.18 | 0.19 |
|---|---|
| `bevy_scene` (serialization) | `bevy_world_serialization` |
| `bevy::scene::*` | `bevy::world_serialization::*` |
| `bevy_scene` (the name) | now the **BSN** crate |

## Type renames (classic serialization)

| 0.18 | 0.19 |
|---|---|
| `Scene` | `WorldAsset` |
| `SceneRoot` | `WorldAssetRoot` |
| `DynamicScene` | `DynamicWorld` |
| `DynamicSceneBuilder` | `DynamicWorldBuilder` |
| `DynamicSceneRoot` | `DynamicWorldRoot` |
| `SceneSpawner` | `WorldInstanceSpawner` |

## What did NOT move

**glTF scene spawning** still uses the old (now `world_serialization`) system — loading
a `.gltf`/`.glb` and spawning its scene is unaffected beyond the type/crate renames above.

## Which one do I want?

- **Saving/loading world state to RON** (savegames, prefabs serialized at runtime) →
  `bevy_world_serialization` (`DynamicWorld`, `DynamicWorldBuilder`).
- **Authoring scenes / next-gen prefabs** → the new `bevy_scene` (BSN). New surface area,
  not a drop-in replacement for the old `DynamicScene` workflow.

When in doubt: if your 0.18 code said `DynamicScene`, you want `bevy_world_serialization`.
