# PMU (Pick Me Up) — Phase 8 (Part A) Architecture

## Module Map
- `src/main.rs`, `src/app.rs`, `src/input.rs`, `src/camera.rs`: Entry point, window/mouse lifecycle, Q/F/X/V/T/G/E/1/2/3/R/F1–F8 hotkeys, and third-person camera.
- `src/collision.rs`, `src/nav.rs`: 4m spatial collision grid (`CollisionGrid`) and 60x60 A* navigation grid (`NavGrid`, `NavSearchScratch`).
- `src/combat.rs`, `src/status.rs`: Shared `Faction`/`ActorId`/`CombatantInfo`, `PlayerCombat`, `FloatingNumberPool`, and `StatusEffects` (Burn 4 DPS & Slow 0.5x).
- `src/skills.rs`, `src/skill_inv.rs`, `src/projectiles.rs`, `src/pickups.rs`: Data-driven `SKILL_TABLE` (9 skills, Tier 1 & Tier 2), mana (100 max, 6/s regen, 1s pause), 4 slots + `T`-key fusion (`FUSION_RECIPES`), sub-stepped `ProjectileManager` (32 projectiles, 32 flashes, meteors, slow zones), and `PickupManager` (16 orbs, 25% drop + guaranteed Wave 1 drop).
- `src/ally.rs`: `Ally`, `Rarity` gacha roll (70% Common, 25% Rare, 5% Epic), rarity stats/tints/name pools, `AllyState`, spawn ease-out + flash, and unit tests.
- `src/squad.rs`: `Squad`, `SquadMode` (`Follow`, `Hold`, `Aggressive`), `AstarBudgetScheduler` (round-robin 3/step), formation slots, sticky attacker-preferring target selection, E-key/wave-start revive, regroup teleport, and unit tests.
- `src/enemy.rs`, `src/ai.rs`, `src/waves.rs`: `Enemy`, multi-target AI with 3.0s threat memory, and `WaveManager` (`3 + 2*(wave-1) + living_allies`).
- `src/animation.rs`, `src/player_anim.rs`, `src/model.rs`, `src/gpu_model.rs`: glTF loader (`player.glb`, `ally.glb`, `enemy.glb`, `prop.glb`) and dynamic-offset GPU skinning (`MAX_SKINNED_INSTANCES = 24`).
- `src/hud.rs`, `src/hud_squad.rs`, `src/font.rs`, `src/debug_lines.rs`, `src/shadow.rs`, `src/sky.rs`, `src/post.rs`, `src/stats.rs`, `src/renderer.rs`: Squad panel, 128x128 rotating radar, status banner, F7/F8 debug lines, and rendering passes.

## Per-Frame Data Flow
1. **Input**: Window/device events update `Input` (`G` summon, `E` revive, `1`/`2`/`3` squad mode, `C` dodge, `R` reset, `Left Click` attack).
2. **Fixed Update (60 Hz)**: `World::update` advances player movement/combat, builds a shared `CombatantInfo` snapshot, schedules up to 3 A* searches round-robin via `AstarBudgetScheduler`, updates `Squad` (formation follow, target selection, attacks, E-key revive) and `Enemy` AI (multi-target sight/threat memory, attacks), resolves combat hits, and updates `WaveManager` (auto-reviving Down allies at wave start).
3. **Animation & Render**: `World::update_animation` steps player, ally, and enemy skeletons. `Renderer::render` uploads up to 24 joint palettes, culls instances, and executes Shadow -> Main+Sky+DebugLines -> Post Upscale -> HUD+Radar passes.

## Ally AI State Machine & Squad Commands
| State | Entry Condition | Behavior & Speed | Exit Transition |
| :--- | :--- | :--- | :--- |
| `Follow` | Spawn / No target | Moves to rotating formation slot (`r=3.0m`) or `hold_pos`; walks `4.0`, runs `7.0` (`>6m`) or `8.0` (`>10m`) | Enemy in aggro range -> `Engage`; `>22m` or stuck `>2s` -> `Regroup` |
| `Engage` | Target acquired | Pursues target (prefers attackers, 1.5s stickiness) via LOS or A* at `7.0 * mult` | `<=1.8m` + LOS -> `Attack`; target lost -> `Follow` |
| `Attack` | Within `1.8m` + LOS | Faces target, `0.30s` windup (`run` 1.6x + lunge), deals rarity damage (`8`/`12`/`20`) | Windup ends -> `Recover` (`0.90s`/`0.75s`/`0.60s`) |
| `Recover` | Attack finished | Cooldown pause (`idle`) | Cooldown ends -> `Engage` or `Follow` |
| `Regroup` | `>22m` from player or stuck `>2s` | Runs toward slot at `1.5x` speed; teleports to slot with flash if still `>12m` after `6s` | Within `12m` -> `Follow` |
| `Down` | `health <= 0` | Falls `90 deg` (`0.5s`), gray tint, untargetable; hold `E` `2.0s` within `2.5m` or new wave -> `50%` HP | Revived -> `Follow` |

