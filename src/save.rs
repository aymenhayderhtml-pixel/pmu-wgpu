use crate::ally::{Rarity, ALLY_HEIGHT, ALLY_RADIUS, MAX_ALLIES};
use crate::collision::CollisionGrid;
use crate::combat::PLAYER_MAX_HEALTH;
use crate::enemy::{ENEMY_HEIGHT, ENEMY_MAX_HEALTH, ENEMY_RADIUS};
use crate::skill_inv::{MAX_SKILL_COPIES, SLOT_COUNT};
use crate::skills::{SkillId, PLAYER_MAX_MANA, SKILL_COUNT, SKILL_TABLE};
use crate::squad::SquadMode;
use crate::waves::MAX_ALIVE_ENEMIES;
use crate::world::{GROUND_SIZE, PLAYER_HEIGHT, PLAYER_RADIUS};
use glam::Vec3;
use std::fmt::Write;
use std::io::Write as IoWrite;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SAVE_HEADER: &str = "PMU_SAVE";
pub const SAVE_VERSION: u32 = 1;
pub const SAVE_DIR_NAME: &str = "saves";

pub const SLOT_FILES: [&str; 5] = [
    "slot1.sav",
    "slot2.sav",
    "slot3.sav",
    "autosave.sav",
    "quicksave.sav",
];

