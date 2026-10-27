# PMU (Pick Me Up) — Phase 10 Architecture

## Module Map
- `src/main.rs`, `src/app.rs`, `src/game_flow.rs`, `src/input.rs`, `src/camera.rs`: Entry point, `GameFlow` (`MainMenu`, `Playing`, `PauseMenu`, `SkillMenu`, `SquadScreen`, `LoginReport`, `GameOver`), mouse/keyboard routing, and camera.
- `src/traits.rs`, `src/life.rs`, `src/camp.rs`, `src/training.rs`, `src/fairy.rs`, `src/offline.rs`: Ally life-sim layer: Fighter/Archer classes, personality traits, levels 1–10 & passive skills (`Basic Sword-Shield` combining, `Pain Tolerance`), hunger/fear/fatigue & `Flee`/`Freeze`, cube-free Camp (`-20, 0, -20`) with 5 objects & 45s Camp phase, training/sparring/quarrels, Fairy intervention, and offline Login Progress Report (capped at 8h in 10m ticks).
- `src/save.rs`, `src/settings.rs`: Save v2 (with v1 backward compatibility) and `settings.cfg` persistence.
- `src/menu.rs`, `src/hud_menu.rs`, `src/hud_life.rs`, `src/hud_squad.rs`, `src/hud.rs`: Menus, `SquadScreen` (`H`), `LoginReport` (`WELCOME BACK`), camp banner (`N: skip`), and world-space activity icons.
- `src/collision.rs`, `src/nav.rs`, `src/combat.rs`, `src/status.rs`: 4m spatial collision grid, 60x60 A* navigation grid, player combat/dodge, and Burn/Slow status effects.
- `src/skills.rs`, `src/skill_inv.rs`, `src/skill_menu.rs`, `src/projectiles.rs`, `src/pickups.rs`, `src/effects.rs`: 9-skill table, inventory/fusion, K-key grimoire, projectiles, skill orbs, and 64-entry priority `EffectsPool`.
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

## Pause Flow, Ally Skills & Effects Pool
- **GameState & Pause**: `K` toggles `MenuState` and `GameState::Paused`. While paused, `World::step_if_unpaused`, animations, and mouse camera look are frozen while rendering, HUD, and menu navigation continue. `Escape` closes the menu first before unlocking the cursor or exiting.
- **Ally Skill Rules**: In `Engage`/`Attack`, Common allies never cast; Rare allies cast `Fireball` (L1, 6.0s CD, 4–14m range + LOS); Epic allies prioritize `Heal Pulse` (20 HP, 10.0s CD, when any living squad member is `<50%` HP) and cast `Fireball` (5.0s CD, 4–14m + LOS), pausing `0.3s` in `Recover`.
- **Effects Pool (`src/effects.rs`)**: 64-entry pool with priority eviction (`PickupSparkle`/`HealParticles` = 1, `ExpandingRing` = 2, `MeteorTelegraph` = 3) rendered via `TOTAL_CUBE_INSTANCES = 1024` guarded cube buffer.

## Save Format, Files & Settings
- **Save Files (`saves/slot1..3.sav`, `autosave.sav`, `quicksave.sav`)**: Header `PMU_SAVE`, `version=1`, `created_unix`, `playtime_seconds`, `player.{x,y,z,yaw,health,mana,total_kills}`, `progress.{wave,phase,intermission_timer,pending_spawns,score}`, `skill.0..8.count`, `slot.0..3`, `squad.mode`, `ally.count` + `ally.N.{rarity,name_index,health,down,x,y,z,hold_x,hold_y,hold_z}`, `enemy.count` + `enemy.N.{x,y,z,health}`, `rng.{wave,squad,pickup,first_wave_drop}`, and footer `checksum=<16 hex FNV-1a>`.
- **Settings (`settings.cfg`)**: `mouse_sensitivity` (`0.2..3.0`), `fov_degrees` (`50..100`), `shadow_quality` (`Off`/`1024`/`2048`), `render_scale` (`0.5..1.0`), `present_mode` (`Fifo`/`Fast`), `hud_scale` (`1..3`), `show_title_stats`, `master_volume`, `music_volume`.