- **Squad Commands**: `1 = Follow` (`14m` aggro around player/ally), `2 = Hold` (locks `hold_pos`, defends within `6m` and returns), `3 = Aggressive` (`30m` aggro, ignores slots while targets exist).

## Key Constants & Locations
- `src/ally.rs` & `src/squad.rs`: `MAX_ALLIES` (`4`), `SUMMON_COST` (`30`), `FOLLOW_DISTANCE` (`3.0`), `SLOT_STOP_RADIUS` (`0.6`), `ALLY_AGGRO_RANGE` (`14.0`), `AGGRESSIVE_AGGRO_RANGE` (`30.0`), `HOLD_DEFEND_RANGE` (`6.0`), `ALLY_ATTACK_RANGE` (`1.8`), `ALLY_ATTACK_WINDUP` (`0.3s`), `REGROUP_DISTANCE` (`22.0`), `STUCK_TIME_THRESHOLD` (`2.0s`), `REGROUP_TELEPORT_TIME` (`6.0s`), `REVIVE_HOLD_TIME` (`2.0s`), `REVIVE_RANGE` (`2.5`), `MAX_SHARED_ASTAR_BUDGET` (`3`).
- `src/gpu_model.rs` & `src/hud_squad.rs`: `MAX_SKINNED_INSTANCES` (`24`), `RADAR_SIZE_PX` (`128`), `RADAR_RANGE` (`40.0`).

## Skill Table
| Skill | Tier | Mana | CD | Behavior |
| :--- | :--- | :--- | :--- | :--- |
| `Fireball` | 1 | 20 | 1.0s | Speed 20, dmg 28, AoE r=2.0, Burn (4 DPS, 3s) |
| `Ice Shard` | 1 | 15 | 0.8s | Speed 26, dmg 18, pierces 1, Slow (0.5x, 3s) |
| `Heal Pulse` | 1 | 30 | 6.0s | Instant 30 HP heal to player & living allies within 8m |
| `Shockwave` | 1 | 25 | 4.0s | Instant AoE r=5.0, dmg 20, knockback 8 |
| `Wind Dash` | 1 | 10 | 3.0s | 6m dash in 0.2s along aim, i-frames, stops at cubes |
| `Steam Burst` | 2 (`Fireball`+`Ice Shard`) | 40 | 3.0s | Speed 16, dmg 55, AoE r=4.0, Slow |
| `Meteor Slam` | 2 (`Fireball`+`Shockwave`) | 45 | 6.0s | Marks 10m ahead, 0.6s delay, r=6.0, dmg 70, kb 10, Burn |
| `Frost Dash` | 2 (`Ice Shard`+`Wind Dash`) | 25 | 4.0s | 8m dash in 0.2s, leaves 2s Slow zones (r=2.5), i-frames |
| `Holy Nova` | 2 (`Heal Pulse`+`Shockwave`) | 50 | 8.0s | Heals allies within 8m by 40; damages enemies within 7m by 30 (kb 6) |

## Controls Table
| Input | Action |
| :--- | :--- |
| `Left Click` / `C` / `Space` / `Shift` | Melee Attack / Dodge Roll / Jump / Sprint |
| `Q` / `F` / `X` / `V` | Cast Skill Slots 1–4 (requires mana, cooldown ready, mouse captured) |
| `T` | Fuse first available Tier 1 pair (`count >= 2` each) into Tier 2 skill |
| `G` / `E` (hold) | Summon Ally (`30` score, gacha roll) / Revive Down Ally (`2.0s` within `2.5m`) |
| `1` / `2` / `3` | Squad Command: `1 = Follow`, `2 = Hold`, `3 = Aggressive` |
| `R` / `F1`..`F8` / `Escape` | Reset Game / Benchmark & Debug Overlays (`F8` includes Ally AI) / Unlock or Quit |

## Planned Next Phases
1. **Skills and Abilities**: Mana resource, projectile fireball casting, and skill combining.
2. **Items and Inventory**: Loot drops, equipment stats, and consumable pickups.
3. **Save/Load and Menus**: Persistent squad/wave progression, pause menu, and settings screen.
4. **Audio**: Spatial 3D sound effects for summons, attacks, footsteps, and UI.