pub const SLOT_TITLES: [&str; 5] = [
    "Slot 1",
    "Slot 2",
    "Slot 3",
    "Autosave",
    "Quicksave",
];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WavePhase {
    Active,
    Intermission,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedAlly {
    pub rarity: Rarity,
    pub name_index: usize,
    pub health: f32,
    pub down: bool,
    pub pos: Vec3,
    pub hold_pos: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedEnemy {
    pub pos: Vec3,
    pub health: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SaveData {
    pub version: u32,
    pub created_unix: u64,
    pub playtime_seconds: f64,
    pub player_pos: Vec3,
    pub player_yaw: f32,
    pub player_health: f32,
    pub player_mana: f32,
    pub total_kills: u32,
    pub wave_number: u32,
    pub wave_phase: WavePhase,
    pub intermission_timer: f32,
    pub pending_spawns: u32,
    pub score: u32,
    pub skill_counts: [u8; SKILL_COUNT],
    pub skill_slots: [Option<SkillId>; SLOT_COUNT],
    pub squad_mode: SquadMode,
    pub allies: Vec<SavedAlly>,
    pub enemies: Vec<SavedEnemy>,
    pub wave_rng_state: u64,
    pub squad_rng_state: u64,
    pub pickup_rng_state: u64,
    pub first_wave_drop_pending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    Io(String),
    BadHeader,
    BadVersion(u32),
    BadChecksum,
    Missing(String),
    Invalid(String),
}

impl SaveError {
    pub fn message(&self) -> String {
        match self {
            Self::Io(e) => format!("I/O error: {e}"),
            Self::BadHeader => "Invalid save header (expected PMU_SAVE)".to_string(),
            Self::BadVersion(v) => format!("Unsupported save version {v} (max supported is {SAVE_VERSION})"),
            Self::BadChecksum => "Save file checksum mismatch (corrupt data)".to_string(),
            Self::Missing(k) => format!("Missing required save key '{k}'"),
            Self::Invalid(k) => format!("Invalid value for save key '{k}'"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlotInfo {
    pub created_unix: u64,
    pub wave: u32,
    pub score: u32,
    pub playtime_seconds: f64,
    pub ally_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SlotStatus {
    Empty,
    Valid(SlotInfo),
    Corrupt,
}

pub fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// FNV-1a 64-bit hash over a byte slice.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Converts Unix timestamp in seconds to `"YYYY-MM-DD HH:MM"` in UTC using Howard Hinnant's civil-from-days algorithm.
pub fn format_utc_datetime(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }

    let mut out = String::with_capacity(16);
    let _ = write!(&mut out, "{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, hour, minute);
    out
}

/// Formats playtime in seconds as `"HH:MM:SS"`.
pub fn format_playtime_hms(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    let mut out = String::with_capacity(10);
    let _ = write!(&mut out, "{:02}:{:02}:{:02}", h, m, s);
    out
}

impl SaveData {
    pub fn to_text(&self) -> String {
        let mut body = String::with_capacity(2048);
        let _ = writeln!(&mut body, "{}", SAVE_HEADER);
        let _ = writeln!(&mut body, "version={}", self.version);
        let _ = writeln!(&mut body, "created_unix={}", self.created_unix);
        let _ = writeln!(&mut body, "playtime_seconds={}", self.playtime_seconds);

        let _ = writeln!(&mut body, "player.x={}", self.player_pos.x);
        let _ = writeln!(&mut body, "player.y={}", self.player_pos.y);
        let _ = writeln!(&mut body, "player.z={}", self.player_pos.z);
        let _ = writeln!(&mut body, "player.yaw={}", self.player_yaw);
        let _ = writeln!(&mut body, "player.health={}", self.player_health);
        let _ = writeln!(&mut body, "player.mana={}", self.player_mana);
        let _ = writeln!(&mut body, "player.total_kills={}", self.total_kills);

        let phase_str = match self.wave_phase {
            WavePhase::Active => "Active",
            WavePhase::Intermission => "Intermission",
        };
        let _ = writeln!(&mut body, "progress.wave={}", self.wave_number);
        let _ = writeln!(&mut body, "progress.phase={}", phase_str);
        let _ = writeln!(&mut body, "progress.intermission_timer={}", self.intermission_timer);
        let _ = writeln!(&mut body, "progress.pending_spawns={}", self.pending_spawns);
        let _ = writeln!(&mut body, "progress.score={}", self.score);

        for i in 0..SKILL_COUNT {
            let _ = writeln!(&mut body, "skill.{}.count={}", i, self.skill_counts[i]);
        }
        for i in 0..SLOT_COUNT {
            let code: i32 = self.skill_slots[i].map_or(-1, |s| s.index() as i32);
            let _ = writeln!(&mut body, "slot.{}={}", i, code);
        }

        let mode_id: u8 = match self.squad_mode {
            SquadMode::Follow => 0,
            SquadMode::Hold => 1,
            SquadMode::Aggressive => 2,
        };
        let _ = writeln!(&mut body, "squad.mode={}", mode_id);
        let _ = writeln!(&mut body, "ally.count={}", self.allies.len());
        for (i, a) in self.allies.iter().enumerate() {
            let r_id: u8 = match a.rarity {
                Rarity::Common => 0,
                Rarity::Rare => 1,
                Rarity::Epic => 2,
            };
            let _ = writeln!(&mut body, "ally.{}.rarity={}", i, r_id);
            let _ = writeln!(&mut body, "ally.{}.name_index={}", i, a.name_index);
            let _ = writeln!(&mut body, "ally.{}.health={}", i, a.health);
            let _ = writeln!(&mut body, "ally.{}.down={}", i, if a.down { 1 } else { 0 });
            let _ = writeln!(&mut body, "ally.{}.x={}", i, a.pos.x);
            let _ = writeln!(&mut body, "ally.{}.y={}", i, a.pos.y);
            let _ = writeln!(&mut body, "ally.{}.z={}", i, a.pos.z);
            let _ = writeln!(&mut body, "ally.{}.hold_x={}", i, a.hold_pos.x);
            let _ = writeln!(&mut body, "ally.{}.hold_y={}", i, a.hold_pos.y);
            let _ = writeln!(&mut body, "ally.{}.hold_z={}", i, a.hold_pos.z);
        }

        let _ = writeln!(&mut body, "enemy.count={}", self.enemies.len());
        for (i, e) in self.enemies.iter().enumerate() {
            let _ = writeln!(&mut body, "enemy.{}.x={}", i, e.pos.x);
            let _ = writeln!(&mut body, "enemy.{}.y={}", i, e.pos.y);
            let _ = writeln!(&mut body, "enemy.{}.z={}", i, e.pos.z);
            let _ = writeln!(&mut body, "enemy.{}.health={}", i, e.health);
        }

        let _ = writeln!(&mut body, "rng.wave={}", self.wave_rng_state);
        let _ = writeln!(&mut body, "rng.squad={}", self.squad_rng_state);
        let _ = writeln!(&mut body, "rng.pickup={}", self.pickup_rng_state);
        let _ = writeln!(
            &mut body,
            "rng.first_wave_drop={}",
            if self.first_wave_drop_pending { 1 } else { 0 }
        );

        let hash = fnv1a_64(body.as_bytes());
        let _ = writeln!(&mut body, "checksum={:016x}", hash);
        body
    }

    pub fn from_text(input: &str) -> Result<Self, SaveError> {
        let trimmed = input.trim_end_matches(&['\r', '\n'][..]);
        let Some(first_nl) = trimmed.find('\n') else {
            return Err(SaveError::BadHeader);
        };
        let first_line = trimmed[..first_nl].trim_end_matches('\r');
        if first_line != SAVE_HEADER {
            return Err(SaveError::BadHeader);
        }

        let Some(last_nl) = trimmed.rfind('\n') else {
            return Err(SaveError::BadChecksum);
        };
        let last_line = trimmed[last_nl + 1..].trim();
        let body_with_newline = &trimmed[..=last_nl];

        let Some(hex_str) = last_line.strip_prefix("checksum=") else {
            return Err(SaveError::BadChecksum);
        };
        if hex_str.len() != 16 {
            return Err(SaveError::BadChecksum);
        }
        let expected_hash =
            u64::from_str_radix(hex_str, 16).map_err(|_| SaveError::BadChecksum)?;
        let actual_hash = fnv1a_64(body_with_newline.as_bytes());
        if expected_hash != actual_hash {
            return Err(SaveError::BadChecksum);
        }

        let mut pairs: Vec<(&str, &str)> = Vec::with_capacity(128);
        for line_raw in body_with_newline[first_nl + 1..].lines() {
            let line = line_raw.trim();
            if line.is_empty() {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(SaveError::Invalid(line.to_string()));
            };
            pairs.push((k.trim(), v.trim()));
        }

        let find_val = |key: &str| -> Result<&str, SaveError> {
            pairs
                .iter()
                .rev()
                .find(|&&(k, _)| k == key)
                .map(|&(_, v)| v)
                .ok_or_else(|| SaveError::Missing(key.to_string()))
        };

        let parse_u32 = |key: &str| -> Result<u32, SaveError> {
            find_val(key)?
                .parse::<u32>()
                .map_err(|_| SaveError::Invalid(key.to_string()))
        };
        let parse_u64 = |key: &str| -> Result<u64, SaveError> {
            find_val(key)?
                .parse::<u64>()
                .map_err(|_| SaveError::Invalid(key.to_string()))
        };
        let parse_f32 = |key: &str| -> Result<f32, SaveError> {
            let v = find_val(key)?
                .parse::<f32>()
                .map_err(|_| SaveError::Invalid(key.to_string()))?;
            if !v.is_finite() {
                return Err(SaveError::Invalid(key.to_string()));
            }
            Ok(v)
        };
        let parse_f64 = |key: &str| -> Result<f64, SaveError> {
            let v = find_val(key)?
                .parse::<f64>()
                .map_err(|_| SaveError::Invalid(key.to_string()))?;
            if !v.is_finite() || v < 0.0 {
                return Err(SaveError::Invalid(key.to_string()));
            }
            Ok(v)
        };

        let version = parse_u32("version")?;
        if version == 0 || version > SAVE_VERSION {
            return Err(SaveError::BadVersion(version));
        }

        let created_unix = parse_u64("created_unix")?;
        let playtime_seconds = parse_f64("playtime_seconds")?;

        let player_pos = Vec3::new(
            parse_f32("player.x")?,
            parse_f32("player.y")?,
            parse_f32("player.z")?,
        );
        let player_yaw = parse_f32("player.yaw")?;
        let player_health = parse_f32("player.health")?;
        let player_mana = parse_f32("player.mana")?;
        let total_kills = parse_u32("player.total_kills")?;

        let wave_number = parse_u32("progress.wave")?.max(1);
        let wave_phase = match find_val("progress.phase")? {
            "Active" => WavePhase::Active,
            "Intermission" => WavePhase::Intermission,
            _ => return Err(SaveError::Invalid("progress.phase".to_string())),
        };
        let intermission_timer = parse_f32("progress.intermission_timer")?.max(0.0);
        let pending_spawns = parse_u32("progress.pending_spawns")?;
        let score = parse_u32("progress.score")?;

        let mut skill_counts = [0u8; SKILL_COUNT];
        for i in 0..SKILL_COUNT {
            let k = format!("skill.{i}.count");
            let c = parse_u32(&k)?;
            skill_counts[i] = (c.min(u32::from(MAX_SKILL_COPIES))) as u8;
        }

        let mut skill_slots = [None; SLOT_COUNT];
        for i in 0..SLOT_COUNT {
            let k = format!("slot.{i}");
            let v_str = find_val(&k)?;
            let code = v_str
                .parse::<i32>()
                .map_err(|_| SaveError::Invalid(k.clone()))?;
            if code >= 0 {
                let idx = code as usize;
                if idx >= SKILL_COUNT {
                    return Err(SaveError::Invalid(k));
                }
                let sk = SKILL_TABLE[idx].id;
                if !skill_slots.contains(&Some(sk)) {
                    skill_slots[i] = Some(sk);
                }
            }
        }

        let squad_mode = match parse_u32("squad.mode")? {
            0 => SquadMode::Follow,
            1 => SquadMode::Hold,
            2 => SquadMode::Aggressive,
            _ => return Err(SaveError::Invalid("squad.mode".to_string())),
        };

        let raw_ally_count = (parse_u32("ally.count")? as usize).min(MAX_ALLIES);
        let mut allies = Vec::with_capacity(raw_ally_count);
        for i in 0..raw_ally_count {
            let r_key = format!("ally.{i}.rarity");
            let rarity = match parse_u32(&r_key)? {
                0 => Rarity::Common,
                1 => Rarity::Rare,
                2 => Rarity::Epic,
                _ => return Err(SaveError::Invalid(r_key)),
            };
            let n_key = format!("ally.{i}.name_index");
            let name_index = (parse_u32(&n_key)? as usize).min(5);
            let health = parse_f32(&format!("ally.{i}.health"))?;
            let down = parse_u32(&format!("ally.{i}.down"))? != 0;
            let pos = Vec3::new(
                parse_f32(&format!("ally.{i}.x"))?,
                parse_f32(&format!("ally.{i}.y"))?,
                parse_f32(&format!("ally.{i}.z"))?,
            );
            let hold_pos = Vec3::new(
                parse_f32(&format!("ally.{i}.hold_x"))?,
                parse_f32(&format!("ally.{i}.hold_y"))?,
                parse_f32(&format!("ally.{i}.hold_z"))?,
            );
            allies.push(SavedAlly {
                rarity,
                name_index,
                health,
                down,
                pos,
                hold_pos,
            });
        }

        let raw_enemy_count = (parse_u32("enemy.count")? as usize).min(MAX_ALIVE_ENEMIES);
        let mut enemies = Vec::with_capacity(raw_enemy_count);
        for i in 0..raw_enemy_count {
            let pos = Vec3::new(
                parse_f32(&format!("enemy.{i}.x"))?,
                parse_f32(&format!("enemy.{i}.y"))?,
                parse_f32(&format!("enemy.{i}.z"))?,
            );
            let health = parse_f32(&format!("enemy.{i}.health"))?;
            enemies.push(SavedEnemy { pos, health });
        }

        let wave_rng_state = parse_u64("rng.wave")?;
        let squad_rng_state = parse_u64("rng.squad")?;
        let pickup_rng_state = parse_u64("rng.pickup")?;
        let first_wave_drop_pending = parse_u32("rng.first_wave_drop")? != 0;

        Ok(Self {
            version,
            created_unix,
            playtime_seconds,
            player_pos,
            player_yaw,
            player_health,
            player_mana,
            total_kills,
            wave_number,
            wave_phase,
            intermission_timer,
            pending_spawns,
            score,
            skill_counts,
            skill_slots,
            squad_mode,
            allies,
            enemies,
            wave_rng_state,
            squad_rng_state,
            pickup_rng_state,
            first_wave_drop_pending,
        })
    }

    /// Clamps values into legal ranges and pushes any saved actor position out of cube AABBs.
    pub fn validate_and_sanitize(&mut self, collision: &CollisionGrid) {
        self.player_health = self.player_health.clamp(1.0, PLAYER_MAX_HEALTH);
        self.player_mana = self.player_mana.clamp(0.0, PLAYER_MAX_MANA);
        self.player_yaw = self.player_yaw.rem_euclid(std::f32::consts::TAU);
        self.player_pos = sanitize_actor_pos(
            self.player_pos,
            PLAYER_RADIUS,
            PLAYER_HEIGHT,
            collision,
        );

        self.allies.truncate(MAX_ALLIES);
        for ally in &mut self.allies {
            ally.name_index = ally.name_index.min(5);
            let max_hp = ally.rarity.max_health();
            if ally.down {
                ally.health = 0.0;
            } else {
                ally.health = ally.health.clamp(1.0, max_hp);
            }
            ally.pos = sanitize_actor_pos(ally.pos, ALLY_RADIUS, ALLY_HEIGHT, collision);
            ally.hold_pos = sanitize_actor_pos(ally.hold_pos, ALLY_RADIUS, ALLY_HEIGHT, collision);
        }

        self.enemies.truncate(MAX_ALIVE_ENEMIES);
        for enemy in &mut self.enemies {
            enemy.health = enemy.health.clamp(1.0, ENEMY_MAX_HEALTH);
            enemy.pos = sanitize_actor_pos(enemy.pos, ENEMY_RADIUS, ENEMY_HEIGHT, collision);
        }
    }

    pub fn slot_info(&self) -> SlotInfo {
        SlotInfo {
            created_unix: self.created_unix,
            wave: self.wave_number,
            score: self.score,
            playtime_seconds: self.playtime_seconds,
            ally_count: self.allies.len(),
        }
    }
}

pub fn sanitize_actor_pos(
    mut pos: Vec3,
    radius: f32,
    height: f32,
    collision: &CollisionGrid,
) -> Vec3 {
    let half = GROUND_SIZE * 0.5 - radius;
    pos.x = pos.x.clamp(-half, half);
    pos.z = pos.z.clamp(-half, half);
    pos.y = pos.y.max(0.0);

    let mut dummy_vel = Vec3::ZERO;
    collision.resolve_player_xz(&mut pos, &mut dummy_vel, radius, height, GROUND_SIZE * 0.5);

    if !collision.is_circle_free(pos.x, pos.z, radius - 0.02, height) {
        let base_x = pos.x;
        let base_z = pos.z;
        for step in 1..120 {
            let r = (step as f32) * 0.35;
            let angle = (step as f32) * 0.9;
            let cx = (base_x + r * angle.cos()).clamp(-half, half);
            let cz = (base_z + r * angle.sin()).clamp(-half, half);
            if collision.is_circle_free(cx, cz, radius + 0.05, height) {
                pos.x = cx;
                pos.z = cz;
                pos.y = 0.0;
                break;
            }
        }
    }
    pos
}

/// Atomically writes `<slot_name>.tmp`, flushes and syncs, copies existing target to `<slot_name>.bak`,
/// and renames over `<slot_name>`. Removes `.tmp` on any failure.
pub fn write_save_atomic(
    base_dir: &Path,
    slot_name: &str,
    data: &SaveData,
) -> Result<(), SaveError> {
    let saves_dir = base_dir.join(SAVE_DIR_NAME);
    std::fs::create_dir_all(&saves_dir).map_err(|e| SaveError::Io(e.to_string()))?;

    let target_path = saves_dir.join(slot_name);
    let tmp_path = saves_dir.join(format!("{slot_name}.tmp"));
    let bak_path = saves_dir.join(format!("{slot_name}.bak"));

    let text = data.to_text();

    let write_res = (|| -> Result<(), std::io::Error> {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(text.as_bytes())?;
        f.flush()?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(SaveError::Io(e.to_string()));
    }

    if target_path.exists() {
        if let Err(e) = std::fs::copy(&target_path, &bak_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(SaveError::Io(format!("backup copy failed: {e}")));
        }
    }

    if let Err(e) = std::fs::rename(&tmp_path, &target_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(SaveError::Io(format!("atomic rename failed: {e}")));
    }

    Ok(())
}

/// Loads `slot_name`, falling back to `<slot_name>.bak` if the main file is corrupt or invalid.
/// Returns `(SaveData, used_backup_flag)`.
pub fn load_save_with_backup(
    base_dir: &Path,
    slot_name: &str,
) -> Result<(SaveData, bool), SaveError> {
    let saves_dir = base_dir.join(SAVE_DIR_NAME);
    let target_path = saves_dir.join(slot_name);
    let bak_path = saves_dir.join(format!("{slot_name}.bak"));

    let primary_err = match std::fs::read_to_string(&target_path) {
        Ok(text) => match SaveData::from_text(&text) {
            Ok(data) => return Ok((data, false)),
            Err(err) => err,
        },
        Err(e) => SaveError::Io(e.to_string()),
    };

    if bak_path.exists() {
        if let Ok(bak_text) = std::fs::read_to_string(&bak_path) {
            if let Ok(data) = SaveData::from_text(&bak_text) {
                return Ok((data, true));
            }
        }
    }

    Err(primary_err)
}

pub fn delete_save(base_dir: &Path, slot_name: &str) -> Result<(), SaveError> {
    let saves_dir = base_dir.join(SAVE_DIR_NAME);
    let target_path = saves_dir.join(slot_name);
    let bak_path = saves_dir.join(format!("{slot_name}.bak"));
    if target_path.exists() {
        std::fs::remove_file(&target_path).map_err(|e| SaveError::Io(e.to_string()))?;
    }
    if bak_path.exists() {
        let _ = std::fs::remove_file(&bak_path);
    }
    Ok(())
}

pub fn list_saves(base_dir: &Path) -> [SlotStatus; 5] {
    let saves_dir = base_dir.join(SAVE_DIR_NAME);
    let mut out = [
        SlotStatus::Empty,
        SlotStatus::Empty,
        SlotStatus::Empty,
        SlotStatus::Empty,
        SlotStatus::Empty,
    ];

    for (i, &name) in SLOT_FILES.iter().enumerate() {
        let path = saves_dir.join(name);
        let bak_path = saves_dir.join(format!("{name}.bak"));
        if !path.exists() && !bak_path.exists() {
            out[i] = SlotStatus::Empty;
            continue;
        }
        match load_save_with_backup(base_dir, name) {
            Ok((data, _)) => out[i] = SlotStatus::Valid(data.slot_info()),
            Err(_) => out[i] = SlotStatus::Corrupt,
        }
    }
    out
}

/// Returns the slot index (`0..5`) of the most recent valid save by `created_unix`.
pub fn most_recent_save_slot(statuses: &[SlotStatus; 5]) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;
    for (i, st) in statuses.iter().enumerate() {
        if let SlotStatus::Valid(info) = st {
            if best.map_or(true, |(_, best_ts)| info.created_unix >= best_ts) {
                best = Some((i, info.created_unix));
            }
        }
    }
    best.map(|(idx, _)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::Aabb;

    fn sample_save_data() -> SaveData {
        SaveData {
            version: SAVE_VERSION,
            created_unix: 1_700_000_000,
            playtime_seconds: 3661.5,
            player_pos: Vec3::new(1.5, 0.0, -2.5),
            player_yaw: 1.25,
            player_health: 82.0,
            player_mana: 64.0,
            total_kills: 14,
            wave_number: 3,
            wave_phase: WavePhase::Intermission,
            intermission_timer: 2.5,
            pending_spawns: 0,
            score: 140,
            skill_counts: [2, 1, 1, 0, 1, 1, 0, 0, 0],
            skill_slots: [
                Some(SkillId::SteamBurst),
                Some(SkillId::HealPulse),
                Some(SkillId::WindDash),
                None,
            ],
            squad_mode: SquadMode::Aggressive,
            allies: vec![
                SavedAlly {
                    rarity: Rarity::Rare,
                    name_index: 2,
                    health: 75.0,
                    down: false,
                    pos: Vec3::new(3.0, 0.0, -2.0),
                    hold_pos: Vec3::new(3.0, 0.0, -2.0),
                },
                SavedAlly {
                    rarity: Rarity::Common,
                    name_index: 0,
                    health: 0.0,
                    down: true,
                    pos: Vec3::new(-2.0, 0.0, 1.0),
                    hold_pos: Vec3::new(-2.0, 0.0, 1.0),
                },
            ],
            enemies: vec![SavedEnemy {
                pos: Vec3::new(10.0, 0.0, -12.0),
                health: 35.0,
            }],
            wave_rng_state: 0x1111_2222_3333_4444,
            squad_rng_state: 0x5555_6666_7777_8888,
            pickup_rng_state: 0x9999_AAAA_BBBB_CCCC,
            first_wave_drop_pending: false,
        }
    }

    #[test]
    fn test_save_round_trip_and_checksum_tamper_detection() {
        let orig = sample_save_data();
        let text = orig.to_text();
        let parsed = SaveData::from_text(&text).expect("round-trip should succeed");
        assert_eq!(orig, parsed);

        // Changing a single byte invalidates the FNV-1a checksum
        let mut tampered = text.replace("score=140", "score=141");
        assert_eq!(SaveData::from_text(&tampered), Err(SaveError::BadChecksum));

        // Unknown keys before checksum are ignored when checksum matches
        let mut lines: Vec<&str> = text.lines().collect();
        let _last_checksum = lines.pop();
        let mut extended = lines.join("\n");
        extended.push_str("\nfuture.extra_key=999\n");
        let new_hash = fnv1a_64(extended.as_bytes());
        let _ = writeln!(&mut extended, "checksum={:016x}", new_hash);
        let parsed_ext = SaveData::from_text(&extended).expect("unknown keys should be ignored");
        assert_eq!(orig, parsed_ext);

        // Future version rejected
        tampered = text.replace("version=1", "version=2");
        let lines2: Vec<&str> = tampered.lines().collect();
        let body2 = format!("{}\n", lines2[..lines2.len() - 1].join("\n"));
        let h2 = fnv1a_64(body2.as_bytes());
        let v2_txt = format!("{body2}checksum={h2:016x}\n");
        assert_eq!(SaveData::from_text(&v2_txt), Err(SaveError::BadVersion(2)));
    }

    #[test]
    fn test_truncated_garbage_missing_key_nan_and_cube_pushout() {
        assert_eq!(SaveData::from_text(""), Err(SaveError::BadHeader));
        assert_eq!(SaveData::from_text("NOT_A_SAVE\n"), Err(SaveError::BadHeader));
        assert_eq!(
            SaveData::from_text("PMU_SAVE\nversion=1\n"),
            Err(SaveError::BadChecksum)
        );

        // Missing key names the missing key
        let body = "PMU_SAVE\nversion=1\ncreated_unix=10\n";
        let h = fnv1a_64(body.as_bytes());
        let missing_txt = format!("{body}checksum={h:016x}\n");
        assert_eq!(
            SaveData::from_text(&missing_txt),
            Err(SaveError::Missing("playtime_seconds".to_string()))
        );

        // NaN value rejected
        let orig_txt = sample_save_data().to_text().replace("player.x=1.5", "player.x=NaN");
        let lns: Vec<&str> = orig_txt.lines().collect();
        let b_nan = format!("{}\n", lns[..lns.len() - 1].join("\n"));
        let h_nan = fnv1a_64(b_nan.as_bytes());
        let nan_txt = format!("{b_nan}checksum={h_nan:016x}\n");
        assert_eq!(
            SaveData::from_text(&nan_txt),
            Err(SaveError::Invalid("player.x".to_string()))
        );

        // Position inside a cube is moved out by validate_and_sanitize
        let cube = Aabb {
            min: Vec3::new(-1.0, 0.0, -1.0),
            max: Vec3::new(1.0, 2.0, 1.0),
        };
        let grid = CollisionGrid::new(vec![cube]);
        let mut s = sample_save_data();
        s.player_pos = Vec3::new(0.0, 0.0, 0.0);
        s.validate_and_sanitize(&grid);
        assert!(grid.is_circle_free(s.player_pos.x, s.player_pos.z, PLAYER_RADIUS - 0.02, PLAYER_HEIGHT));
    }

    #[test]
    fn test_atomic_write_backup_fallback_list_delete_and_most_recent() {
        let dir = std::env::temp_dir().join("pmu_test_saves_phase9");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let mut s1 = sample_save_data();
        s1.created_unix = 1000;
        s1.score = 50;
        write_save_atomic(&dir, "slot1.sav", &s1).expect("write slot1");

        // Block `<slot1.sav>.tmp` by creating a directory at `saves/slot1.sav.tmp` so atomic write fails deterministically on Linux
        let tmp_blocker = dir.join(SAVE_DIR_NAME).join("slot1.sav.tmp").join("sub");
        std::fs::create_dir_all(&tmp_blocker).expect("create blocker dir");
        let mut s_fail = sample_save_data();
        s_fail.score = 999;
        assert!(write_save_atomic(&dir, "slot1.sav", &s_fail).is_err());
        std::fs::remove_dir_all(dir.join(SAVE_DIR_NAME).join("slot1.sav.tmp")).expect("clean blocker");

        // Verify original slot1.sav (score = 50) was kept intact!
        let (loaded1, used_bak1) = load_save_with_backup(&dir, "slot1.sav").expect("load slot1");
        assert!(!used_bak1);
        assert_eq!(loaded1.score, 50);

        // Overwrite slot1 with new data (score = 200), which creates slot1.sav.bak (score = 50)
        let mut s2 = sample_save_data();
        s2.created_unix = 2000;
        s2.score = 200;
        write_save_atomic(&dir, "slot1.sav", &s2).expect("overwrite slot1");

        // Corrupt the main slot1.sav file -> load_save_with_backup falls back to slot1.sav.bak!
        std::fs::write(dir.join(SAVE_DIR_NAME).join("slot1.sav"), "CORRUPTED_DATA").expect("corrupt main");
        let (recovered, used_bak2) = load_save_with_backup(&dir, "slot1.sav").expect("recover from bak");
        assert!(used_bak2);
        assert_eq!(recovered.score, 50);

        // Write valid slot2 (timestamp 5000) and corrupt slot3 (no backup)
        write_save_atomic(&dir, "slot1.sav", &s2).expect("restore slot1");
        let mut s_new = sample_save_data();
        s_new.created_unix = 5000;
        write_save_atomic(&dir, "slot2.sav", &s_new).expect("write slot2");
        std::fs::write(dir.join(SAVE_DIR_NAME).join("slot3.sav"), "BAD").expect("write bad slot3");

        let statuses = list_saves(&dir);
        assert!(matches!(statuses[0], SlotStatus::Valid(_)));
        assert!(matches!(statuses[1], SlotStatus::Valid(_)));
        assert_eq!(statuses[2], SlotStatus::Corrupt);
        assert_eq!(statuses[3], SlotStatus::Empty);
        assert_eq!(most_recent_save_slot(&statuses), Some(1));

        // Delete slot2
        delete_save(&dir, "slot2.sav").expect("delete slot2");
        let statuses_after = list_saves(&dir);
        assert_eq!(statuses_after[1], SlotStatus::Empty);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_utc_date_formatter_known_timestamps() {
        assert_eq!(format_utc_datetime(0), "1970-01-01 00:00");
        assert_eq!(format_utc_datetime(86_400), "1970-01-02 00:00");
        assert_eq!(format_utc_datetime(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(format_utc_datetime(1_709_208_000), "2024-02-29 12:00");
        assert_eq!(format_playtime_hms(3661.0), "01:01:01");
    }
}