## GameFlow & Menu Screens Table
| GameFlow / Screen | World Update | Camera | Escape Action | Primary Actions |
| :--- | :--- | :--- | :--- | :--- |
| `MainMenu` (`Main`) | Frozen | Orbiting (`r=30, h=14`) | Pops sub-screen (never quits) | `Continue`, `NewGame`, `OpenLoad`, `OpenSettings`, `OpenControls`, `Quit` |
| `Playing` | Active (60 Hz) | Player Orbit | Opens `PauseMenu` (unlocks mouse) | Movement, Combat, Skills (`Q/F/X/V`), Summon (`G`), Quick Save/Load (`F9/F10`) |
| `PauseMenu` (`Pause`) | Frozen | Frozen | Pops sub-screen or resumes `Playing` | `ResumeGame`, `OpenSave`, `OpenLoad`, `OpenSettings`, `OpenControls`, `BackToMainMenu` (autosaves), `Quit` (autosaves) |
| `SkillMenu` | Frozen | Frozen | Closes Grimoire -> `Playing` | Assign slots `1..4`, Clear (`Backspace`), Fuse (`Enter`/`T`) |
| `GameOver` | Frozen | Frozen | Pops sub-screen | `Retry`, `LoadLast`, `BackToMainMenu` |

## Camp Activities, Traits, Passive Skills & Save v2 Keys
- **Camp Zone & Phase**: Cube-free `12x12m` square at `(-20, 0, -20)` with Dummy (cap 2), Archery Target (cap 2), Food Pot (cap 3), Rest Mat (cap 2), and Flag Post (cap 2). Clearing a wave starts a `45s` Camp phase (autosaved at start; `N` skips to next wave).
- **Camp Activities**: `Eat` (`4s`, `-60` hunger, `-1` food), `Rest` (`12s`, `+6% HP/s`, `-8 fatigue/s`, tracks `Pain Tolerance`), `Train` (`10s`, `+25` fatigue, 15/70/15 roll for 0x/1x/2x XP + passive progress, 1% death risk on great roll if `<70%` HP or `>70` fatigue), `Spar` (`8s`, non-lethal hits, `+20` XP, 25% quarrel escalation when hungry/scared/hotheaded), `Idle` (`3s`).
- **Traits & Passives**: `Brave`/`Cautious` (fear threshold `80`/`45`, normal `60`), `Diligent`/`Lazy` (`+0.3`/`-0.3` train score, `+10` XP), `Hotheaded`/`Calm` (`+0.25`/`-0.15` spar score), `HungryType`/`LightEater` (`1.3x`/`0.7x` hunger rate). Passives (`Lv1..5`): `Basic Swordsmanship` (`+6%` dmg), `Basic Archery` (`+6%` dmg), `Basic Shield` (`-4%` dmg taken), `Pain Tolerance` (`+6%` max HP, 1 pt per 150 dmg healed by resting), and `Basic Sword-Shield` (combines Sword Lv2 + Shield Lv2 into Lv3, `+5%` dmg & `-4%` dmg taken/lvl).
- **Save v2 Keys**: Adds `squad.food` and `ally.N.{class,trait0,trait1,level,xp,hunger,fear,fatigue,passive.0..4.level,passive.0..4.prog,pain.dmg,pain.healed,pain.points}`, plus `progress.phase=Camp`.

## Controls Table
| Input | Action |
| :--- | :--- |
| `Left Click` / `C` / `Space` / `Shift` | Melee Attack (or Menu Click) / Dodge Roll / Jump / Sprint |
| `Q` / `F` / `X` / `V` / `T` / `K` | Cast Skill Slots 1–4 / Quick-Fuse / Skill Grimoire Menu |
| `H` / `N` | Open/Close Squad Life & Training Screen / Skip 45s Camp Phase |
| `G` / `E` (hold) / `1`..`3` | Summon Ally (`30` score) / Revive Down Ally / Squad Mode (`Follow`/`Hold`/`Aggressive`) |
| `F1`..`F8` / `F9` / `F10` / `R` / `Delete` / `Escape` | Benchmark & Debug Tools / Quicksave / Quickload / Restart / Delete Save Slot / Pause or Back |

</USER_REQUEST>
<ADDITIONAL_METADATA>
The current local time is: 2026-10-27T12:31:53+03:00.
</ADDITIONAL_METADATA>

## Planned Next Phases
1. **Skills and Abilities**: Mana resource, projectile fireball casting, and skill combining.
2. **Items and Inventory**: Loot drops, equipment stats, and consumable pickups.
3. **Save/Load and Menus**: Persistent squad/wave progression, pause menu, and settings screen.
4. **Audio**: Spatial 3D sound effects for summons, attacks, footsteps, and UI.
