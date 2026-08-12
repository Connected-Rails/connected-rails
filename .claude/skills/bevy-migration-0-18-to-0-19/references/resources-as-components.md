# Resources as Components — 0.18 → 0.19

In 0.19 `Resource` became a **subtrait of `Component`**: a resource is stored as a
component on a singleton entity. The headline grep ("~50 files touch resources!")
overstates the impact — the working APIs are intact. Do **not** mass-rewrite resource code.

## Still works, unchanged

```rust
#[derive(Resource)] struct Score(u32);
app.insert_resource(Score(0));
fn read(score: Res<Score>) { /* … */ }
fn write(mut score: ResMut<Score>) { score.0 += 1; }
```

`Res<T>`, `ResMut<T>`, `#[derive(Resource)]`, `init_resource`, `insert_resource`,
`remove_resource` — all behave as before.

## Breakage 1 — no double derive

A single type can no longer be both a component and a resource:

```rust
#[derive(Component, Resource)] struct Health(u32);   // ❌ compile error in 0.19
```

Split into two types (e.g. `Health` component + `PlayerHealth` resource wrapper), or pick
the one role the type actually plays. (None of the bevy-skills snippets do this today —
grep `derive(Component, Resource)` / `derive(Resource, Component)` before bumping a project.)

## Breakage 2 — footgun: resources show up in component queries

Because a resource is now a component on a singleton entity, a type used as a resource
**also matches `Query<&T>`**. And inserting a resource type as a *component* onto an entity
can despawn other copies. Symptoms:

- A `Query<&MyResource>` that returned nothing in 0.18 now yields the resource's singleton.
- `commands.entity(e).insert(my_resource_typed_value)` has surprising despawn side effects.

If you query a type, don't also register it as a resource (and vice versa).

## Breakage 3 — reflection

`#[reflect(Resource)]` access paths now also need `ReflectComponent` registered in some
cases. If reflection-based resource access stops resolving, register `ReflectComponent`
for the type too.

## Renames — non-send resources (deprecated, not removed)

| 0.18 | 0.19 |
|---|---|
| `init_non_send_resource()` | `init_non_send()` |
| `insert_non_send_resource()` | `insert_non_send()` |
| `get_non_send_resource()` | `get_non_send()` |
| `get_non_send_resource_mut()` | `get_non_send_mut()` |
| `non_send_resource_mut()` | `non_send_mut()` |

The old names still compile (deprecation warnings) but should be updated.
