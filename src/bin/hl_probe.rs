//! hl_probe: native diagnostic that reports which Highlander-relevant data exists in a
//! SourceTV demo and how it behaves (packet entities + send props, string tables,
//! temp entities, game events, user messages).
//!
//! Usage:
//!   hl_probe <demo.dem> [report.txt]   full probe report (stdout when no path is given)
//!   hl_probe --bench <demo.dem>        time GameStateAnalyser and GameState+match Analyser parses
//!
//! Build: `cargo build --release --features write --bin hl_probe`
//! (`write` is only required so that game event definitions keep their field names.)
//!
//! The probe keeps its own full entity state (baseline + deltas), so it sees every send prop,
//! not only the ones the GameStateAnalyser decodes. Temp entities are delta-merged the way the
//! engine does it (consecutive events of the same class are deltas against the previous event,
//! everything else is a delta against zero).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::time::{Duration, Instant};
use std::{env, fs};

use bitbuffer::BitRead;
use fnv::{FnvHashMap, FnvHashSet};

use tf_demo_parser::demo::data::DemoTick;
use tf_demo_parser::demo::gamevent::{GameEvent, GameEventValue};
use tf_demo_parser::demo::header::Header;
use tf_demo_parser::demo::message::gameevent::GameEventMessage;
use tf_demo_parser::demo::message::packetentities::{
    PacketEntitiesMessage, PacketEntity, UpdateType,
};
use tf_demo_parser::demo::message::tempentities::TempEntitiesMessage;
use tf_demo_parser::demo::message::Message;
use tf_demo_parser::demo::packet::datatable::{ParseSendTable, ServerClass};
use tf_demo_parser::demo::packet::message::MessagePacketMeta;
use tf_demo_parser::demo::packet::stringtable::StringTableEntry;
use tf_demo_parser::demo::parser::analyser::Analyser as MatchAnalyser;
use tf_demo_parser::demo::parser::gamestateanalyser::GameStateAnalyser;
use tf_demo_parser::demo::parser::handler::{BorrowMessageHandler, DemoHandler};
use tf_demo_parser::demo::parser::{MessageHandler, RawPacketStream};
use tf_demo_parser::demo::sendprop::{
    RawSendPropDefinition, SendPropFlag, SendPropIdentifier, SendPropType, SendPropValue,
};
use tf_demo_parser::{Demo, DemoParser, MessageType, ParserState};

// ---------------------------------------------------------------------------------------------
// Name tables (generated from source-sdk-2013 src/game/shared/tf/tf_shareddefs.h and
// src/game/shared/Multiplayer/multiplayer_animstate.h)
// ---------------------------------------------------------------------------------------------

const WEAPON_IDS: [&str; 110] = [
    "NONE",
    "BAT",
    "BAT_WOOD",
    "BOTTLE",
    "FIREAXE",
    "CLUB",
    "CROWBAR",
    "KNIFE",
    "FISTS",
    "SHOVEL",
    "WRENCH",
    "BONESAW",
    "SHOTGUN_PRIMARY",
    "SHOTGUN_SOLDIER",
    "SHOTGUN_HWG",
    "SHOTGUN_PYRO",
    "SCATTERGUN",
    "SNIPERRIFLE",
    "MINIGUN",
    "SMG",
    "SYRINGEGUN_MEDIC",
    "TRANQ",
    "ROCKETLAUNCHER",
    "GRENADELAUNCHER",
    "PIPEBOMBLAUNCHER",
    "FLAMETHROWER",
    "GRENADE_NORMAL",
    "GRENADE_CONCUSSION",
    "GRENADE_NAIL",
    "GRENADE_MIRV",
    "GRENADE_MIRV_DEMOMAN",
    "GRENADE_NAPALM",
    "GRENADE_GAS",
    "GRENADE_EMP",
    "GRENADE_CALTROP",
    "GRENADE_PIPEBOMB",
    "GRENADE_SMOKE_BOMB",
    "GRENADE_HEAL",
    "GRENADE_STUNBALL",
    "GRENADE_JAR",
    "GRENADE_JAR_MILK",
    "PISTOL",
    "PISTOL_SCOUT",
    "REVOLVER",
    "NAILGUN",
    "PDA",
    "PDA_ENGINEER_BUILD",
    "PDA_ENGINEER_DESTROY",
    "PDA_SPY",
    "BUILDER",
    "MEDIGUN",
    "GRENADE_MIRVBOMB",
    "FLAMETHROWER_ROCKET",
    "GRENADE_DEMOMAN",
    "SENTRY_BULLET",
    "SENTRY_ROCKET",
    "DISPENSER",
    "INVIS",
    "FLAREGUN",
    "LUNCHBOX",
    "JAR",
    "COMPOUND_BOW",
    "BUFF_ITEM",
    "PUMPKIN_BOMB",
    "SWORD",
    "ROCKETLAUNCHER_DIRECTHIT",
    "LIFELINE",
    "LASER_POINTER",
    "DISPENSER_GUN",
    "SENTRY_REVENGE",
    "JAR_MILK",
    "HANDGUN_SCOUT_PRIMARY",
    "BAT_FISH",
    "CROSSBOW",
    "STICKBOMB",
    "HANDGUN_SCOUT_SECONDARY",
    "SODA_POPPER",
    "SNIPERRIFLE_DECAP",
    "RAYGUN",
    "PARTICLE_CANNON",
    "MECHANICAL_ARM",
    "DRG_POMSON",
    "BAT_GIFTWRAP",
    "GRENADE_ORNAMENT_BALL",
    "FLAREGUN_REVENGE",
    "PEP_BRAWLER_BLASTER",
    "CLEAVER",
    "GRENADE_CLEAVER",
    "STICKY_BALL_LAUNCHER",
    "GRENADE_STICKY_BALL",
    "SHOTGUN_BUILDING_RESCUE",
    "CANNON",
    "THROWABLE",
    "GRENADE_THROWABLE",
    "PDA_SPY_BUILD",
    "GRENADE_WATERBALLOON",
    "HARVESTER_SAW",
    "SPELLBOOK",
    "SPELLBOOK_PROJECTILE",
    "SNIPERRIFLE_CLASSIC",
    "PARACHUTE",
    "GRAPPLINGHOOK",
    "PASSTIME_GUN",
    "CHARGED_SMG",
    "BREAKABLE_SIGN",
    "ROCKETPACK",
    "SLAP",
    "JAR_GAS",
    "GRENADE_JAR_GAS",
    "FLAME_BALL",
];
const COND_NAMES: [&str; 131] = [
    "AIMING",
    "ZOOMED",
    "DISGUISING",
    "DISGUISED",
    "STEALTHED",
    "INVULNERABLE",
    "TELEPORTED",
    "TAUNTING",
    "INVULNERABLE_WEARINGOFF",
    "STEALTHED_BLINK",
    "SELECTED_TO_TELEPORT",
    "CRITBOOSTED",
    "TMPDAMAGEBONUS",
    "FEIGN_DEATH",
    "PHASE",
    "STUNNED",
    "OFFENSEBUFF",
    "SHIELD_CHARGE",
    "DEMO_BUFF",
    "ENERGY_BUFF",
    "RADIUSHEAL",
    "HEALTH_BUFF",
    "BURNING",
    "HEALTH_OVERHEALED",
    "URINE",
    "BLEEDING",
    "DEFENSEBUFF",
    "MAD_MILK",
    "MEGAHEAL",
    "REGENONDAMAGEBUFF",
    "MARKEDFORDEATH",
    "NOHEALINGDAMAGEBUFF",
    "SPEED_BOOST",
    "CRITBOOSTED_PUMPKIN",
    "CRITBOOSTED_USER_BUFF",
    "CRITBOOSTED_DEMO_CHARGE",
    "SODAPOPPER_HYPE",
    "CRITBOOSTED_FIRST_BLOOD",
    "CRITBOOSTED_BONUS_TIME",
    "CRITBOOSTED_CTF_CAPTURE",
    "CRITBOOSTED_ON_KILL",
    "CANNOT_SWITCH_FROM_MELEE",
    "DEFENSEBUFF_NO_CRIT_BLOCK",
    "REPROGRAMMED",
    "CRITBOOSTED_RAGE_BUFF",
    "DEFENSEBUFF_HIGH",
    "SNIPERCHARGE_RAGE_BUFF",
    "DISGUISE_WEARINGOFF",
    "MARKEDFORDEATH_SILENT",
    "DISGUISED_AS_DISPENSER",
    "SAPPED",
    "INVULNERABLE_HIDE_UNLESS_DAMAGED",
    "INVULNERABLE_USER_BUFF",
    "HALLOWEEN_BOMB_HEAD",
    "HALLOWEEN_THRILLER",
    "RADIUSHEAL_ON_DAMAGE",
    "CRITBOOSTED_CARD_EFFECT",
    "INVULNERABLE_CARD_EFFECT",
    "MEDIGUN_UBER_BULLET_RESIST",
    "MEDIGUN_UBER_BLAST_RESIST",
    "MEDIGUN_UBER_FIRE_RESIST",
    "MEDIGUN_SMALL_BULLET_RESIST",
    "MEDIGUN_SMALL_BLAST_RESIST",
    "MEDIGUN_SMALL_FIRE_RESIST",
    "STEALTHED_USER_BUFF",
    "MEDIGUN_DEBUFF",
    "STEALTHED_USER_BUFF_FADING",
    "BULLET_IMMUNE",
    "BLAST_IMMUNE",
    "FIRE_IMMUNE",
    "PREVENT_DEATH",
    "MVM_BOT_STUN_RADIOWAVE",
    "HALLOWEEN_SPEED_BOOST",
    "HALLOWEEN_QUICK_HEAL",
    "HALLOWEEN_GIANT",
    "HALLOWEEN_TINY",
    "HALLOWEEN_IN_HELL",
    "HALLOWEEN_GHOST_MODE",
    "MINICRITBOOSTED_ON_KILL",
    "OBSCURED_SMOKE",
    "PARACHUTE_ACTIVE",
    "BLASTJUMPING",
    "HALLOWEEN_KART",
    "HALLOWEEN_KART_DASH",
    "BALLOON_HEAD",
    "MELEE_ONLY",
    "SWIMMING_CURSE",
    "FREEZE_INPUT",
    "HALLOWEEN_KART_CAGE",
    "DONOTUSE_0",
    "RUNE_STRENGTH",
    "RUNE_HASTE",
    "RUNE_REGEN",
    "RUNE_RESIST",
    "RUNE_VAMPIRE",
    "RUNE_REFLECT",
    "RUNE_PRECISION",
    "RUNE_AGILITY",
    "GRAPPLINGHOOK",
    "GRAPPLINGHOOK_SAFEFALL",
    "GRAPPLINGHOOK_LATCHED",
    "GRAPPLINGHOOK_BLEEDING",
    "AFTERBURN_IMMUNE",
    "RUNE_KNOCKOUT",
    "RUNE_IMBALANCE",
    "CRITBOOSTED_RUNE_TEMP",
    "PASSTIME_INTERCEPTION",
    "SWIMMING_NO_EFFECTS",
    "PURGATORY",
    "RUNE_KING",
    "RUNE_PLAGUE",
    "RUNE_SUPERNOVA",
    "PLAGUE",
    "KING_BUFFED",
    "TEAM_GLOWS",
    "KNOCKED_INTO_AIR",
    "COMPETITIVE_WINNER",
    "COMPETITIVE_LOSER",
    "HEALING_DEBUFF",
    "PASSTIME_PENALTY_DEBUFF",
    "GRAPPLED_TO_PLAYER",
    "GRAPPLED_BY_PLAYER",
    "PARACHUTE_DEPLOYED",
    "GAS",
    "BURNING_PYRO",
    "ROCKETPACK",
    "LOST_FOOTING",
    "AIR_CURRENT",
    "HALLOWEEN_HELL_HEAL",
    "POWERUPMODE_DOMINANT",
    "IMMUNE_TO_PUSHBACK",
];
const PROJECTILE_TYPES: [&str; 31] = [
    "NONE",
    "BULLET",
    "ROCKET",
    "PIPEBOMB",
    "PIPEBOMB_REMOTE",
    "SYRINGE",
    "FLARE",
    "JAR",
    "ARROW",
    "FLAME_ROCKET",
    "JAR_MILK",
    "HEALING_BOLT",
    "ENERGY_BALL",
    "ENERGY_RING",
    "PIPEBOMB_PRACTICE",
    "CLEAVER",
    "STICKY_BALL",
    "CANNONBALL",
    "BUILDING_REPAIR_BOLT",
    "FESTIVE_ARROW",
    "THROWABLE",
    "SPELL",
    "FESTIVE_JAR",
    "FESTIVE_HEALING_BOLT",
    "BREADMONSTER_JARATE",
    "BREADMONSTER_MADMILK",
    "GRAPPLINGHOOK",
    "SENTRY_ROCKET",
    "BREAD_MONSTER",
    "JAR_GAS",
    "FLAME_BALL",
];
const ANIM_EVENTS: [&str; 41] = [
    "ATTACK_PRIMARY",
    "ATTACK_SECONDARY",
    "ATTACK_GRENADE",
    "RELOAD",
    "RELOAD_LOOP",
    "RELOAD_END",
    "JUMP",
    "SWIM",
    "DIE",
    "FLINCH_CHEST",
    "FLINCH_HEAD",
    "FLINCH_LEFTARM",
    "FLINCH_RIGHTARM",
    "FLINCH_LEFTLEG",
    "FLINCH_RIGHTLEG",
    "DOUBLEJUMP",
    "CANCEL",
    "SPAWN",
    "SNAP_YAW",
    "CUSTOM",
    "CUSTOM_GESTURE",
    "CUSTOM_SEQUENCE",
    "CUSTOM_GESTURE_SEQUENCE",
    "ATTACK_PRE",
    "ATTACK_POST",
    "GRENADE1_DRAW",
    "GRENADE2_DRAW",
    "GRENADE1_THROW",
    "GRENADE2_THROW",
    "VOICE_COMMAND_GESTURE",
    "DOUBLEJUMP_CROUCH",
    "STUN_BEGIN",
    "STUN_MIDDLE",
    "STUN_END",
    "PASSTIME_THROW_BEGIN",
    "PASSTIME_THROW_MIDDLE",
    "PASSTIME_THROW_END",
    "CYOAPDA_BEGIN",
    "CYOAPDA_MIDDLE",
    "CYOAPDA_END",
    "ATTACK_PRIMARY_SUPER",
];
const CLASS_NAMES: [&str; 10] = [
    "none", "scout", "sniper", "soldier", "demoman", "medic", "heavy", "pyro", "spy", "engineer",
];
// as listed in src/demo/message/usermessage.rs (UserMessageType)
const USER_MSG_NAMES: [&str; 58] = [
    "Geiger",
    "Train",
    "HudText",
    "SayText",
    "SayText2",
    "TextMsg",
    "ResetHUD",
    "GameTitle",
    "ItemPickup",
    "ShowMenu",
    "Shake",
    "Fade",
    "VGuiMenu",
    "Rumble",
    "CloseCaption",
    "SendAudio",
    "VoiceMask",
    "RequestState",
    "Damage",
    "HintText",
    "KeyHintText",
    "HudMsg",
    "AmmoDenied",
    "AchievementEvent",
    "UpdateRadar",
    "VoiceSubtitle",
    "HudNotify",
    "HudNotifyCustom",
    "PlayerStatsUpdate",
    "PlayerIgnited",
    "PlayerIgnitedInv",
    "HudArenaNotify",
    "UpdateAchievement",
    "TrainingMsg",
    "TrainingObjective",
    "DamageDodged",
    "PlayerJarated",
    "PlayerExtinguished",
    "PlayerJaratedFade",
    "PlayerShieldBlocked",
    "BreakModel",
    "CheapBreakModel",
    "BreakModelPumpkin",
    "BreakModelRocketDud",
    "CallVoteFailed",
    "VoteStart",
    "VotePass",
    "VoteFailed",
    "VoteSetup",
    "PlayerBonusPoints",
    "SpawnFlyingBird",
    "PlayerGodRayEffect",
    "SPHapWeapEvent",
    "HapDmg",
    "HapPunch",
    "HapSetDrag",
    "HapSet",
    "HapMeleeContact",
];

/// Single-instance / objective-ish classes whose full state and change log get dumped.
const INFO_CLASSES: &[&str] = &[
    "CTFObjectiveResource",
    "CTeamRoundTimer",
    "CTFGameRulesProxy",
    "CTeamTrainWatcher",
    "CFuncTrackTrain",
    "CObjectCartDispenser",
    "CTFTeam",
    "CTeamplayRoundBasedRulesProxy",
    "CCaptureFlag",
    "CTeamControlPoint",
    "CTeamControlPointMaster",
    "CTriggerAreaCapture",
    "CCaptureZone",
    "CMonsterResource",
    "CTFLogicArena",
    "CFuncRespawnRoomVisualizer",
    "CFuncRespawnRoom",
    "CTFPlayerDestructionLogic",
];

/// Classes whose flattened send table gets dumped (in addition to all weapon/projectile/TE
/// classes that were seen).
const DUMP_CLASSES: &[&str] = &[
    "CTFPlayer",
    "CTFPlayerResource",
    "CTFViewModel",
    "CSniperDot",
    "CObjectSentrygun",
    "CObjectDispenser",
    "CObjectTeleporter",
    "CObjectCartDispenser",
    "CObjectSapper",
    "CTFObjectiveResource",
    "CTeamRoundTimer",
    "CTFGameRulesProxy",
    "CTeamTrainWatcher",
    "CFuncTrackTrain",
    "CTFTeam",
    "CTFAmmoPack",
    "CTFDroppedWeapon",
    "CTFRagdoll",
    "CBaseDoor",
    "CFuncMoveLinear",
    "CFuncRotating",
    "CBaseToggle",
    "CFuncBrush",
    "CDynamicProp",
    "CBaseAnimating",
    "CPhysicsProp",
    "CTFWearable",
    "CTFWearableVM",
    "CTFWearableDemoShield",
    "CBaseEntity",
    "CFuncAreaPortalWindow",
    "CFuncRespawnRoomVisualizer",
    "CSprite",
    "CTFGenericBomb",
    "CTFPumpkinBomb",
    "CTFFlameManager",
    "CLaserDot",
    "CTFTauntProp",
    "CSceneEntity",
    "CTFPowerupBottle",
];

const INVALID_EHANDLE: i64 = (1 << 21) - 1;

fn h_index(h: i64) -> Option<u32> {
    if h < 0 || h == INVALID_EHANDLE {
        None
    } else {
        Some((h & 0x7FF) as u32)
    }
}

fn h_serial(h: i64) -> u32 {
    ((h >> 11) & 0x3FF) as u32
}

fn fh(h: i64) -> String {
    match h_index(h) {
        None => "null".into(),
        Some(i) => format!("#{}/s{}", i, h_serial(h)),
    }
}

fn ff(f: f32) -> String {
    if !f.is_finite() {
        return format!("{f}");
    }
    if f == f.trunc() && f.abs() < 1e9 {
        return format!("{}", f as i64);
    }
    if f.abs() >= 0.01 {
        let s = format!("{f:.4}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    } else {
        format!("{f:.3e}")
    }
}

fn fmt_raw(v: &SendPropValue) -> String {
    match v {
        SendPropValue::Integer(i) => i.to_string(),
        SendPropValue::Float(f) => ff(*f),
        SendPropValue::Vector(v) => format!("({},{},{})", ff(v.x), ff(v.y), ff(v.z)),
        SendPropValue::VectorXY(v) => format!("({},{})", ff(v.x), ff(v.y)),
        SendPropValue::String(s) => format!("{s:?}"),
        SendPropValue::Array(a) => fmt_array(a),
    }
}

fn fmt_array(a: &[SendPropValue]) -> String {
    format!("[{}]", a.iter().map(fmt_raw).collect::<Vec<_>>().join(","))
}

fn def_str(p: &RawSendPropDefinition) -> String {
    const FLAGS: [(SendPropFlag, &str); 15] = [
        (SendPropFlag::Unsigned, "UNSIGNED"),
        (SendPropFlag::Coord, "COORD"),
        (SendPropFlag::NoScale, "NOSCALE"),
        (SendPropFlag::RoundDown, "ROUNDDOWN"),
        (SendPropFlag::RoundUp, "ROUNDUP"),
        (SendPropFlag::NormalVarInt, "NORMAL/VARINT"),
        (SendPropFlag::Exclude, "EXCLUDE"),
        (SendPropFlag::XYZE, "XYZE"),
        (SendPropFlag::InsideArray, "INSIDEARRAY"),
        (SendPropFlag::ProxyAlwaysYes, "PROXY_ALWAYS"),
        (SendPropFlag::ChangesOften, "CHANGES_OFTEN"),
        (SendPropFlag::IsVectorElement, "VECTORELEM"),
        (SendPropFlag::Collapsible, "COLLAPSIBLE"),
        (SendPropFlag::CoordMP, "COORD_MP"),
        (SendPropFlag::CoordMPLowPrecision, "COORD_MP_LOWPRECISION"),
    ];
    let mut flags: Vec<&str> = FLAGS
        .iter()
        .filter(|(f, _)| p.flags.contains(*f))
        .map(|(_, n)| *n)
        .collect();
    if p.flags.contains(SendPropFlag::CoordMPIntegral) {
        flags.push("COORD_MP_INTEGRAL");
    }
    let fl = if flags.is_empty() {
        String::new()
    } else {
        format!(" {}", flags.join("|"))
    };
    let bits = p.original_bit_count.unwrap_or(0);
    let coordish = p.flags.contains(SendPropFlag::Coord)
        || p.flags.contains(SendPropFlag::CoordMP)
        || p.flags.contains(SendPropFlag::CoordMPLowPrecision)
        || p.flags.contains(SendPropFlag::CoordMPIntegral)
        || p.flags.contains(SendPropFlag::NoScale)
        || p.flags.contains(SendPropFlag::NormalVarInt);
    match p.prop_type {
        SendPropType::Int => {
            if p.flags.contains(SendPropFlag::NormalVarInt) {
                format!("int varint{fl}")
            } else {
                format!("int {bits}b{fl}")
            }
        }
        SendPropType::Float | SendPropType::Vector | SendPropType::VectorXY => {
            let t = match p.prop_type {
                SendPropType::Float => "float",
                SendPropType::Vector => "vector",
                _ => "vectorXY",
            };
            if coordish {
                format!("{t}{fl}")
            } else {
                format!(
                    "{t} {bits}b [{}..{}]{fl}",
                    ff(p.low_value.unwrap_or(0.0)),
                    ff(p.high_value.unwrap_or(0.0))
                )
            }
        }
        SendPropType::String => format!("string{fl}"),
        SendPropType::Array => format!(
            "array[{}] of {}",
            p.element_count.unwrap_or(0),
            p.array_property.as_deref().map(def_str).unwrap_or_default()
        ),
        SendPropType::DataTable => format!(
            "datatable {}",
            p.table_name
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_default()
        ),
        SendPropType::NumSendPropTypes => "?".into(),
    }
}

fn cond_name(c: usize) -> String {
    COND_NAMES
        .get(c)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("COND_{c}"))
}

fn weapon_id_name(id: i64) -> String {
    WEAPON_IDS
        .get(id as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("WEAPON_{id}"))
}

fn anim_event_name(id: i64) -> String {
    ANIM_EVENTS
        .get(id as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("EVENT_{id}"))
}

fn class_name(id: i64) -> &'static str {
    CLASS_NAMES.get(id as usize).copied().unwrap_or("?")
}

fn vm_hwm() -> String {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .filter(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(", ")
}

fn id(table: &str, prop: &str) -> SendPropIdentifier {
    SendPropIdentifier::new(table, prop)
}

macro_rules! w {
    ($o:expr) => { let _ = writeln!($o); };
    ($o:expr, $($t:tt)*) => { let _ = writeln!($o, $($t)*); };
}

// ---------------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Val {
    I(i64),
    F(f32),
    V([f32; 3]),
    XY([f32; 2]),
    S(u32),
}

impl Val {
    fn same(&self, other: &Val) -> bool {
        match (self, other) {
            (Val::F(a), Val::F(b)) => a.to_bits() == b.to_bits(),
            (Val::V(a), Val::V(b)) => a
                .iter()
                .zip(b.iter())
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            (Val::XY(a), Val::XY(b)) => a
                .iter()
                .zip(b.iter())
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            (a, b) => a == b,
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Val::I(i) => Some(*i as f64),
            Val::F(f) => Some(*f as f64),
            _ => None,
        }
    }
    fn int(&self) -> Option<i64> {
        match self {
            Val::I(i) => Some(*i),
            Val::F(f) => Some(*f as i64),
            _ => None,
        }
    }
    fn float(&self) -> Option<f32> {
        match self {
            Val::F(f) => Some(*f),
            Val::I(i) => Some(*i as f32),
            _ => None,
        }
    }
    fn vec3(&self) -> Option<[f32; 3]> {
        match self {
            Val::V(v) => Some(*v),
            Val::XY(v) => Some([v[0], v[1], 0.0]),
            _ => None,
        }
    }
    fn is_zero(&self) -> bool {
        match self {
            Val::I(i) => *i == 0,
            Val::F(f) => *f == 0.0,
            Val::V(v) => v.iter().all(|x| *x == 0.0),
            Val::XY(v) => v.iter().all(|x| *x == 0.0),
            Val::S(_) => false,
        }
    }
}

#[derive(Default)]
struct Interner {
    map: FnvHashMap<String, u32>,
    list: Vec<String>,
}

impl Interner {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(i) = self.map.get(s) {
            return *i;
        }
        let i = self.list.len() as u32;
        self.list.push(s.to_string());
        self.map.insert(s.to_string(), i);
        i
    }
    fn get(&self, i: u32) -> &str {
        self.list.get(i as usize).map(|s| s.as_str()).unwrap_or("?")
    }
}

fn to_val(v: &SendPropValue, interner: &mut Interner) -> Val {
    match v {
        SendPropValue::Integer(i) => Val::I(*i),
        SendPropValue::Float(f) => Val::F(*f),
        SendPropValue::Vector(v) => Val::V([v.x, v.y, v.z]),
        SendPropValue::VectorXY(v) => Val::XY([v.x, v.y]),
        SendPropValue::String(s) => Val::S(interner.id(s)),
        SendPropValue::Array(a) => {
            let s = fmt_array(a);
            Val::S(interner.id(&s))
        }
    }
}

fn add_distinct(list: &mut Vec<(Val, u64)>, overflow: &mut bool, v: Val, cap: usize) {
    if let Some(e) = list.iter_mut().find(|(x, _)| x.same(&v)) {
        e.1 += 1;
        return;
    }
    if list.len() < cap {
        list.push((v, 1));
    } else {
        *overflow = true;
    }
}

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Player,
    Weapon,
    Projectile,
    ViewModel,
    Wearable,
    Building,
    Other,
}

#[derive(Clone)]
struct ClassInfo {
    name: String,
    table: String,
    kind: Kind,
    flat: Vec<SendPropIdentifier>,
    by_name: FnvHashMap<String, Vec<SendPropIdentifier>>,
    /// flattened index -> synthetic identifier, for identifiers that occur more than once in
    /// the flattened table (UtlVector-of-datatable props such as m_ConditionData[i].m_pProvider
    /// or item attribute lists share one (table, prop) hash for every element)
    dup: FnvHashMap<u32, SendPropIdentifier>,
}

struct PropName {
    table: String,
    prop: String,
    def: String,
}

struct Ent {
    class: u16,
    serial: u32,
    instance: u32,
    created: u32,
    props: FnvHashMap<SendPropIdentifier, Val>,
}

#[derive(Default)]
struct PropStat {
    enter_sent: u64,
    sends: u64,
    changes: u64,
    ents: FnvHashSet<u32>,
    first_change: Option<u32>,
    last_change: u32,
    distinct: Vec<(Val, u64)>,
    overflow: bool,
    min: f64,
    max: f64,
    has_num: bool,
    enter_vals: Vec<(Val, u64)>,
    enter_overflow: bool,
}

#[derive(Default)]
struct ClassStat {
    instances: u64,
    enters: u64,
    reenters: u64,
    leaves: u64,
    deletes: u64,
    deltas: u64,
    alive: i64,
    max_alive: i64,
    lifetime_ticks: u64,
    props: FnvHashMap<SendPropIdentifier, PropStat>,
    models: BTreeMap<String, u64>,
}

#[derive(Clone, Copy)]
struct LogE {
    tick: u32,
    ent: u16,
    /// value at instance creation (baseline + enter props), not a change
    init: bool,
    val: Val,
}

struct PropLog {
    v: Vec<LogE>,
    total: u64,
    cap: usize,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct Life {
    tick: u32,
    ent: u16,
    class: u16,
    serial: u32,
    kind: u8, // 0 create, 1 leave, 2 reenter, 3 delete
}

#[derive(Default)]
struct TeStat {
    count: u64,
    reliable: u64,
    delayed: u64,
    empty: u64,
    props: FnvHashMap<SendPropIdentifier, (u64, Vec<(Val, u64)>, bool)>,
    samples: Vec<String>,
    first: Option<u32>,
}

struct FireBullet {
    tick: u32,
    player: i64,
    weapon: i64,
    mode: i64,
    seed: i64,
    spread: f32,
    crit: i64,
    origin: [f32; 3],
    ang: [f32; 2],
    p_origin: Option<[f32; 3]>,
    p_ang: Option<[f32; 2]>,
    p_class: i64,
}

struct AnimEv {
    tick: u32,
    player: i64,
    event: i64,
    data: i64,
}

#[derive(Default)]
struct EvStat {
    count: u64,
    first: u32,
    last: u32,
    fields: BTreeMap<String, (Vec<(String, u64)>, bool)>,
    samples: Vec<String>,
}

#[allow(dead_code)]
struct ProjInfo {
    class: u16,
    ent: u32,
    serial: u32,
    created: u32,
    deleted: Option<u32>,
    owner: i64,
    thrower: i64,
    launcher: i64,
    orig_launcher: i64,
    deflected_owner: i64,
    pipe_type: Option<i64>,
    proj_type: Option<i64>,
    crit: Option<i64>,
    deflected_max: i64,
    deflect_changes: u32,
    model: String,
    team: i64,
    origin: Option<[f32; 3]>,
}

#[allow(dead_code)]
#[derive(Clone)]
struct WeaponInfo {
    class: u16,
    itemdef: i64,
    owner: i64,
    owner_entity: i64,
    created: u32,
    deleted: Option<u32>,
}

#[derive(Default)]
struct MoverStat {
    class: u16,
    ent: u32,
    serial: u32,
    created: u32,
    deleted: Option<u32>,
    model: String,
    origin_changes: u32,
    angle_changes: u32,
    effects_changes: u32,
    effects_vals: BTreeSet<i64>,
    render_changes: u32,
    color_changes: u32,
    model_changes: u32,
    parent: i64,
    parent_changes: u32,
    first_origin: Option<[f32; 3]>,
    min_o: [f32; 3],
    max_o: [f32; 3],
    leaves: u32,
    reenters: u32,
    first_change: Option<u32>,
    last_change: u32,
}

struct DotInfo {
    ent: u32,
    created: u32,
    deleted: Option<u32>,
    owner: i64,
    charge_start: f32,
    origin: Option<[f32; 3]>,
    origin_changes: u32,
    sim_time: f32,
}

struct InfoDump {
    class: String,
    ent: u32,
    serial: u32,
    tick: u32,
    props: Vec<(SendPropIdentifier, Val)>,
}

#[derive(Default)]
struct Probe {
    gs: GameStateAnalyser,
    tick: u32,
    server_tick: u32,
    interval: f32,
    first_tick: Option<u32>,
    last_tick: u32,
    first_server_tick: Option<u32>,
    entity_packet_ticks: Vec<u32>,
    server_ticks_at_entities: Vec<(u32, u32)>,
    msg_counts: BTreeMap<String, u64>,
    user_msgs: BTreeMap<u8, u64>,
    convars: Vec<(u32, String, String)>,
    server_info: String,
    names: FnvHashMap<SendPropIdentifier, PropName>,
    server_classes: Vec<(String, String)>,
    class_info: Vec<Option<ClassInfo>>,
    string_tables: BTreeMap<String, Vec<String>>,
    ents: Vec<Option<Ent>>,
    next_instance: u32,
    class_stats: FnvHashMap<u16, ClassStat>,
    logs: FnvHashMap<(u16, SendPropIdentifier), PropLog>,
    lifecycle: Vec<Life>,
    interner: Interner,
    update_counts: [u64; 4],
    full_updates: u64,
    entity_messages: u64,
    orphan_deltas: u64,
    te_stats: BTreeMap<u16, TeStat>,
    te_total: u64,
    te_msgs: u64,
    fire_bullets: Vec<FireBullet>,
    anim_events: Vec<AnimEv>,
    ev_stats: BTreeMap<String, EvStat>,
    ev_log: Vec<(u32, String, String)>,
    ev_defs: BTreeMap<String, Vec<String>>,
    all_ev_defs: BTreeMap<String, Vec<String>>,
    effect_dispatch: BTreeMap<String, u64>,
    projectiles: Vec<ProjInfo>,
    proj_by_ent: FnvHashMap<u32, usize>,
    weapons: FnvHashMap<(u32, u32), WeaponInfo>,
    movers: FnvHashMap<u32, MoverStat>,
    dots: Vec<DotInfo>,
    dot_by_ent: FnvHashMap<u32, usize>,
    info_dumps: Vec<InfoDump>,
}

const IMPORTANT_PLAYER_PROPS: &[&str] = &[
    "m_nPlayerCond",
    "m_nPlayerCondEx",
    "m_nPlayerCondEx2",
    "m_nPlayerCondEx3",
    "m_nPlayerCondEx4",
    "_condition_bits",
    "m_flCloakMeter",
    "m_flInvisChangeCompleteTime",
    "m_nDisguiseTeam",
    "m_nDisguiseClass",
    "m_nMaskClass",
    "m_hDisguiseTarget",
    "m_iDisguiseHealth",
    "m_hDisguiseWeapon",
    "m_iDisguiseBody",
    "m_nDisguiseSkinOverride",
    "m_hActiveWeapon",
    "m_iFOV",
    "m_iFOVStart",
    "m_flFOVTime",
    "m_iDefaultFOV",
    "m_hZoomOwner",
    "m_nNumHealers",
    "m_iTauntIndex",
    "m_iTauntConcept",
    "m_iTauntItemDefIndex",
    "m_nActiveTauntSlot",
    "m_flTauntYaw",
    "m_bAllowMoveDuringTaunt",
    "m_flCurrentTauntMoveSpeed",
    "m_bCarryingObject",
    "m_hCarriedObject",
    "m_iClass",
    "m_lifeState",
    "m_iHealth",
    "m_flChargeMeter",
    "m_iCritMult",
    "m_nPlayerState",
    "m_iStunFlags",
    "m_bFeignDeathReady",
    "m_iObserverMode",
    "m_hObserverTarget",
    "m_iTeamNum",
    "m_bJumping",
    "m_iAirDash",
    "m_nAirDucked",
    "m_hRagdoll",
    "m_fFlags",
    "m_iSpawnCounter",
    "m_flMaxspeed",
    "m_flHypeMeter",
    "m_flEnergyDrinkMeter",
    "m_iDecapitations",
    "m_iRevengeCrits",
    "m_hItem",
    "m_bShieldEquipped",
    "m_flHolsterAnimTime",
    "m_hSwitchTo",
    "m_flInspectTime",
    "m_nTeamTeleporterUsed",
    "m_flSpyTranqBuffDuration",
    "m_iKillCountSinceLastDeploy",
    "m_flFirstPrimaryAttack",
    "m_iNextMeleeCrit",
    "m_flRuneCharge",
    "m_bArenaFirstBloodBoost",
    "m_iMovementStunAmount",
    "m_flMovementStunTime",
    "m_hStunner",
    "m_iMovementStunParity",
    "m_nHalloweenBombHeadStage",
    "m_iStunIndex",
    "m_flDuckTimer",
    "m_bLoadoutUnavailable",
];

const NOISY_PROPS: &[&str] = &[
    "m_vecOrigin",
    "m_vecOrigin[2]",
    "m_angEyeAngles[0]",
    "m_angEyeAngles[1]",
    "m_flSimulationTime",
    "m_flAnimTime",
    "m_flCycle",
    "m_vecVelocity[0]",
    "m_vecVelocity[1]",
    "m_vecVelocity[2]",
    "m_angRotation",
    "m_vecBaseVelocity",
];

impl Probe {
    fn pname(&self, id: SendPropIdentifier) -> String {
        match self.names.get(&id) {
            Some(n) => format!("{}.{}", n.table, n.prop),
            None => format!("id:{}", u64::from(id)),
        }
    }

    fn prop_only(&self, id: SendPropIdentifier) -> &str {
        self.names.get(&id).map(|n| n.prop.as_str()).unwrap_or("")
    }

    fn is_handle_prop(&self, id: SendPropIdentifier) -> bool {
        self.names
            .get(&id)
            .map(|n| {
                n.prop.starts_with("m_h")
                    || n.table.starts_with("m_h")
                    || n.prop == "m_pProvider"
                    || n.prop == "m_hOwnerEntity"
            })
            .unwrap_or(false)
    }

    fn is_model_prop(&self, id: SendPropIdentifier) -> bool {
        matches!(
            self.prop_only(id),
            "m_nModelIndex" | "m_iWorldModelIndex" | "m_iViewModelIndex" | "m_nModelIndexOverrides"
        ) || self
            .names
            .get(&id)
            .map(|n| n.table == "m_nModelIndexOverrides")
            .unwrap_or(false)
    }

    fn model_name(&self, index: i64) -> String {
        self.string_tables
            .get("modelprecache")
            .and_then(|t| t.get(index as usize))
            .cloned()
            .unwrap_or_else(|| format!("model#{index}"))
    }

    fn fval(&self, v: &Val) -> String {
        match v {
            Val::I(i) => i.to_string(),
            Val::F(f) => ff(*f),
            Val::V(v) => format!("({},{},{})", ff(v[0]), ff(v[1]), ff(v[2])),
            Val::XY(v) => format!("({},{})", ff(v[0]), ff(v[1])),
            Val::S(s) => {
                let s = self.interner.get(*s);
                if s.len() > 160 {
                    format!(
                        "{}…",
                        &s[..s.char_indices().nth(160).map(|(i, _)| i).unwrap_or(s.len())]
                    )
                } else {
                    s.to_string()
                }
            }
        }
    }

    fn fprop(&self, id: SendPropIdentifier, v: &Val) -> String {
        if let Val::I(i) = v {
            if self.is_handle_prop(id) {
                return fh(*i);
            }
            if self.is_model_prop(id) {
                return format!("{} ({})", i, self.model_name(*i));
            }
        }
        self.fval(v)
    }

    fn class_name_of(&self, class: u16) -> &str {
        self.server_classes
            .get(class as usize)
            .map(|c| c.0.as_str())
            .unwrap_or("?")
    }

    fn class_id_by_name(&self, name: &str) -> Option<u16> {
        self.server_classes
            .iter()
            .position(|c| c.0 == name)
            .map(|i| i as u16)
    }

    fn kind_of(&self, class: u16) -> Kind {
        self.class_info
            .get(class as usize)
            .and_then(|c| c.as_ref())
            .map(|c| c.kind)
            .unwrap_or(Kind::Other)
    }

    fn ensure_class(&mut self, class: u16, ps: &ParserState) {
        let i = class as usize;
        if self.class_info.len() <= i {
            self.class_info.resize(i + 1, None);
        }
        if self.class_info[i].is_some() {
            return;
        }
        let (name, table) = self.server_classes.get(i).cloned().unwrap_or_default();
        let raw_flat: Vec<SendPropIdentifier> = ps
            .send_tables
            .get(i)
            .map(|t| t.flattened_props.iter().map(|p| p.identifier).collect())
            .unwrap_or_default();
        let mut counts: FnvHashMap<SendPropIdentifier, u32> = FnvHashMap::default();
        for pid in &raw_flat {
            *counts.entry(*pid).or_default() += 1;
        }
        let mut dup: FnvHashMap<u32, SendPropIdentifier> = FnvHashMap::default();
        let mut seen: FnvHashMap<SendPropIdentifier, u32> = FnvHashMap::default();
        let mut flat = Vec::with_capacity(raw_flat.len());
        for (fi, pid) in raw_flat.iter().enumerate() {
            if counts.get(pid).copied().unwrap_or(0) > 1 {
                let k = seen.entry(*pid).or_default();
                let syn = SendPropIdentifier::from(
                    u64::from(*pid)
                        .wrapping_add((*k as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
                );
                if let Some(n) = self.names.get(pid) {
                    let pn = PropName {
                        table: n.table.clone(),
                        prop: format!("{}[{}]", n.prop, k),
                        def: n.def.clone(),
                    };
                    self.names.insert(syn, pn);
                }
                dup.insert(fi as u32, syn);
                flat.push(syn);
                *k += 1;
            } else {
                flat.push(*pid);
            }
        }
        let mut by_name: FnvHashMap<String, Vec<SendPropIdentifier>> = FnvHashMap::default();
        for pid in &flat {
            if let Some(n) = self.names.get(pid) {
                by_name.entry(n.prop.clone()).or_default().push(*pid);
            }
        }
        let has = |t: &str, p: &str| flat.contains(&id(t, p));
        let kind = if name == "CTFPlayer" {
            Kind::Player
        } else if name.contains("ViewModel") {
            Kind::ViewModel
        } else if name.contains("Projectile")
            || has("DT_BaseProjectile", "m_hOriginalLauncher")
            || name == "CTFStunBall"
            || name == "CTFBall_Ornament"
        {
            Kind::Projectile
        } else if has("DT_BaseCombatWeapon", "m_iState") {
            Kind::Weapon
        } else if name.contains("Wearable") {
            Kind::Wearable
        } else if name.starts_with("CObject") {
            Kind::Building
        } else {
            Kind::Other
        };
        self.class_info[i] = Some(ClassInfo {
            name,
            table,
            kind,
            flat,
            by_name,
            dup,
        });
    }

    fn get_by_name(&self, ent: &Ent, name: &str) -> Option<Val> {
        let info = self.class_info.get(ent.class as usize)?.as_ref()?;
        let ids = info.by_name.get(name)?;
        ids.iter().find_map(|i| ent.props.get(i).copied())
    }

    fn get(ent: &Ent, table: &str, prop: &str) -> Option<Val> {
        ent.props.get(&id(table, prop)).copied()
    }

    fn log_cap(&self, class: u16, pid: SendPropIdentifier) -> usize {
        let kind = self.kind_of(class);
        let pname = self.prop_only(pid);
        let noisy = NOISY_PROPS.contains(&pname);
        let cname = self.class_name_of(class);
        match kind {
            Kind::Player => {
                if noisy {
                    3000
                } else if IMPORTANT_PLAYER_PROPS.contains(&pname) {
                    2_000_000
                } else {
                    50_000
                }
            }
            Kind::Projectile => {
                if noisy {
                    3000
                } else {
                    50_000
                }
            }
            Kind::ViewModel => 2_000_000,
            Kind::Weapon => {
                if matches!(pname, "m_flAnimTime" | "m_flCycle" | "m_flSimulationTime") {
                    3000
                } else {
                    2_000_000
                }
            }
            _ => {
                if INFO_CLASSES.contains(&cname) || cname == "CSniperDot" {
                    2_000_000
                } else {
                    50_000
                }
            }
        }
    }

    fn log_changes(
        &mut self,
        class: u16,
        ent: u32,
        changed: &[(SendPropIdentifier, Val)],
        init: bool,
    ) {
        for (pid, v) in changed {
            let key = (class, *pid);
            if !self.logs.contains_key(&key) {
                let cap = self.log_cap(class, *pid);
                self.logs.insert(
                    key,
                    PropLog {
                        v: Vec::new(),
                        total: 0,
                        cap,
                    },
                );
            }
            if let Some(log) = self.logs.get_mut(&key) {
                log.total += 1;
                if log.v.len() < log.cap {
                    log.v.push(LogE {
                        tick: self.tick,
                        ent: ent as u16,
                        init,
                        val: *v,
                    });
                }
            }
        }
    }

    // ------------------------------------------------------------------ entities

    fn on_entities(&mut self, msg: &PacketEntitiesMessage, ps: &ParserState) {
        self.entity_messages += 1;
        self.entity_packet_ticks.push(self.tick);
        self.server_ticks_at_entities
            .push((self.tick, self.server_tick));
        if msg.delta.is_none() {
            self.full_updates += 1;
        }
        if self.ents.len() < 2048 {
            self.ents.resize_with(2048, || None);
        }
        for e in &msg.entities {
            match e.update_type {
                UpdateType::Enter => {
                    self.update_counts[0] += 1;
                    self.on_enter(e, ps)
                }
                UpdateType::Delta => {
                    self.update_counts[1] += 1;
                    self.on_delta(e)
                }
                UpdateType::Leave => {
                    self.update_counts[2] += 1;
                    self.on_leave(u32::from(e.entity_index))
                }
                UpdateType::Delete => {
                    self.update_counts[3] += 1;
                    self.on_delete(u32::from(e.entity_index))
                }
            }
        }
        for removed in &msg.removed_entities {
            self.update_counts[3] += 1;
            self.on_delete(u32::from(*removed));
        }
    }

    fn on_enter(&mut self, e: &PacketEntity, ps: &ParserState) {
        let idx = u32::from(e.entity_index);
        let class = u16::from(e.server_class);
        self.ensure_class(class, ps);
        let old = self.ents[idx as usize].take();
        let reenter = matches!(&old, Some(o) if o.serial == e.serial_number && o.class == class);
        let mut ent = match old {
            Some(o) if reenter => o,
            other => {
                if let Some(o) = other {
                    self.end_instance(idx, o);
                }
                self.next_instance += 1;
                Ent {
                    class,
                    serial: e.serial_number,
                    instance: self.next_instance,
                    created: self.tick,
                    props: FnvHashMap::default(),
                }
            }
        };
        let tick = self.tick;
        let mut changed: Vec<(SendPropIdentifier, Val)> = Vec::new();
        {
            let cs = self.class_stats.entry(class).or_default();
            if reenter {
                cs.reenters += 1;
            } else {
                cs.instances += 1;
                cs.alive += 1;
                cs.max_alive = cs.max_alive.max(cs.alive);
            }
            cs.enters += 1;
            let dup = self
                .class_info
                .get(class as usize)
                .and_then(|c| c.as_ref())
                .map(|c| &c.dup);
            let remap = |p: &tf_demo_parser::demo::sendprop::SendProp| -> SendPropIdentifier {
                dup.and_then(|d| d.get(&p.index))
                    .copied()
                    .unwrap_or(p.identifier)
            };
            for p in &e.props {
                cs.props.entry(remap(p)).or_default().enter_sent += 1;
            }
            // own props first, then baseline props, deduplicated by flattened index (the
            // library's PacketEntity::props() dedupes by identifier, which drops repeated
            // UtlVector element props)
            // the library's instance baselines only contain the props of the Enter that created
            // them (not the static baseline underneath), so merge three layers by index:
            // own props > instance-or-static baseline > static baseline
            let baseline = e.get_baseline_props(ps);
            let static_bl: Vec<tf_demo_parser::demo::sendprop::SendProp> = ps
                .send_tables
                .get(class as usize)
                .and_then(|t| ps.get_static_baseline(e.server_class, t).ok())
                .unwrap_or_default();
            let own_idx: FnvHashSet<u32> = e.props.iter().map(|p| p.index).collect();
            let base_idx: FnvHashSet<u32> = baseline.iter().map(|p| p.index).collect();
            let merged = e
                .props
                .iter()
                .chain(baseline.iter().filter(|p| !own_idx.contains(&p.index)))
                .chain(
                    static_bl
                        .iter()
                        .filter(|p| !own_idx.contains(&p.index) && !base_idx.contains(&p.index)),
                );
            for p in merged {
                let pid = remap(p);
                let v = to_val(&p.value, &mut self.interner);
                let prev = ent.props.insert(pid, v);
                let st = cs.props.entry(pid).or_default();
                if !reenter {
                    add_distinct(&mut st.enter_vals, &mut st.enter_overflow, v, 24);
                    changed.push((pid, v));
                } else if prev.map(|pv| !pv.same(&v)).unwrap_or(true) {
                    st.changes += 1;
                    st.ents.insert(idx);
                    if st.first_change.is_none() {
                        st.first_change = Some(tick);
                    }
                    st.last_change = tick;
                    add_distinct(&mut st.distinct, &mut st.overflow, v, 24);
                    if let Some(n) = v.num() {
                        if !st.has_num {
                            st.min = n;
                            st.max = n;
                            st.has_num = true;
                        } else {
                            st.min = st.min.min(n);
                            st.max = st.max.max(n);
                        }
                    }
                    changed.push((pid, v));
                }
            }
        }
        self.lifecycle.push(Life {
            tick,
            ent: idx as u16,
            class,
            serial: ent.serial,
            kind: if reenter { 2 } else { 0 },
        });
        self.log_changes(class, idx, &changed, !reenter);
        if reenter {
            if let Some(m) = self.movers.get_mut(&ent.instance) {
                m.reenters += 1;
            }
            self.after_changes(idx, &ent, &changed);
        } else {
            self.on_created(idx, &ent);
        }
        self.ents[idx as usize] = Some(ent);
    }

    fn on_delta(&mut self, e: &PacketEntity) {
        let idx = u32::from(e.entity_index);
        let Some(mut ent) = self.ents.get_mut(idx as usize).and_then(|x| x.take()) else {
            self.orphan_deltas += 1;
            return;
        };
        let class = ent.class;
        let tick = self.tick;
        let mut changed: Vec<(SendPropIdentifier, Val)> = Vec::new();
        {
            let cs = self.class_stats.entry(class).or_default();
            cs.deltas += 1;
            let dup = self
                .class_info
                .get(class as usize)
                .and_then(|c| c.as_ref())
                .map(|c| &c.dup);
            for p in &e.props {
                let pid = dup
                    .and_then(|d| d.get(&p.index))
                    .copied()
                    .unwrap_or(p.identifier);
                let v = to_val(&p.value, &mut self.interner);
                let prev = ent.props.insert(pid, v);
                let st = cs.props.entry(pid).or_default();
                st.sends += 1;
                if prev.map(|pv| !pv.same(&v)).unwrap_or(true) {
                    st.changes += 1;
                    st.ents.insert(idx);
                    if st.first_change.is_none() {
                        st.first_change = Some(tick);
                    }
                    st.last_change = tick;
                    add_distinct(&mut st.distinct, &mut st.overflow, v, 24);
                    if let Some(n) = v.num() {
                        if !st.has_num {
                            st.min = n;
                            st.max = n;
                            st.has_num = true;
                        } else {
                            st.min = st.min.min(n);
                            st.max = st.max.max(n);
                        }
                    }
                    changed.push((pid, v));
                }
            }
        }
        self.log_changes(class, idx, &changed, false);
        self.after_changes(idx, &ent, &changed);
        self.ents[idx as usize] = Some(ent);
    }

    fn on_leave(&mut self, idx: u32) {
        let tick = self.tick;
        if let Some(Some(ent)) = self.ents.get(idx as usize) {
            let class = ent.class;
            let serial = ent.serial;
            let instance = ent.instance;
            self.class_stats.entry(class).or_default().leaves += 1;
            if let Some(m) = self.movers.get_mut(&instance) {
                m.leaves += 1;
            }
            self.lifecycle.push(Life {
                tick,
                ent: idx as u16,
                class,
                serial,
                kind: 1,
            });
        }
    }

    fn on_delete(&mut self, idx: u32) {
        if let Some(ent) = self.ents.get_mut(idx as usize).and_then(|x| x.take()) {
            self.lifecycle.push(Life {
                tick: self.tick,
                ent: idx as u16,
                class: ent.class,
                serial: ent.serial,
                kind: 3,
            });
            self.end_instance(idx, ent);
        }
    }

    fn end_instance(&mut self, idx: u32, ent: Ent) {
        let tick = self.tick;
        let cs = self.class_stats.entry(ent.class).or_default();
        cs.deletes += 1;
        cs.alive -= 1;
        cs.lifetime_ticks += (tick.saturating_sub(ent.created)) as u64;
        if let Some(pi) = self.proj_by_ent.get(&idx).copied() {
            if let Some(p) = self.projectiles.get_mut(pi) {
                if p.serial == ent.serial && p.deleted.is_none() {
                    p.deleted = Some(tick);
                }
            }
            self.proj_by_ent.remove(&idx);
        }
        if let Some(di) = self.dot_by_ent.get(&idx).copied() {
            if let Some(d) = self.dots.get_mut(di) {
                if d.deleted.is_none() {
                    d.deleted = Some(tick);
                }
            }
            self.dot_by_ent.remove(&idx);
        }
        if let Some(w) = self.weapons.get_mut(&(idx, ent.serial)) {
            if w.deleted.is_none() {
                w.deleted = Some(tick);
            }
        }
        if let Some(m) = self.movers.get_mut(&ent.instance) {
            m.deleted = Some(tick);
        }
    }

    fn on_created(&mut self, idx: u32, ent: &Ent) {
        let tick = self.tick;
        let kind = self.kind_of(ent.class);
        let cname = self.class_name_of(ent.class).to_string();
        let model = self
            .get_by_name(ent, "m_nModelIndex")
            .and_then(|v| v.int())
            .map(|m| self.model_name(m))
            .unwrap_or_default();
        *self
            .class_stats
            .entry(ent.class)
            .or_default()
            .models
            .entry(model.clone())
            .or_default() += 1;

        match kind {
            Kind::Projectile => {
                let geth = |_p: &Probe, t: &str, n: &str| {
                    Probe::get(ent, t, n)
                        .and_then(|v| v.int())
                        .unwrap_or(INVALID_EHANDLE)
                };
                let origin = Probe::get(ent, "DT_TFBaseRocket", "m_vecOrigin")
                    .or_else(|| Probe::get(ent, "DT_TFWeaponBaseGrenadeProj", "m_vecOrigin"))
                    .or_else(|| Probe::get(ent, "DT_BaseEntity", "m_vecOrigin"))
                    .and_then(|v| v.vec3());
                let info = ProjInfo {
                    class: ent.class,
                    ent: idx,
                    serial: ent.serial,
                    created: tick,
                    deleted: None,
                    owner: geth(self, "DT_BaseEntity", "m_hOwnerEntity"),
                    thrower: self
                        .get_by_name(ent, "m_hThrower")
                        .and_then(|v| v.int())
                        .unwrap_or(INVALID_EHANDLE),
                    launcher: self
                        .get_by_name(ent, "m_hLauncher")
                        .and_then(|v| v.int())
                        .unwrap_or(INVALID_EHANDLE),
                    orig_launcher: geth(self, "DT_BaseProjectile", "m_hOriginalLauncher"),
                    deflected_owner: self
                        .get_by_name(ent, "m_hDeflectOwner")
                        .and_then(|v| v.int())
                        .unwrap_or(INVALID_EHANDLE),
                    pipe_type: Probe::get(ent, "DT_TFProjectile_Pipebomb", "m_iType")
                        .and_then(|v| v.int()),
                    proj_type: self
                        .get_by_name(ent, "m_iProjectileType")
                        .and_then(|v| v.int()),
                    crit: self.get_by_name(ent, "m_bCritical").and_then(|v| v.int()),
                    deflected_max: self
                        .get_by_name(ent, "m_iDeflected")
                        .and_then(|v| v.int())
                        .unwrap_or(0),
                    deflect_changes: 0,
                    model,
                    team: Probe::get(ent, "DT_BaseEntity", "m_iTeamNum")
                        .and_then(|v| v.int())
                        .unwrap_or(0),
                    origin,
                };
                self.proj_by_ent.insert(idx, self.projectiles.len());
                self.projectiles.push(info);
            }
            Kind::Weapon => {
                let itemdef = Probe::get(ent, "DT_ScriptCreatedItem", "m_iItemDefinitionIndex")
                    .and_then(|v| v.int())
                    .unwrap_or(-1);
                let owner = Probe::get(ent, "DT_BaseCombatWeapon", "m_hOwner")
                    .and_then(|v| v.int())
                    .unwrap_or(INVALID_EHANDLE);
                let owner_entity = Probe::get(ent, "DT_BaseEntity", "m_hOwnerEntity")
                    .and_then(|v| v.int())
                    .unwrap_or(INVALID_EHANDLE);
                self.weapons.insert(
                    (idx, ent.serial),
                    WeaponInfo {
                        class: ent.class,
                        itemdef,
                        owner,
                        owner_entity,
                        created: tick,
                        deleted: None,
                    },
                );
            }
            Kind::Other | Kind::Building => {
                if cname == "CSniperDot" {
                    let d = DotInfo {
                        ent: idx,
                        created: tick,
                        deleted: None,
                        owner: Probe::get(ent, "DT_BaseEntity", "m_hOwnerEntity")
                            .and_then(|v| v.int())
                            .unwrap_or(INVALID_EHANDLE),
                        charge_start: self
                            .get_by_name(ent, "m_flChargeStartTime")
                            .and_then(|v| v.float())
                            .unwrap_or(0.0),
                        origin: Probe::get(ent, "DT_BaseEntity", "m_vecOrigin")
                            .and_then(|v| v.vec3()),
                        origin_changes: 0,
                        sim_time: Probe::get(ent, "DT_BaseEntity", "m_flSimulationTime")
                            .and_then(|v| v.float())
                            .unwrap_or(0.0),
                    };
                    self.dot_by_ent.insert(idx, self.dots.len());
                    self.dots.push(d);
                }
                let origin = Probe::get(ent, "DT_BaseEntity", "m_vecOrigin").and_then(|v| v.vec3());
                let m = MoverStat {
                    class: ent.class,
                    ent: idx,
                    serial: ent.serial,
                    created: tick,
                    model: model.clone(),
                    parent: Probe::get(ent, "DT_BaseEntity", "moveparent")
                        .or_else(|| Probe::get(ent, "DT_BaseEntity", "m_hMoveParent"))
                        .and_then(|v| v.int())
                        .unwrap_or(INVALID_EHANDLE),
                    first_origin: origin,
                    min_o: origin.unwrap_or([0.0; 3]),
                    max_o: origin.unwrap_or([0.0; 3]),
                    effects_vals: Probe::get(ent, "DT_BaseEntity", "m_fEffects")
                        .and_then(|v| v.int())
                        .into_iter()
                        .collect(),
                    ..MoverStat::default()
                };
                self.movers.insert(ent.instance, m);
            }
            _ => {}
        }

        if INFO_CLASSES.contains(&cname.as_str())
            && self.info_dumps.iter().filter(|d| d.class == cname).count() < 8
        {
            let props: Vec<(SendPropIdentifier, Val)> =
                ent.props.iter().map(|(p, v)| (*p, *v)).collect();
            self.info_dumps.push(InfoDump {
                class: cname,
                ent: idx,
                serial: ent.serial,
                tick,
                props,
            });
        }
    }

    fn after_changes(&mut self, idx: u32, ent: &Ent, changed: &[(SendPropIdentifier, Val)]) {
        if changed.is_empty() {
            return;
        }
        let tick = self.tick;
        let kind = self.kind_of(ent.class);
        match kind {
            Kind::Projectile => {
                for (pid, v) in changed {
                    if self.prop_only(*pid) == "m_iDeflected" {
                        if let Some(pi) = self.proj_by_ent.get(&idx).copied() {
                            if let Some(p) = self.projectiles.get_mut(pi) {
                                p.deflect_changes += 1;
                                p.deflected_max = p.deflected_max.max(v.int().unwrap_or(0));
                            }
                        }
                    }
                }
            }
            Kind::Other | Kind::Building => {
                let names: Vec<(String, String)> = changed
                    .iter()
                    .map(|(pid, _)| {
                        self.names
                            .get(pid)
                            .map(|n| (n.table.clone(), n.prop.clone()))
                            .unwrap_or_default()
                    })
                    .collect();
                let is_dot = self.class_name_of(ent.class) == "CSniperDot";
                if let Some(m) = self.movers.get_mut(&ent.instance) {
                    let mut any = false;
                    for ((pid, v), (table, prop)) in changed.iter().zip(names.iter()) {
                        let _ = pid;
                        if prop.starts_with("m_vecOrigin") {
                            m.origin_changes += 1;
                            any = true;
                            if let Some(o) = v.vec3() {
                                if prop == "m_vecOrigin" {
                                    for k in 0..3 {
                                        m.min_o[k] = m.min_o[k].min(o[k]);
                                        m.max_o[k] = m.max_o[k].max(o[k]);
                                    }
                                }
                            }
                        } else if prop.starts_with("m_angRotation") {
                            m.angle_changes += 1;
                            any = true;
                        } else if prop == "m_fEffects" && table == "DT_BaseEntity" {
                            m.effects_changes += 1;
                            if let Some(i) = v.int() {
                                m.effects_vals.insert(i);
                            }
                            any = true;
                        } else if prop == "m_nRenderMode" {
                            m.render_changes += 1;
                            any = true;
                        } else if prop == "m_clrRender" {
                            m.color_changes += 1;
                            any = true;
                        } else if prop == "m_nModelIndex" {
                            m.model_changes += 1;
                            any = true;
                        } else if prop == "m_hMoveParent" || prop == "moveparent" {
                            m.parent_changes += 1;
                            m.parent = v.int().unwrap_or(INVALID_EHANDLE);
                            any = true;
                        }
                    }
                    if any {
                        if m.first_change.is_none() {
                            m.first_change = Some(tick);
                        }
                        m.last_change = tick;
                    }
                }
                if is_dot {
                    if let Some(di) = self.dot_by_ent.get(&idx).copied() {
                        if let Some(d) = self.dots.get_mut(di) {
                            for (_, prop) in names.iter() {
                                if prop.starts_with("m_vecOrigin") {
                                    d.origin_changes += 1;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ temp entities

    fn on_temp_entities(&mut self, msg: &TempEntitiesMessage, ps: &ParserState) {
        self.te_msgs += 1;
        let mut prev: Option<(u16, FnvHashMap<SendPropIdentifier, Val>)> = None;
        for ev in &msg.events {
            let class = u16::from(ev.class_id);
            self.ensure_class(class, ps);
            let mut merged = match prev.take() {
                Some((c, m)) if c == class => m,
                _ => FnvHashMap::default(),
            };
            for p in &ev.props {
                merged.insert(p.identifier, to_val(&p.value, &mut self.interner));
            }
            // a full temp entity update is a delta against zero: absent props are 0
            if let Some(Some(info)) = self.class_info.get(class as usize) {
                for pid in &info.flat {
                    if !merged.contains_key(pid) {
                        let def = self.names.get(pid).map(|n| n.def.as_str()).unwrap_or("int");
                        let z = if def.starts_with("float") {
                            Val::F(0.0)
                        } else if def.starts_with("vectorXY") {
                            Val::XY([0.0; 2])
                        } else if def.starts_with("vector") {
                            Val::V([0.0; 3])
                        } else if def.starts_with("string") || def.starts_with("array") {
                            Val::S(self.interner.id(""))
                        } else {
                            Val::I(0)
                        };
                        merged.insert(*pid, z);
                    }
                }
            }
            let sent: Vec<SendPropIdentifier> = ev.props.iter().map(|p| p.identifier).collect();
            self.handle_te(class, &merged, &sent, ev.reliable, ev.fire_delay);
            prev = Some((class, merged));
        }
    }

    fn handle_te(
        &mut self,
        class: u16,
        merged: &FnvHashMap<SendPropIdentifier, Val>,
        sent: &[SendPropIdentifier],
        reliable: bool,
        delay: f32,
    ) {
        let empty = sent.is_empty();
        self.te_total += 1;
        let tick = self.tick;
        let cname = self.class_name_of(class).to_string();
        let sample = {
            let st = self.te_stats.get(&class);
            if st.map(|s| s.samples.len() < 8).unwrap_or(true) {
                let mut parts: Vec<String> = merged
                    .iter()
                    .map(|(pid, v)| format!("{}={}", self.prop_only(*pid), self.fprop(*pid, v)))
                    .collect();
                parts.sort();
                Some(format!("tick {} {}", tick, parts.join(" ")))
            } else {
                None
            }
        };
        {
            let st = self.te_stats.entry(class).or_default();
            st.count += 1;
            if st.first.is_none() {
                st.first = Some(tick);
            }
            if reliable {
                st.reliable += 1;
            }
            if delay > 0.0 {
                st.delayed += 1;
            }
            if empty {
                st.empty += 1;
            }
            for (pid, v) in merged {
                let e = st.props.entry(*pid).or_default();
                if sent.contains(pid) {
                    e.0 += 1;
                }
                add_distinct(&mut e.1, &mut e.2, *v, 40);
            }
            if let Some(s) = sample {
                st.samples.push(s);
            }
        }
        let g = |t: &str, p: &str| merged.get(&id(t, p)).copied();
        match cname.as_str() {
            "CTEFireBullets" => {
                let t = "DT_TEFireBullets";
                let player = g(t, "m_iPlayer").and_then(|v| v.int()).unwrap_or(0);
                let origin = g(t, "m_vecOrigin")
                    .and_then(|v| v.vec3())
                    .unwrap_or([0.0; 3]);
                let a0 = g(t, "m_vecAngles[0]")
                    .and_then(|v| v.float())
                    .unwrap_or(0.0);
                let a1 = g(t, "m_vecAngles[1]")
                    .and_then(|v| v.float())
                    .unwrap_or(0.0);
                let pent = (player + 1) as usize;
                let (p_origin, p_ang, p_class) = match self.ents.get(pent).and_then(|e| e.as_ref())
                {
                    Some(e) => {
                        let xy = Probe::get(e, "DT_TFNonLocalPlayerExclusive", "m_vecOrigin")
                            .and_then(|v| v.vec3());
                        let z = Probe::get(e, "DT_TFNonLocalPlayerExclusive", "m_vecOrigin[2]")
                            .and_then(|v| v.float());
                        let pitch =
                            Probe::get(e, "DT_TFNonLocalPlayerExclusive", "m_angEyeAngles[0]")
                                .and_then(|v| v.float());
                        let yaw =
                            Probe::get(e, "DT_TFNonLocalPlayerExclusive", "m_angEyeAngles[1]")
                                .and_then(|v| v.float());
                        let class = Probe::get(e, "DT_TFPlayerClassShared", "m_iClass")
                            .and_then(|v| v.int())
                            .unwrap_or(0);
                        (
                            match (xy, z) {
                                (Some(xy), Some(z)) => Some([xy[0], xy[1], z]),
                                _ => None,
                            },
                            match (pitch, yaw) {
                                (Some(p), Some(y)) => Some([p, y]),
                                _ => None,
                            },
                            class,
                        )
                    }
                    None => (None, None, 0),
                };
                self.fire_bullets.push(FireBullet {
                    tick,
                    player,
                    weapon: g(t, "m_iWeaponID").and_then(|v| v.int()).unwrap_or(0),
                    mode: g(t, "m_iMode").and_then(|v| v.int()).unwrap_or(0),
                    seed: g(t, "m_iSeed").and_then(|v| v.int()).unwrap_or(0),
                    spread: g(t, "m_flSpread").and_then(|v| v.float()).unwrap_or(0.0),
                    crit: g(t, "m_bCritical").and_then(|v| v.int()).unwrap_or(0),
                    origin,
                    ang: [a0, a1],
                    p_origin,
                    p_ang,
                    p_class,
                });
            }
            "CTEEffectDispatch" => {
                let t = "DT_EffectData";
                let en = g(t, "m_iEffectName").and_then(|v| v.int()).unwrap_or(0);
                let name = self
                    .string_tables
                    .get("EffectDispatch")
                    .and_then(|tb| tb.get(en as usize))
                    .cloned()
                    .unwrap_or_else(|| format!("effect#{en}"));
                let key = if name == "ParticleEffect" {
                    let pi = g(t, "m_nHitBox").and_then(|v| v.int()).unwrap_or(0);
                    let pn = self
                        .string_tables
                        .get("ParticleEffectNames")
                        .and_then(|tb| tb.get(pi as usize))
                        .cloned()
                        .unwrap_or_else(|| format!("particle#{pi}"));
                    let ei = g(t, "entindex").and_then(|v| v.int()).unwrap_or(0);
                    let ek = if (1..=101).contains(&ei) {
                        "player"
                    } else if ei == 0 {
                        "world/none"
                    } else {
                        "entity"
                    };
                    format!("ParticleEffect:{pn} (entindex {ek})")
                } else {
                    name
                };
                *self.effect_dispatch.entry(key).or_default() += 1;
            }
            "CTEPlayerAnimEvent" => {
                let t = "DT_TEPlayerAnimEvent";
                let h = g(t, "m_hPlayer").and_then(|v| v.int()).unwrap_or(0);
                self.anim_events.push(AnimEv {
                    tick,
                    player: h_index(h).map(|i| i as i64).unwrap_or(-1),
                    event: g(t, "m_iEvent").and_then(|v| v.int()).unwrap_or(0),
                    data: g(t, "m_nData").and_then(|v| v.int()).unwrap_or(0),
                });
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ game events

    fn on_game_event(&mut self, msg: &GameEventMessage, ps: &ParserState) {
        let name = msg.event_type.as_str().to_string();
        let def_names: Vec<String> = ps
            .event_definitions
            .get(usize::from(msg.event_type_id))
            .map(|d| d.entries.iter().map(|e| e.name.clone()).collect())
            .unwrap_or_default();
        if !self.ev_defs.contains_key(&name) {
            if let Some(def) = ps.event_definitions.get(usize::from(msg.event_type_id)) {
                let entries = def
                    .entries
                    .iter()
                    .map(|e| format!("{}:{}", e.name, e.kind))
                    .collect();
                self.ev_defs.insert(name.clone(), entries);
            }
        }
        let json = match &msg.event {
            GameEvent::Unknown(raw) => {
                let mut obj = serde_json::Map::new();
                for (i, v) in raw.values.iter().enumerate() {
                    let k = def_names.get(i).cloned().unwrap_or_else(|| format!("f{i}"));
                    let jv = match v {
                        GameEventValue::String(s) => serde_json::Value::from(s.to_string()),
                        GameEventValue::Float(f) => serde_json::Value::from(*f),
                        GameEventValue::Long(l) => serde_json::Value::from(*l),
                        GameEventValue::Short(s) => serde_json::Value::from(*s),
                        GameEventValue::Byte(b) => serde_json::Value::from(*b),
                        GameEventValue::Boolean(b) => serde_json::Value::from(*b),
                        GameEventValue::Local => serde_json::Value::Null,
                    };
                    obj.insert(k, jv);
                }
                serde_json::Value::Object(obj)
            }
            ev => serde_json::to_value(ev).unwrap_or(serde_json::Value::Null),
        };
        let tick = self.tick;
        let st = self.ev_stats.entry(name.clone()).or_default();
        if st.count == 0 {
            st.first = tick;
        }
        st.count += 1;
        st.last = tick;
        if let serde_json::Value::Object(obj) = &json {
            for (k, v) in obj {
                if k == "type" {
                    continue;
                }
                let e = st.fields.entry(k.clone()).or_default();
                let s = v.to_string();
                if let Some(x) = e.0.iter_mut().find(|(x, _)| *x == s) {
                    x.1 += 1;
                } else if e.0.len() < 16 {
                    e.0.push((s, 1));
                } else {
                    e.1 = true;
                }
            }
        }
        let compact = json.to_string();
        if st.samples.len() < 3 {
            st.samples.push(format!("tick {tick}: {compact}"));
        }
        if self.ev_log.len() < 300_000 {
            self.ev_log.push((tick, name, compact));
        }
    }
}

impl MessageHandler for Probe {
    type Output = Probe;

    fn does_handle(message_type: MessageType) -> bool {
        matches!(
            message_type,
            MessageType::PacketEntities
                | MessageType::GameEvent
                | MessageType::ServerInfo
                | MessageType::TempEntities
                | MessageType::NetTick
                | MessageType::UserMessage
                | MessageType::SetConVar
        )
    }

    fn handle_message(&mut self, message: &Message, tick: DemoTick, ps: &ParserState) {
        let mt = message.get_message_type();
        *self.msg_counts.entry(format!("{mt:?}")).or_default() += 1;
        if GameStateAnalyser::does_handle(mt) {
            self.gs.handle_message(message, tick, ps);
        }
        match message {
            Message::PacketEntities(m) => self.on_entities(m, ps),
            Message::TempEntities(m) => self.on_temp_entities(m, ps),
            Message::GameEvent(m) => self.on_game_event(m, ps),
            Message::NetTick(m) => {
                self.server_tick = u32::from(m.tick);
                if self.first_server_tick.is_none() && self.server_tick > 0 {
                    self.first_server_tick = Some(self.server_tick);
                }
            }
            Message::ServerInfo(m) => {
                self.interval = m.interval_per_tick;
                self.server_info = format!(
                    "map={} server={:?} stv={} dedicated={} max_players={} interval_per_tick={} max_classes={} platform={} game={} skybox={} version={} protocol-ish server_count={}",
                    m.map, m.server_name, m.stv, m.dedicated, m.max_player_count, m.interval_per_tick, m.max_classes, m.platform, m.game, m.skybox, m.version, m.server_count
                );
            }
            Message::UserMessage(m) => {
                *self.user_msgs.entry(m.message_type()).or_default() += 1;
            }
            Message::SetConVar(m) => {
                for v in &m.vars {
                    self.convars
                        .push((self.tick, v.key.clone(), v.value.clone()));
                }
            }
            _ => {}
        }
    }

    fn handle_string_entry(
        &mut self,
        table: &str,
        index: usize,
        entry: &StringTableEntry,
        ps: &ParserState,
    ) {
        self.gs.handle_string_entry(table, index, entry, ps);
        let t = self.string_tables.entry(table.to_string()).or_default();
        if t.len() <= index {
            t.resize(index + 1, String::new());
        }
        if let Some(text) = &entry.text {
            t[index] = text.to_string();
        }
    }

    fn handle_data_tables(
        &mut self,
        tables: &[ParseSendTable],
        server_classes: &[ServerClass],
        ps: &ParserState,
    ) {
        self.gs.handle_data_tables(tables, server_classes, ps);
        for t in tables {
            for p in &t.props {
                self.names.insert(
                    p.identifier(),
                    PropName {
                        table: t.name.to_string(),
                        prop: p.name.as_str().to_string(),
                        def: def_str(p),
                    },
                );
            }
        }
        self.server_classes = server_classes
            .iter()
            .map(|c| (c.name.to_string(), c.data_table.to_string()))
            .collect();
        self.class_info = vec![None; server_classes.len()];
    }

    fn handle_packet_meta(&mut self, tick: DemoTick, meta: &MessagePacketMeta, ps: &ParserState) {
        self.gs.handle_packet_meta(tick, meta, ps);
        self.tick = u32::from(tick);
        if self.first_tick.is_none() {
            self.first_tick = Some(self.tick);
        }
        self.last_tick = self.last_tick.max(self.tick);
    }

    fn into_output(mut self, state: &ParserState) -> Self::Output {
        for c in 0..self.server_classes.len() {
            self.ensure_class(c as u16, state);
        }
        for d in &state.event_definitions {
            self.all_ev_defs.insert(
                d.event_type.as_str().to_string(),
                d.entries
                    .iter()
                    .map(|e| format!("{}:{}", e.name, e.kind))
                    .collect(),
            );
        }
        // close lifetimes of entities alive at the end
        let last = self
            .entity_packet_ticks
            .last()
            .copied()
            .unwrap_or(self.tick);
        for e in self.ents.iter().flatten() {
            let cs = self.class_stats.entry(e.class).or_default();
            cs.lifetime_ticks += last.saturating_sub(e.created) as u64;
        }
        self
    }
}

// ---------------------------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------------------------

struct Timeline {
    // per player entity: sorted (tick, value)
    by_ent: BTreeMap<u32, Vec<(u32, Val)>>,
}

impl Timeline {
    fn at(&self, ent: u32, tick: u32) -> Option<Val> {
        let v = self.by_ent.get(&ent)?;
        let i = v.partition_point(|(t, _)| *t <= tick);
        if i == 0 {
            None
        } else {
            Some(v[i - 1].1)
        }
    }
}

impl Probe {
    fn timeline(&self, class: u16, pid: SendPropIdentifier) -> Timeline {
        let mut by_ent: BTreeMap<u32, Vec<(u32, Val)>> = BTreeMap::new();
        if let Some(log) = self.logs.get(&(class, pid)) {
            for e in &log.v {
                by_ent
                    .entry(e.ent as u32)
                    .or_default()
                    .push((e.tick, e.val));
            }
        }
        Timeline { by_ent }
    }

    fn log_of(&self, class: u16, pid: SendPropIdentifier) -> Option<&PropLog> {
        self.logs.get(&(class, pid))
    }

    fn player_names(&self) -> BTreeMap<u32, String> {
        let mut m = BTreeMap::new();
        for p in &self.gs.state.players {
            if let Some(info) = &p.info {
                m.insert(u32::from(p.entity), info.name.clone());
            }
        }
        m
    }

    fn secs(&self, ticks: u32) -> f32 {
        ticks as f32
            * if self.interval > 0.0 {
                self.interval
            } else {
                0.015
            }
    }

    fn base_tick(&self) -> u32 {
        self.entity_packet_ticks.first().copied().unwrap_or(0)
    }

    fn rel(&self, tick: u32) -> String {
        let base = self.base_tick();
        format!("{}(+{:.2}s)", tick, self.secs(tick.saturating_sub(base)))
    }

    /// merged changes of one entity instance between t0..=t1 for all props of its class
    fn entity_log(
        &self,
        class: u16,
        ent: u32,
        t0: u32,
        t1: u32,
        skip: &dyn Fn(&str) -> bool,
        with_init: bool,
    ) -> Vec<(u32, SendPropIdentifier, Val)> {
        let mut out = Vec::new();
        for ((c, pid), log) in &self.logs {
            if *c != class {
                continue;
            }
            if skip(self.prop_only(*pid)) {
                continue;
            }
            for e in &log.v {
                if e.ent as u32 == ent && e.tick >= t0 && e.tick <= t1 && (with_init || !e.init) {
                    out.push((e.tick, *pid, e.val));
                }
            }
        }
        out.sort_by_key(|(t, pid, _)| (*t, self.pname(*pid)));
        out
    }

    fn entity_changes(
        &self,
        class: u16,
        ent: u32,
        t0: u32,
        t1: u32,
        skip: &dyn Fn(&str) -> bool,
    ) -> Vec<(u32, SendPropIdentifier, Val)> {
        self.entity_log(class, ent, t0, t1, skip, false)
    }

    /// one-line summary of the change statistics of a prop of a class
    fn stat_line(&self, class: u16, pid: SendPropIdentifier) -> String {
        let def = self.names.get(&pid).map(|n| n.def.as_str()).unwrap_or("");
        let Some(st) = self.class_stats.get(&class).and_then(|c| c.props.get(&pid)) else {
            return format!("{} [{}] never sent", self.pname(pid), def);
        };
        let vals = if st.changes == 0 {
            "-".to_string()
        } else if st.overflow {
            if st.has_num {
                format!("{}..{} (many)", ff(st.min as f32), ff(st.max as f32))
            } else {
                "(many)".into()
            }
        } else {
            let mut v: Vec<&(Val, u64)> = st.distinct.iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.iter()
                .take(14)
                .map(|(x, n)| format!("{}×{}", self.fprop(pid, x), n))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let enter = if st.enter_overflow {
            "(many)".to_string()
        } else {
            let mut v: Vec<&(Val, u64)> = st.enter_vals.iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.iter()
                .take(5)
                .map(|(x, n)| format!("{}×{}", self.fprop(pid, x), n))
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "{} [{}] changes={} ents={} | {} | at creation: {}",
            self.pname(pid),
            def,
            st.changes,
            st.ents.len(),
            vals,
            enter
        )
    }

    /// summaries of all flattened props of `class` whose prop name (or array table name) is `pn`
    fn props_named_summary(&self, class: u16, pn: &str) -> String {
        let Some(Some(info)) = self.class_info.get(class as usize) else {
            return "class unknown".into();
        };
        let ids: Vec<SendPropIdentifier> = info
            .flat
            .iter()
            .copied()
            .filter(|p| {
                self.names
                    .get(p)
                    .map(|n| n.prop == pn || n.table == pn || n.prop.starts_with(&format!("{pn}[")))
                    .unwrap_or(false)
            })
            .collect();
        if ids.is_empty() {
            return "NOT IN SEND TABLE".into();
        }
        let mut lines: Vec<String> = ids
            .iter()
            .filter(|p| {
                ids.len() <= 3
                    || self
                        .class_stats
                        .get(&class)
                        .and_then(|c| c.props.get(*p))
                        .map(|s| s.changes > 0)
                        .unwrap_or(false)
            })
            .map(|p| self.stat_line(class, *p))
            .collect();
        if lines.is_empty() {
            lines.push(format!("{} elements, none ever changed", ids.len()));
        }
        lines.join(" || ")
    }

    /// tick at which the entity instance (index, serial) created at `created` ended
    fn instance_end(&self, ent: u32, serial: u32, created: u32) -> u32 {
        self.lifecycle
            .iter()
            .find(|l| {
                l.ent as u32 == ent
                    && l.tick >= created
                    && ((l.kind == 3 && l.serial == serial) || (l.kind == 0 && l.tick > created))
            })
            .map(|l| l.tick)
            .unwrap_or(u32::MAX)
    }

    fn weapon_label(&self, h: i64) -> String {
        match h_index(h) {
            None => "null".into(),
            Some(i) => match self.weapons.get(&(i, h_serial(h))) {
                Some(w) => format!("{}[def {}]#{}", self.class_name_of(w.class), w.itemdef, i),
                None => format!("#{}/s{}(not a weapon)", i, h_serial(h)),
            },
        }
    }

    fn report(&self, header: &Header, elapsed: Duration) -> String {
        let mut o = String::new();
        let names = self.player_names();
        let pname = |e: u32| names.get(&e).cloned().unwrap_or_else(|| format!("ent{e}"));

        // ---------------------------------------------------------------- 0 meta
        w!(o, "# hl_probe report");
        w!(o, "header: map={} server={:?} nick={:?} duration={:.1}s ticks={} frames={} signon={} protocol={} version={} demo_type={}",
            header.map, header.server, header.nick, header.duration, header.ticks, header.frames, header.signon, header.protocol, header.version, header.demo_type);
        w!(o, "serverinfo: {}", self.server_info);
        w!(o, "demo ticks (packet meta): first={:?} max={} | entity-message demo ticks {:?}..{:?} | first server tick={:?} last server tick={}", self.first_tick, self.last_tick, self.entity_packet_ticks.first(), self.entity_packet_ticks.last(), self.first_server_tick, self.server_tick);
        w!(
            o,
            "probe parse+analysis time: {:.2}s | memory: {}",
            elapsed.as_secs_f32(),
            vm_hwm()
        );
        w!(o, "entity updates: enter={} delta={} leave={} delete(incl. explicit)={} | packet-entities msgs={} full updates={} orphan deltas={}",
            self.update_counts[0], self.update_counts[1], self.update_counts[2], self.update_counts[3], self.entity_messages, self.full_updates, self.orphan_deltas);
        {
            let mut gaps: BTreeMap<u32, u64> = BTreeMap::new();
            for w2 in self.entity_packet_ticks.windows(2) {
                *gaps.entry(w2[1].saturating_sub(w2[0])).or_default() += 1;
            }
            w!(
                o,
                "demo-tick gap between consecutive PacketEntities messages (gap: count): {:?}",
                gaps.iter().take(12).collect::<Vec<_>>()
            );
            let mut sgaps: BTreeMap<i64, u64> = BTreeMap::new();
            for w2 in self.server_ticks_at_entities.windows(2) {
                *sgaps.entry(w2[1].1 as i64 - w2[0].1 as i64).or_default() += 1;
            }
            w!(
                o,
                "server-tick gap between consecutive PacketEntities messages: {:?}",
                sgaps.iter().take(12).collect::<Vec<_>>()
            );
            if let (Some(a), Some(b)) = (
                self.server_ticks_at_entities.first(),
                self.server_ticks_at_entities.last(),
            ) {
                w!(
                    o,
                    "server_tick - demo_tick: at first entities msg={} at last={}",
                    a.1 as i64 - a.0 as i64,
                    b.1 as i64 - b.0 as i64
                );
            }
        }
        w!(o, "message counts: {:?}", self.msg_counts);
        w!(o, "string tables:");
        for (n, t) in &self.string_tables {
            let non_empty = t.iter().filter(|s| !s.is_empty()).count();
            w!(o, "  {n}: {} slots ({} with text)", t.len(), non_empty);
        }
        if !self.convars.is_empty() {
            w!(o, "SetConVar ({}):", self.convars.len());
            for (t, k, v) in self.convars.iter().take(80) {
                w!(o, "  tick {t}: {k} = {v:?}");
            }
        }
        w!(o, "user messages:");
        for (t, c) in &self.user_msgs {
            w!(
                o,
                "  {} ({}): {}",
                USER_MSG_NAMES.get(*t as usize).copied().unwrap_or("?"),
                t,
                c
            );
        }

        // ---------------------------------------------------------------- 1 classes
        let _total_secs = self.secs(self.last_tick.saturating_sub(self.first_tick.unwrap_or(0)));
        w!(o);
        w!(
            o,
            "## 1. Server classes ({} total)",
            self.server_classes.len()
        );
        w!(o, "id name (table) kind | instances maxAlive enters reenters leaves deletes deltas | deltas per instance-second | models");
        let mut not_inst = Vec::new();
        for (i, (n, t)) in self.server_classes.iter().enumerate() {
            let Some(cs) = self.class_stats.get(&(i as u16)) else {
                not_inst.push(n.clone());
                continue;
            };
            let inst_secs = self.secs(cs.lifetime_ticks as u32).max(0.001);
            let mut models: Vec<_> = cs.models.iter().collect();
            models.sort_by(|a, b| b.1.cmp(a.1));
            let models: Vec<String> = models
                .iter()
                .take(6)
                .map(|(m, c)| format!("{m}×{c}"))
                .collect();
            w!(
                o,
                "{i} {n} ({t}) {:?} | {} {} {} {} {} {} {} | {:.2} | {}",
                self.kind_of(i as u16),
                cs.instances,
                cs.max_alive,
                cs.enters,
                cs.reenters,
                cs.leaves,
                cs.deletes,
                cs.deltas,
                cs.deltas as f32 / inst_secs,
                models.join(", ")
            );
        }
        w!(
            o,
            "not instantiated ({}): {}",
            not_inst.len(),
            not_inst.join(", ")
        );

        // ---------------------------------------------------------------- 2 per-class props
        w!(o);
        w!(
            o,
            "## 2. Props that change after creation, per instantiated class"
        );
        w!(o, "format: table.prop [def] chg=changes ents=entities-with-change sends=delta-sends rate=changes/instance-s first=tick | values (×count) or min..max | enter: values at creation");
        let mut class_ids: Vec<u16> = self.class_stats.keys().copied().collect();
        class_ids.sort_by_key(|c| self.class_name_of(*c).to_string());
        for c in class_ids {
            let cs = &self.class_stats[&c];
            let inst_secs = self.secs(cs.lifetime_ticks as u32).max(0.001);
            w!(o);
            w!(
                o,
                "### {} (class {}, {} instances, {:.0} instance-seconds)",
                self.class_name_of(c),
                c,
                cs.instances,
                inst_secs
            );
            let mut props: Vec<(&SendPropIdentifier, &PropStat)> =
                cs.props.iter().filter(|(_, s)| s.changes > 0).collect();
            props.sort_by(|a, b| {
                b.1.changes
                    .cmp(&a.1.changes)
                    .then_with(|| self.pname(*a.0).cmp(&self.pname(*b.0)))
            });
            for (pid, st) in props {
                let def = self.names.get(pid).map(|n| n.def.as_str()).unwrap_or("");
                let vals = if st.overflow {
                    if st.has_num {
                        format!("{}..{} (many)", ff(st.min as f32), ff(st.max as f32))
                    } else {
                        "(many)".into()
                    }
                } else {
                    let mut v: Vec<&(Val, u64)> = st.distinct.iter().collect();
                    v.sort_by(|a, b| b.1.cmp(&a.1));
                    v.iter()
                        .take(14)
                        .map(|(x, n)| format!("{}×{}", self.fprop(*pid, x), n))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let enter = if st.enter_overflow {
                    "(many)".to_string()
                } else {
                    let mut v: Vec<&(Val, u64)> = st.enter_vals.iter().collect();
                    v.sort_by(|a, b| b.1.cmp(&a.1));
                    v.iter()
                        .take(6)
                        .map(|(x, n)| format!("{}×{}", self.fprop(*pid, x), n))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                w!(
                    o,
                    "  {} [{}] chg={} ents={} sends={} rate={:.3} first={} | {} | enter: {}",
                    self.pname(*pid),
                    def,
                    st.changes,
                    st.ents.len(),
                    st.sends,
                    st.changes as f32 / inst_secs,
                    st.first_change.unwrap_or(0),
                    vals,
                    enter
                );
            }
            let static_props: Vec<String> = cs
                .props
                .iter()
                .filter(|(_, s)| s.changes == 0 && !s.enter_vals.is_empty())
                .filter(|(_, s)| s.enter_overflow || s.enter_vals.iter().any(|(v, _)| !v.is_zero()))
                .map(|(pid, s)| {
                    let v = if s.enter_overflow {
                        "(many)".to_string()
                    } else {
                        let mut v: Vec<&(Val, u64)> = s.enter_vals.iter().collect();
                        v.sort_by(|a, b| b.1.cmp(&a.1));
                        v.iter()
                            .take(4)
                            .map(|(x, n)| format!("{}×{}", self.fprop(*pid, x), n))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    format!("{}={}", self.pname(*pid), v)
                })
                .collect();
            let mut static_props = static_props;
            static_props.sort();
            w!(
                o,
                "  static non-zero props (set at creation, never changed) [{}]:",
                static_props.len()
            );
            for s in static_props.iter().take(120) {
                w!(o, "    {s}");
            }
        }

        // ---------------------------------------------------------------- 3 flattened tables
        w!(o);
        w!(o, "## 3. Flattened send tables for classes of interest");
        let mut dump: BTreeSet<u16> = BTreeSet::new();
        for n in DUMP_CLASSES {
            if let Some(c) = self.class_id_by_name(n) {
                dump.insert(c);
            }
        }
        for c in self.class_stats.keys() {
            if matches!(
                self.kind_of(*c),
                Kind::Weapon | Kind::Projectile | Kind::ViewModel
            ) {
                dump.insert(*c);
            }
        }
        for c in self.te_stats.keys() {
            dump.insert(*c);
        }
        for c in dump {
            let Some(Some(info)) = self.class_info.get(c as usize) else {
                continue;
            };
            let cs = self.class_stats.get(&c);
            let te = self.te_stats.get(&c);
            w!(o);
            w!(
                o,
                "### {} ({}) — {} flattened props{}",
                info.name,
                info.table,
                info.flat.len(),
                if cs.is_none() && te.is_none() {
                    " — NEVER INSTANTIATED"
                } else {
                    ""
                }
            );
            for (i, pid) in info.flat.iter().enumerate() {
                let def = self.names.get(pid).map(|n| n.def.as_str()).unwrap_or("");
                let status = if let Some(te) = te {
                    match te.props.get(pid) {
                        Some((n, _, _)) => format!("explicitly sent in {n}/{} events", te.count),
                        None => "never sent".into(),
                    }
                } else if let Some(cs) = cs {
                    match cs.props.get(pid) {
                        Some(s) if s.changes > 0 => format!("changes {}", s.changes),
                        Some(s) if s.enter_sent > 0 => "sent at creation only".into(),
                        Some(s) if s.enter_vals.iter().any(|(v, _)| !v.is_zero()) => {
                            "baseline only".into()
                        }
                        Some(_) => "baseline zero".into(),
                        None => "never".into(),
                    }
                } else {
                    String::new()
                };
                w!(o, "  {i:4} {} [{}] {}", self.pname(*pid), def, status);
            }
        }

        // ---------------------------------------------------------------- 4 players
        w!(o);
        w!(o, "## 4. Players");
        let player_class = self.class_id_by_name("CTFPlayer");
        let class_tl =
            player_class.map(|c| self.timeline(c, id("DT_TFPlayerClassShared", "m_iClass")));
        let class_at = |e: u32, t: u32| -> i64 {
            class_tl
                .as_ref()
                .and_then(|tl| tl.at(e, t))
                .and_then(|v| v.int())
                .unwrap_or(0)
        };
        for p in &self.gs.state.players {
            if let Some(info) = &p.info {
                let e = u32::from(p.entity);
                let classes: BTreeSet<&str> = class_tl
                    .as_ref()
                    .and_then(|tl| tl.by_ent.get(&e))
                    .map(|v| {
                        v.iter()
                            .filter_map(|(_, c)| c.int())
                            .filter(|c| *c > 0)
                            .map(class_name)
                            .collect()
                    })
                    .unwrap_or_default();
                w!(
                    o,
                    "  ent {} {:?} steam={} team={:?} classes={:?}",
                    e,
                    info.name,
                    info.steam_id,
                    info.team,
                    classes
                );
            }
        }
        if let Some(pc) = player_class {
            // condition analysis
            let words: [(&str, &str); 6] = [
                ("DT_TFPlayerShared", "m_nPlayerCond"),
                ("DT_TFPlayerShared", "m_nPlayerCondEx"),
                ("DT_TFPlayerShared", "m_nPlayerCondEx2"),
                ("DT_TFPlayerShared", "m_nPlayerCondEx3"),
                ("DT_TFPlayerShared", "m_nPlayerCondEx4"),
                ("DT_TFPlayerConditionListExclusive", "_condition_bits"),
            ];
            let mut events: BTreeMap<u32, Vec<(u32, usize, u32)>> = BTreeMap::new();
            for (wi, (t, n)) in words.iter().enumerate() {
                let pid = id(t, n);
                let present = self.class_info[pc as usize]
                    .as_ref()
                    .map(|i| i.flat.contains(&pid))
                    .unwrap_or(false);
                let log = self.log_of(pc, pid);
                w!(
                    o,
                    "  cond word {wi} {t}.{n}: in send table={} changes logged={} (total {})",
                    present,
                    log.map(|l| l.v.len()).unwrap_or(0),
                    log.map(|l| l.total).unwrap_or(0)
                );
                if let Some(log) = log {
                    for e in &log.v {
                        events.entry(e.ent as u32).or_default().push((
                            e.tick,
                            wi,
                            e.val.int().unwrap_or(0) as u32,
                        ));
                    }
                }
            }
            #[derive(Default)]
            struct CondAgg {
                onsets: u64,
                total: u64,
                min: u32,
                max: u32,
                players: BTreeSet<u32>,
                classes: BTreeMap<&'static str, u64>,
                samples: Vec<String>,
            }
            let mut agg: BTreeMap<(usize, usize), CondAgg> = BTreeMap::new(); // (word, bit)
            let mut cond_tl: BTreeMap<u32, Vec<(u32, [u32; 6])>> = BTreeMap::new();
            for (e, evs) in events.iter_mut() {
                evs.sort_by_key(|x| x.0);
                let mut cur = [0u32; 6];
                let mut open: FnvHashMap<(usize, usize), u32> = FnvHashMap::default();
                let mut tl = Vec::new();
                for (t, wi, v) in evs.iter() {
                    let old = cur[*wi];
                    cur[*wi] = *v;
                    tl.push((*t, cur));
                    let diff = old ^ *v;
                    for b in 0..32usize {
                        if diff & (1 << b) == 0 {
                            continue;
                        }
                        let key = (*wi, b);
                        if *v & (1 << b) != 0 {
                            open.insert(key, *t);
                            let a = agg.entry(key).or_default();
                            a.onsets += 1;
                            a.players.insert(*e);
                            *a.classes.entry(class_name(class_at(*e, *t))).or_default() += 1;
                            if a.samples.len() < 4 {
                                a.samples.push(format!(
                                    "{} {}({})",
                                    self.rel(*t),
                                    pname(*e),
                                    class_name(class_at(*e, *t))
                                ));
                            }
                        } else if let Some(s) = open.remove(&key) {
                            let a = agg.entry(key).or_default();
                            let d = t - s;
                            a.total += d as u64;
                            if a.min == 0 || d < a.min {
                                a.min = d;
                            }
                            a.max = a.max.max(d);
                        }
                    }
                }
                cond_tl.insert(*e, tl);
            }
            w!(o, "  condition bits ever set (word.bit -> cond): onsets, total/min/max duration (ticks), players, classes at onset, first onsets");
            for ((wi, b), a) in &agg {
                let cond = if *wi < 5 { wi * 32 + b } else { *b };
                let src = if *wi < 5 {
                    words[*wi].1
                } else {
                    "_condition_bits"
                };
                w!(o, "    {src} bit {b} => cond {cond} {}: onsets={} total={} min={} max={} avg={:.0} players={} classes={:?} | {}",
                    cond_name(cond), a.onsets, a.total, a.min, a.max,
                    if a.onsets > 0 { a.total as f32 / a.onsets as f32 } else { 0.0 },
                    a.players.len(), a.classes, a.samples.join("; "));
            }
            let cond_at = |e: u32, t: u32| -> [u32; 6] {
                cond_tl
                    .get(&e)
                    .and_then(|v| {
                        let i = v.partition_point(|(tt, _)| *tt <= t);
                        if i == 0 {
                            None
                        } else {
                            Some(v[i - 1].1)
                        }
                    })
                    .unwrap_or([0; 6])
            };
            let has_cond = |c: [u32; 6], cond: usize| -> bool {
                let wi = cond / 32;
                let b = cond % 32;
                (c[wi] >> b) & 1 == 1 || (cond < 32 && (c[5] >> b) & 1 == 1)
            };
            let decode_diff = |a: [u32; 6], b: [u32; 6]| -> String {
                let mut parts = Vec::new();
                for wi in 0..6 {
                    let d = a[wi] ^ b[wi];
                    for bit in 0..32 {
                        if d & (1 << bit) != 0 {
                            let cond = if wi < 5 { wi * 32 + bit } else { bit };
                            let src = if wi == 5 { "(list)" } else { "" };
                            parts.push(format!(
                                "{}{}{}",
                                if b[wi] & (1 << bit) != 0 { "+" } else { "-" },
                                cond_name(cond),
                                src
                            ));
                        }
                    }
                }
                parts.join(" ")
            };

            // per-prop timelines of interest
            let cloak_id = id("DT_TFPlayerShared", "m_flCloakMeter");
            let onsets = |e: u32, cond: usize| -> Vec<u32> {
                cond_tl
                    .get(&e)
                    .map(|tl| {
                        let mut v = Vec::new();
                        let mut prev = [0u32; 6];
                        for (t, c) in tl {
                            if !has_cond(prev, cond) && has_cond(*c, cond) {
                                v.push(*t);
                            }
                            prev = *c;
                        }
                        v
                    })
                    .unwrap_or_default()
            };

            // spy timeline
            const SPY_PROPS: &[&str] = &[
                "m_flInvisChangeCompleteTime",
                "m_nDisguiseTeam",
                "m_nDisguiseClass",
                "m_nMaskClass",
                "m_hDisguiseTarget",
                "m_iDisguiseHealth",
                "m_hDisguiseWeapon",
                "m_iDisguiseBody",
                "m_nDisguiseSkinOverride",
                "m_nDesiredDisguiseTeam",
                "m_nDesiredDisguiseClass",
                "m_bLastDisguisedAsOwnTeam",
                "m_flStealthNoAttackExpire",
                "m_flStealthNextChangeTime",
                "m_bFeignDeathReady",
                "m_hActiveWeapon",
                "m_lifeState",
            ];
            w!(o);
            w!(o, "### 4a. Spy timeline (cloak/disguise)");
            let spies: Vec<u32> = agg
                .get(&(0, 4))
                .map(|a| a.players.iter().copied().collect())
                .unwrap_or_default();
            let best_spy = spies.iter().copied().max_by_key(|e| onsets(*e, 4).len());
            if let Some(spy) = best_spy {
                let ons = onsets(spy, 4);
                let t = ons
                    .iter()
                    .copied()
                    .find(|t| self.secs(t.saturating_sub(self.base_tick())) > 60.0)
                    .or(ons.first().copied());
                if let Some(t) = t {
                    let t0 = t.saturating_sub(250);
                    let t1 = t + 700;
                    w!(
                        o,
                        "  spy {} ({}) — {} cloak onsets; window {}..{}",
                        spy,
                        pname(spy),
                        ons.len(),
                        self.rel(t0),
                        self.rel(t1)
                    );
                    let mut lines: Vec<(u32, String)> = Vec::new();
                    if let Some(tl) = cond_tl.get(&spy) {
                        let mut prev = cond_at(spy, t0.saturating_sub(1));
                        for (tt, c) in tl.iter().filter(|(tt, _)| *tt >= t0 && *tt <= t1) {
                            let d = decode_diff(prev, *c);
                            if !d.is_empty() {
                                lines.push((*tt, format!("cond {d}")));
                            }
                            prev = *c;
                        }
                    }
                    for (tt, pid, v) in
                        self.entity_changes(pc, spy, t0, t1, &|p: &str| !SPY_PROPS.contains(&p))
                    {
                        let pn = self.prop_only(pid);
                        let label = if pn == "m_hActiveWeapon" || pn == "m_hDisguiseWeapon" {
                            self.weapon_label(v.int().unwrap_or(INVALID_EHANDLE))
                        } else if pn == "m_hDisguiseTarget" {
                            let h = v.int().unwrap_or(INVALID_EHANDLE);
                            format!("{} ({})", fh(h), h_index(h).map(pname).unwrap_or_default())
                        } else {
                            self.fprop(pid, &v)
                        };
                        lines.push((tt, format!("{} = {}", self.pname(pid), label)));
                    }
                    if let Some(log) = self.log_of(pc, cloak_id) {
                        let pts: Vec<&LogE> = log
                            .v
                            .iter()
                            .filter(|e| {
                                e.ent as u32 == spy && !e.init && e.tick >= t0 && e.tick <= t1
                            })
                            .collect();
                        for (i, e) in pts.iter().enumerate() {
                            if i % 12 == 0 || i + 1 == pts.len() {
                                lines.push((
                                    e.tick,
                                    format!(
                                        "m_flCloakMeter = {} (sample {} of {} changes in window)",
                                        self.fval(&e.val),
                                        i + 1,
                                        pts.len()
                                    ),
                                ));
                            }
                        }
                    }
                    lines.sort_by_key(|x| x.0);
                    for (tt, s2) in lines.iter().take(200) {
                        w!(o, "    {} {}", self.rel(*tt), s2);
                    }
                    if let Some(log) = self.log_of(pc, cloak_id) {
                        let ticks: Vec<u32> = log
                            .v
                            .iter()
                            .filter(|e| e.ent as u32 == spy && !e.init)
                            .map(|e| e.tick)
                            .collect();
                        let mut gh: BTreeMap<u32, u64> = BTreeMap::new();
                        for w2 in ticks.windows(2) {
                            *gh.entry((w2[1] - w2[0]).min(100)).or_default() += 1;
                        }
                        w!(o, "  m_flCloakMeter tick-gap histogram for this spy (gap: count; 100 = >=100): {:?}", gh.iter().take(16).collect::<Vec<_>>());
                    }
                }
            }
            for pn in SPY_PROPS
                .iter()
                .chain(["m_flCloakMeter", "m_flSpyTranqBuffDuration"].iter())
            {
                w!(o, "  {pn}: {}", self.props_named_summary(pc, pn));
            }

            // sniper timeline
            const SNIPER_PROPS: &[&str] = &[
                "m_iFOV",
                "m_iFOVStart",
                "m_flFOVTime",
                "m_iDefaultFOV",
                "m_hZoomOwner",
                "m_flFOVRate",
                "m_hActiveWeapon",
                "m_lifeState",
            ];
            const RIFLE_PROPS: &[&str] = &[
                "m_flChargedDamage",
                "m_flLastFireTime",
                "m_nMuzzleFlashParity",
                "m_iState",
                "m_bLowered",
                "m_flUnzoomTime",
                "m_flRezoomTime",
                "m_bRezoomAfterShot",
                "m_flNextPrimaryAttack",
                "m_iConsecutiveShots",
                "m_iReloadMode",
            ];
            w!(o);
            w!(
                o,
                "### 4b. Sniper timeline (zoom / FOV / rifle charge / dot)"
            );
            let snipers: Vec<u32> = agg
                .get(&(0, 1))
                .map(|a| a.players.iter().copied().collect())
                .unwrap_or_default();
            let best = snipers.iter().copied().max_by_key(|e| onsets(*e, 1).len());
            if let Some(sn) = best {
                let ons = onsets(sn, 1);
                if let Some(&t) = ons.get(3).or(ons.first()) {
                    let t0 = t.saturating_sub(120);
                    let t1 = t + 420;
                    w!(
                        o,
                        "  sniper {} ({}) — {} zoom onsets; window {}..{}",
                        sn,
                        pname(sn),
                        ons.len(),
                        self.rel(t0),
                        self.rel(t1)
                    );
                    let mut lines: Vec<(u32, String)> = Vec::new();
                    if let Some(tl) = cond_tl.get(&sn) {
                        let mut prev = cond_at(sn, t0.saturating_sub(1));
                        for (tt, c) in tl.iter().filter(|(tt, _)| *tt >= t0 && *tt <= t1) {
                            let d = decode_diff(prev, *c);
                            if !d.is_empty() {
                                lines.push((*tt, format!("cond {d}")));
                            }
                            prev = *c;
                        }
                    }
                    for (tt, pid, v) in
                        self.entity_changes(pc, sn, t0, t1, &|p: &str| !SNIPER_PROPS.contains(&p))
                    {
                        let label = if self.prop_only(pid) == "m_hActiveWeapon" {
                            self.weapon_label(v.int().unwrap_or(INVALID_EHANDLE))
                        } else {
                            self.fprop(pid, &v)
                        };
                        lines.push((tt, format!("{} = {}", self.pname(pid), label)));
                    }
                    // the rifle(s) owned by this sniper in the window
                    for ((wi, wser), w2) in &self.weapons {
                        let cn = self.class_name_of(w2.class);
                        if !cn.starts_with("CTFSniperRifle") || h_index(w2.owner) != Some(sn) {
                            continue;
                        }
                        if w2.created > t1 || w2.deleted.map(|d| d < t0).unwrap_or(false) {
                            continue;
                        }
                        let _ = wser;
                        let mut charge_n = 0;
                        for (tt, pid, v) in self.entity_changes(
                            w2.class,
                            *wi,
                            t0.max(w2.created),
                            t1.min(w2.deleted.unwrap_or(u32::MAX)),
                            &|p: &str| !RIFLE_PROPS.contains(&p),
                        ) {
                            if self.prop_only(pid) == "m_flChargedDamage" {
                                charge_n += 1;
                                if charge_n % 6 != 1 {
                                    continue;
                                }
                            }
                            lines.push((
                                tt,
                                format!(
                                    "{}#{} {} = {}",
                                    cn,
                                    wi,
                                    self.pname(pid),
                                    self.fprop(pid, &v)
                                ),
                            ));
                        }
                    }
                    for d in self.dots.iter().filter(|d| {
                        h_index(d.owner) == Some(sn)
                            && d.created <= t1
                            && d.deleted.map(|x| x >= t0).unwrap_or(true)
                    }) {
                        lines.push((
                            d.created,
                            format!(
                                "CSniperDot ent {} CREATED chargeStart={} origin={:?}",
                                d.ent,
                                ff(d.charge_start),
                                d.origin
                            ),
                        ));
                        if let Some(x) = d.deleted {
                            lines.push((x, format!("CSniperDot ent {} DELETED (lifetime {} ticks, {} origin updates)", d.ent, x - d.created, d.origin_changes)));
                        }
                    }
                    for f in self
                        .fire_bullets
                        .iter()
                        .filter(|f| (f.player + 1) as u32 == sn && f.tick >= t0 && f.tick <= t1)
                    {
                        lines.push((
                            f.tick,
                            format!(
                                "TEFireBullets {} origin {:?} ang {:?}",
                                weapon_id_name(f.weapon),
                                f.origin,
                                f.ang
                            ),
                        ));
                    }
                    for a in self
                        .anim_events
                        .iter()
                        .filter(|a| a.player == sn as i64 && a.tick >= t0 && a.tick <= t1)
                    {
                        lines.push((
                            a.tick,
                            format!(
                                "TEPlayerAnimEvent {} data={}",
                                anim_event_name(a.event),
                                a.data
                            ),
                        ));
                    }
                    lines.sort_by_key(|x| x.0);
                    for (tt, s2) in lines.iter().take(220) {
                        w!(o, "    {} {}", self.rel(*tt), s2);
                    }
                }
            }
            for pn in SNIPER_PROPS {
                w!(o, "  {pn}: {}", self.props_named_summary(pc, pn));
            }

            // active weapons
            w!(o);
            w!(o, "### 4c. Active weapon / my weapons / healers / taunts");
            let aw = id("DT_BaseCombatCharacter", "m_hActiveWeapon");
            if let Some(log) = self.log_of(pc, aw) {
                let mut per: BTreeMap<u32, u64> = BTreeMap::new();
                let mut resolved = 0u64;
                let mut unresolved = 0u64;
                let mut by_class: BTreeMap<String, u64> = BTreeMap::new();
                for e in &log.v {
                    *per.entry(e.ent as u32).or_default() += 1;
                    let h = e.val.int().unwrap_or(INVALID_EHANDLE);
                    if let Some(i) = h_index(h) {
                        match self.weapons.get(&(i, h_serial(h))) {
                            Some(w2) => {
                                resolved += 1;
                                *by_class
                                    .entry(self.class_name_of(w2.class).to_string())
                                    .or_default() += 1;
                            }
                            None => unresolved += 1,
                        }
                    }
                }
                w!(o, "  m_hActiveWeapon changes={} resolved-to-weapon={} unresolved={} per player={:?}", log.total, resolved, unresolved, per);
                w!(
                    o,
                    "  active weapon classes (switch targets): {:?}",
                    by_class
                );
                let first: Vec<String> = log
                    .v
                    .iter()
                    .take(12)
                    .map(|e| {
                        format!(
                            "{}@{}:{}",
                            e.ent,
                            e.tick,
                            self.weapon_label(e.val.int().unwrap_or(INVALID_EHANDLE))
                        )
                    })
                    .collect();
                w!(o, "  first: {}", first.join(" "));
            }
            // m_hMyWeapons for one player of each class at end of demo
            let mut seen_class: BTreeSet<i64> = BTreeSet::new();
            for (e, ent) in self
                .ents
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.as_ref().map(|e| (i, e)))
            {
                if ent.class != pc {
                    continue;
                }
                let cls = Probe::get(ent, "DT_TFPlayerClassShared", "m_iClass")
                    .and_then(|v| v.int())
                    .unwrap_or(0);
                if !seen_class.insert(cls) {
                    continue;
                }
                let mut ws = Vec::new();
                for k in 0..48 {
                    let pid = id("m_hMyWeapons", &format!("{k:03}"));
                    if let Some(h) = ent.props.get(&pid).and_then(|v| v.int()) {
                        if h_index(h).is_some() {
                            ws.push(format!("{}:{}", k, self.weapon_label(h)));
                        }
                    }
                }
                let active = Probe::get(ent, "DT_BaseCombatCharacter", "m_hActiveWeapon")
                    .and_then(|v| v.int())
                    .unwrap_or(INVALID_EHANDLE);
                w!(
                    o,
                    "  end-of-demo {} ({}, {}): m_hMyWeapons = {} | active = {}",
                    e,
                    pname(e as u32),
                    class_name(cls),
                    ws.join(", "),
                    self.weapon_label(active)
                );
            }
            for pn in [
                "m_nNumHealers",
                "m_iTauntIndex",
                "m_iTauntConcept",
                "m_iTauntItemDefIndex",
                "m_nActiveTauntSlot",
                "m_flTauntYaw",
                "m_bAllowMoveDuringTaunt",
                "m_flCurrentTauntMoveSpeed",
                "m_unTauntSourceItemID_Low",
                "m_bCarryingObject",
                "m_hCarriedObject",
                "m_flChargeMeter",
                "m_iCritMult",
                "m_iStunFlags",
                "m_iDecapitations",
                "m_iRevengeCrits",
                "m_flHypeMeter",
                "m_flEnergyDrinkMeter",
                "m_flRageMeter",
                "m_nPlayerState",
                "m_iObserverMode",
                "m_hObserverTarget",
                "m_hRagdoll",
                "m_iSpawnCounter",
                "m_flInspectTime",
                "m_hItem",
                "m_bShieldEquipped",
                "m_flHolsterAnimTime",
                "m_hSwitchTo",
                "m_nTeamTeleporterUsed",
                "m_iKillCountSinceLastDeploy",
                "m_flFirstPrimaryAttack",
                "m_iNextMeleeCrit",
                "m_nAirDucked",
                "m_iAirDash",
                "m_bJumping",
                "m_hGroundEntity",
                "m_nWaterLevel",
                "m_iHealth",
                "m_iMaxHealth",
                "m_lifeState",
                "m_flDeathTime",
                "m_iTeamNum",
                "m_hLastWeapon",
                "m_hOffHandWeapon",
                "m_nActiveWpnClip",
                "m_iAmmo",
                "m_flMaxspeed",
                "m_fFlags",
                "m_bDucked",
                "m_bDucking",
                "m_flDucktime",
                "m_nForceTauntCam",
                "m_bIsReadyToHighFive",
                "m_hHighFivePartner",
                "m_flFallVelocity",
                "m_vecPunchAngle",
                "m_iWeaponKnockbackID",
                "m_bParachuteEquipped",
                "m_flItemChargeMeter",
                "m_bUsingActionSlot",
                "m_flHelpmeButtonPressTime",
                "m_nCurrency",
            ] {
                w!(o, "  {pn}: {}", self.props_named_summary(pc, pn));
            }
            // m_ConditionData providers
            let mut prov: Vec<String> = Vec::new();
            for ((c, pid), log) in &self.logs {
                if *c != pc {
                    continue;
                }
                if self.prop_only(*pid).starts_with("m_pProvider") {
                    let n = self
                        .names
                        .get(pid)
                        .map(|n| n.table.clone())
                        .unwrap_or_default();
                    prov.push(format!(
                        "{} total={} first={}",
                        n,
                        log.total,
                        log.v
                            .iter()
                            .take(3)
                            .map(|e| format!(
                                "{}@{}:{}",
                                e.ent,
                                e.tick,
                                fh(e.val.int().unwrap_or(0))
                            ))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ));
                }
            }
            prov.sort();
            w!(
                o,
                "  m_ConditionData[*].m_pProvider tables that changed ({}): {}",
                prov.len(),
                prov.join(" | ")
            );

            // ---------------------------------------------------------------- 5 weapons
            w!(o);
            w!(o, "## 5. Weapons");
            let mut wclass: BTreeMap<String, (u64, BTreeMap<i64, u64>, u64, u64)> = BTreeMap::new();
            for w2 in self.weapons.values() {
                let e = wclass
                    .entry(self.class_name_of(w2.class).to_string())
                    .or_default();
                e.0 += 1;
                *e.1.entry(w2.itemdef).or_default() += 1;
                if h_index(w2.owner).is_some() {
                    e.2 += 1;
                }
                if w2.owner == w2.owner_entity {
                    e.3 += 1;
                }
            }
            w!(o, "class: instances | item def indexes (×count) | with m_hOwner | m_hOwner==m_hOwnerEntity");
            for (n, (cnt, defs, own, same)) in &wclass {
                w!(o, "  {n}: {cnt} | {:?} | {own} | {same}", defs);
            }
            // weapon state props
            let weapon_classes: Vec<u16> = self
                .class_stats
                .keys()
                .copied()
                .filter(|c| self.kind_of(*c) == Kind::Weapon)
                .collect();
            for pn in [
                "m_iState",
                "m_hOwner",
                "m_iWeaponState",
                "m_bCritFire",
                "m_bHitTarget",
                "m_flChargeBeginTime",
                "m_bCritShot",
                "m_bChargeRelease",
                "m_flChargeLevel",
                "m_hHealingTarget",
                "m_bHealing",
                "m_bAttacking",
                "m_bHolstered",
                "m_nChargeResistType",
                "m_hLastHealingTarget",
                "m_flChargedDamage",
                "m_bArrowAlight",
                "m_bNoFire",
                "m_bLowered",
                "m_flLastFireTime",
                "m_iReloadMode",
                "m_flEnergy",
                "m_flEffectBarRegenTime",
                "m_bReadyToBackstab",
                "m_bKnifeExists",
                "m_flKnifeRegenerateDuration",
                "m_flKnifeMeltTimestamp",
                "m_bDisguiseWeapon",
                "m_flObservedCritChance",
                "m_iConsecutiveShots",
                "m_flLastCritCheckTime",
                "m_nInspectStage",
                "m_flInspectAnimEndTime",
                "m_iClip1",
                "m_flNextPrimaryAttack",
                "m_flNextSecondaryAttack",
                "m_nKillComboCount",
                "m_hExtraWearable",
                "m_hExtraWearableViewModel",
                "m_flDetonateTime",
                "m_iDetonated",
                "m_bBeingRepurposedForTaunt",
                "m_nMuzzleFlashParity",
                "m_flUnzoomTime",
                "m_flRezoomTime",
                "m_bRezoomAfterShot",
                "m_iPipebombCount",
                "m_flChargeBeginTime",
                "m_hHealingTargetEffect",
                "m_flResetBurstEffect",
                "m_bAirBlast",
            ] {
                let mut lines = Vec::new();
                let mut wcs: Vec<u16> = weapon_classes.clone();
                wcs.sort_by_key(|c| self.class_name_of(*c).to_string());
                for c in &wcs {
                    let Some(Some(info)) = self.class_info.get(*c as usize) else {
                        continue;
                    };
                    if !info.flat.iter().any(|p| self.prop_only(*p) == pn) {
                        continue;
                    }
                    let changed = info
                        .flat
                        .iter()
                        .filter(|p| self.prop_only(**p) == pn)
                        .any(|p| {
                            self.class_stats
                                .get(c)
                                .and_then(|s| s.props.get(p))
                                .map(|s| s.changes > 0)
                                .unwrap_or(false)
                        });
                    if !changed {
                        continue;
                    }
                    lines.push(format!(
                        "{}: {}",
                        info.name,
                        self.props_named_summary(*c, pn)
                    ));
                }
                let in_any = wcs.iter().any(|c| {
                    self.class_info
                        .get(*c as usize)
                        .and_then(|i| i.as_ref())
                        .map(|i| i.flat.iter().any(|p| self.prop_only(*p) == pn))
                        .unwrap_or(false)
                });
                w!(
                    o,
                    "  {pn}: {}",
                    if lines.is_empty() {
                        if in_any {
                            "in send tables but never changed".to_string()
                        } else {
                            "not in any instantiated weapon table".to_string()
                        }
                    } else {
                        String::new()
                    }
                );
                for l in lines {
                    w!(o, "      {l}");
                }
            }
            // medigun timeline
            if let Some(mc) = self.class_id_by_name("CWeaponMedigun") {
                let rel_ids: Vec<SendPropIdentifier> = self.class_info[mc as usize]
                    .as_ref()
                    .map(|i| {
                        i.flat
                            .iter()
                            .copied()
                            .filter(|p| {
                                let n = self.prop_only(*p);
                                matches!(
                                    n,
                                    "m_bChargeRelease"
                                        | "m_hHealingTarget"
                                        | "m_bHealing"
                                        | "m_bAttacking"
                                        | "m_bHolstered"
                                        | "m_nChargeResistType"
                                        | "m_iState"
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut evs: Vec<(u32, u16, SendPropIdentifier, Val)> = Vec::new();
                for p in rel_ids {
                    if let Some(log) = self.log_of(mc, p) {
                        for e in &log.v {
                            evs.push((e.tick, e.ent, p, e.val));
                        }
                    }
                }
                evs.sort_by_key(|x| (x.0, x.1));
                let first_release = evs.iter().position(|x| {
                    self.prop_only(x.2) == "m_bChargeRelease" && x.3.int() == Some(1)
                });
                if let Some(i) = first_release {
                    let (t, ent, _, _) = evs[i];
                    w!(
                        o,
                        "  medigun ent {} first m_bChargeRelease=1 at {} — timeline ±:",
                        ent,
                        self.rel(t)
                    );
                    for x in evs
                        .iter()
                        .filter(|x| x.1 == ent && x.0 + 400 >= t && x.0 <= t + 700)
                    {
                        w!(
                            o,
                            "    {} {} = {}",
                            self.rel(x.0),
                            self.pname(x.2),
                            self.fprop(x.2, &x.3)
                        );
                    }
                    let ch = self.class_info[mc as usize]
                        .as_ref()
                        .map(|i| {
                            i.flat
                                .iter()
                                .copied()
                                .filter(|p| self.prop_only(*p) == "m_flChargeLevel")
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for p in ch {
                        if let Some(log) = self.log_of(mc, p) {
                            let pts: Vec<String> = log
                                .v
                                .iter()
                                .filter(|e| e.ent == ent && e.tick + 400 >= t && e.tick <= t + 700)
                                .map(|e| format!("{}:{}", e.tick, self.fval(&e.val)))
                                .collect();
                            w!(
                                o,
                                "    {} in window ({} changes, every 10th): {}",
                                self.pname(p),
                                pts.len(),
                                pts.iter()
                                    .step_by(10)
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            );
                        }
                    }
                }
            }
            // flamethrower / minigun weapon state timeline sample
            for cn in ["CTFFlameThrower", "CTFMinigun"] {
                if let Some(c) = self.class_id_by_name(cn) {
                    let ids: Vec<SendPropIdentifier> = self.class_info[c as usize]
                        .as_ref()
                        .map(|i| {
                            i.flat
                                .iter()
                                .copied()
                                .filter(|p| {
                                    matches!(
                                        self.prop_only(*p),
                                        "m_iWeaponState"
                                            | "m_bCritFire"
                                            | "m_bHitTarget"
                                            | "m_bCritShot"
                                            | "m_iState"
                                            | "m_flChargeBeginTime"
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut evs: Vec<(u32, u16, SendPropIdentifier, Val)> = Vec::new();
                    for p in ids {
                        if let Some(log) = self.log_of(c, p) {
                            for e in &log.v {
                                evs.push((e.tick, e.ent, p, e.val));
                            }
                        }
                    }
                    evs.sort_by_key(|x| (x.0, x.1));
                    let start = evs.iter().position(|x| {
                        self.prop_only(x.2) == "m_iWeaponState" && x.3.int().unwrap_or(0) > 0
                    });
                    if let Some(i) = start {
                        let (t, ent, _, _) = evs[i];
                        w!(
                            o,
                            "  {cn} ent {} first m_iWeaponState>0 at {}:",
                            ent,
                            self.rel(t)
                        );
                        for x in evs.iter().filter(|x| x.1 == ent && x.0 + 60 >= t).take(40) {
                            w!(
                                o,
                                "    {} {} = {}",
                                self.rel(x.0),
                                self.pname(x.2),
                                self.fprop(x.2, &x.3)
                            );
                        }
                    }
                }
            }

            // ---------------------------------------------------------------- 6 viewmodels
            w!(o);
            w!(o, "## 6. Viewmodels");
            if let Some(vc) = self.class_id_by_name("CTFViewModel") {
                let owner_id = id("DT_BaseViewModel", "m_hOwner");
                let seq_id = id("DT_BaseViewModel", "m_nSequence");
                let par_id = id("DT_BaseViewModel", "m_nAnimationParity");
                let _newpar_id = id("DT_BaseViewModel", "m_nNewSequenceParity");
                let _reset_id = id("DT_BaseViewModel", "m_nResetEventsParity");
                let model_id = id("DT_BaseViewModel", "m_nModelIndex");
                let weap_id = id("DT_BaseViewModel", "m_hWeapon");
                let vmi_id = id("DT_BaseViewModel", "m_nViewModelIndex");
                let owner_tl = self.timeline(vc, owner_id);
                let model_tl = self.timeline(vc, model_id);
                let weap_tl = self.timeline(vc, weap_id);
                let vmi_tl = self.timeline(vc, vmi_id);
                w!(
                    o,
                    "  CTFViewModel entities: {:?}",
                    owner_tl.by_ent.keys().collect::<Vec<_>>()
                );
                for (vm, v) in &owner_tl.by_ent {
                    let owners: BTreeSet<String> =
                        v.iter().map(|(_, h)| fh(h.int().unwrap_or(0))).collect();
                    let seqs = self
                        .log_of(vc, seq_id)
                        .map(|l| l.v.iter().filter(|e| e.ent as u32 == *vm).count())
                        .unwrap_or(0);
                    let pars = self
                        .log_of(vc, par_id)
                        .map(|l| l.v.iter().filter(|e| e.ent as u32 == *vm).count())
                        .unwrap_or(0);
                    let models: BTreeSet<String> = model_tl
                        .by_ent
                        .get(vm)
                        .map(|v| {
                            v.iter()
                                .map(|(_, m)| self.model_name(m.int().unwrap_or(0)))
                                .collect()
                        })
                        .unwrap_or_default();
                    let vmis: BTreeSet<i64> = vmi_tl
                        .by_ent
                        .get(vm)
                        .map(|v| v.iter().filter_map(|(_, m)| m.int()).collect())
                        .unwrap_or_default();
                    let weaps = weap_tl.by_ent.get(vm).map(|v| v.len()).unwrap_or(0);
                    w!(o, "  vm ent {vm}: owners {:?} vmIndex {:?} | m_nSequence changes={seqs} m_nAnimationParity changes={pars} m_hWeapon changes={weaps} | models {:?}", owners, vmis, models);
                }
                // per player activity / shot-signal table
                let mut vm_of: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
                for (vm, v) in &owner_tl.by_ent {
                    for (_, h) in v {
                        if let Some(i) = h_index(h.int().unwrap_or(0)) {
                            let e = vm_of.entry(i).or_default();
                            if !e.contains(vm) {
                                e.push(*vm);
                            }
                        }
                    }
                }
                let mut anim_by_p: BTreeMap<i64, BTreeMap<String, u64>> = BTreeMap::new();
                for a in &self.anim_events {
                    *anim_by_p
                        .entry(a.player)
                        .or_default()
                        .entry(anim_event_name(a.event))
                        .or_default() += 1;
                }
                let mut fb_by_p: BTreeMap<i64, u64> = BTreeMap::new();
                for f in &self.fire_bullets {
                    *fb_by_p.entry(f.player + 1).or_default() += 1;
                }
                let mut proj_by_p: BTreeMap<u32, u64> = BTreeMap::new();
                for p in &self.projectiles {
                    let ow = h_index(p.owner).or(h_index(p.thrower));
                    if let Some(ow) = ow {
                        *proj_by_p.entry(ow).or_default() += 1;
                    }
                }
                let mut w_by_idx: BTreeMap<u32, Vec<(u32, u32, Option<u32>)>> = BTreeMap::new();
                for ((wi, _), w2) in &self.weapons {
                    w_by_idx.entry(*wi).or_default().push((
                        w2.created,
                        w2.deleted.unwrap_or(u32::MAX),
                        h_index(w2.owner),
                    ));
                }
                let owner_at = |wi: u32, t: u32| -> Option<u32> {
                    w_by_idx
                        .get(&wi)
                        .and_then(|v| v.iter().find(|(c, d, _)| *c <= t && t <= *d))
                        .and_then(|x| x.2)
                };
                let mut lastfire_by_p: BTreeMap<u32, u64> = BTreeMap::new();
                let mut muzzle_by_p: BTreeMap<u32, u64> = BTreeMap::new();
                let mut clipdrop_by_p: BTreeMap<u32, u64> = BTreeMap::new();
                let weapon_classes2: Vec<u16> = self
                    .class_stats
                    .keys()
                    .copied()
                    .filter(|c| self.kind_of(*c) == Kind::Weapon)
                    .collect();
                for c in &weapon_classes2 {
                    for ((cc, pid), log) in &self.logs {
                        if cc != c {
                            continue;
                        }
                        let pn = self.prop_only(*pid);
                        if pn != "m_flLastFireTime"
                            && pn != "m_nMuzzleFlashParity"
                            && pn != "m_iClip1"
                        {
                            continue;
                        }
                        let mut last_clip: FnvHashMap<u16, i64> = FnvHashMap::default();
                        for e in &log.v {
                            if pn == "m_iClip1" {
                                let v = e.val.int().unwrap_or(0);
                                let prev = last_clip.insert(e.ent, v);
                                if e.init {
                                    continue;
                                }
                                if let (Some(prev), Some(ow)) =
                                    (prev, owner_at(e.ent as u32, e.tick))
                                {
                                    if v < prev && v >= 0 {
                                        *clipdrop_by_p.entry(ow).or_default() += (prev - v) as u64;
                                    }
                                }
                                continue;
                            }
                            if e.init {
                                continue;
                            }
                            if let Some(ow) = owner_at(e.ent as u32, e.tick) {
                                if pn == "m_flLastFireTime" {
                                    *lastfire_by_p.entry(ow).or_default() += 1;
                                } else {
                                    *muzzle_by_p.entry(ow).or_default() += 1;
                                }
                            }
                        }
                    }
                }
                let vm_muzzle_id = id("DT_BaseViewModel", "m_nMuzzleFlashParity");
                w!(o, "  per player shot/animation signals: classes | vm0 m_nSequence chg | vm m_nAnimationParity chg | vm m_nMuzzleFlashParity chg | weapon m_flLastFireTime chg | weapon m_nMuzzleFlashParity chg | weapon m_iClip1 rounds used | TEFireBullets | projectiles spawned | TEPlayerAnimEvent counts");
                for (e, n) in &names {
                    let vms = vm_of.get(e).cloned().unwrap_or_default();
                    let count = |pid: SendPropIdentifier, only_vm0: bool| -> usize {
                        vms.iter()
                            .filter(|vm| {
                                !only_vm0
                                    || vmi_tl
                                        .by_ent
                                        .get(vm)
                                        .and_then(|v| v.last())
                                        .and_then(|x| x.1.int())
                                        == Some(0)
                            })
                            .map(|vm| {
                                self.log_of(vc, pid)
                                    .map(|l| {
                                        l.v.iter()
                                            .filter(|x| x.ent as u32 == *vm && !x.init)
                                            .count()
                                    })
                                    .unwrap_or(0)
                            })
                            .sum()
                    };
                    let classes: BTreeSet<&str> = class_tl
                        .as_ref()
                        .and_then(|tl| tl.by_ent.get(e))
                        .map(|v| {
                            v.iter()
                                .filter_map(|(_, c)| c.int())
                                .filter(|c| *c > 0)
                                .map(class_name)
                                .collect()
                        })
                        .unwrap_or_default();
                    w!(
                        o,
                        "    {e} {n:?} {:?} | {} | {} | {} | {} | {} | {} | {} | {} | {:?}",
                        classes,
                        count(seq_id, true),
                        count(par_id, false),
                        count(vm_muzzle_id, true),
                        lastfire_by_p.get(e).copied().unwrap_or(0),
                        muzzle_by_p.get(e).copied().unwrap_or(0),
                        clipdrop_by_p.get(e).copied().unwrap_or(0),
                        fb_by_p.get(&(*e as i64)).copied().unwrap_or(0),
                        proj_by_p.get(e).copied().unwrap_or(0),
                        anim_by_p.get(&(*e as i64)).cloned().unwrap_or_default()
                    );
                }
                // soldier window: pick player with most rockets
                let rocket_class = self.class_id_by_name("CTFProjectile_Rocket");
                let mut rockets_by_p: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
                for p in &self.projectiles {
                    if Some(p.class) == rocket_class {
                        if let Some(ow) = h_index(p.owner) {
                            rockets_by_p.entry(ow).or_default().push(p.created);
                        }
                    }
                }
                if let Some((sol, ticks)) = rockets_by_p.iter().max_by_key(|(_, v)| v.len()) {
                    let win = 330u32;
                    let mut best_t = ticks[0];
                    let mut best_n = 0;
                    for (i, t) in ticks.iter().enumerate() {
                        let n = ticks[i..].iter().take_while(|x| **x < t + win).count();
                        if n > best_n {
                            best_n = n;
                            best_t = *t;
                        }
                    }
                    let t0 = best_t.saturating_sub(30);
                    let t1 = best_t + win;
                    let vms = vm_of.get(sol).cloned().unwrap_or_default();
                    w!(o, "  soldier {} ({}) — {} rockets total; window {}..{} ({} rockets); viewmodels {:?}", sol, pname(*sol), ticks.len(), self.rel(t0), self.rel(t1), best_n, vms);
                    let mut lines: Vec<(u32, String)> = Vec::new();
                    for p in self.projectiles.iter().filter(|p| {
                        Some(p.class) == rocket_class
                            && h_index(p.owner) == Some(*sol)
                            && p.created >= t0
                            && p.created <= t1
                    }) {
                        lines.push((
                            p.created,
                            format!(
                                "ROCKET spawn ent {} crit={:?} launcher={}",
                                p.ent,
                                p.crit,
                                self.weapon_label(p.launcher)
                            ),
                        ));
                    }
                    for vm in &vms {
                        for (tt, pid, v) in self.entity_changes(vc, *vm, t0, t1, &|p: &str| {
                            p == "m_flAnimTime" || p == "m_flCycle" || p == "m_flSimulationTime"
                        }) {
                            lines.push((
                                tt,
                                format!("vm{} {} = {}", vm, self.pname(pid), self.fprop(pid, &v)),
                            ));
                        }
                    }
                    for a in self
                        .anim_events
                        .iter()
                        .filter(|a| a.player == *sol as i64 && a.tick >= t0 && a.tick <= t1)
                    {
                        lines.push((
                            a.tick,
                            format!(
                                "TEPlayerAnimEvent {} data={}",
                                anim_event_name(a.event),
                                a.data
                            ),
                        ));
                    }
                    if let Some(pc2) = player_class {
                        for (tt, pid, v) in self
                            .entity_changes(pc2, *sol, t0, t1, &|p: &str| p != "m_hActiveWeapon")
                        {
                            lines.push((
                                tt,
                                format!(
                                    "player {} = {}",
                                    self.pname(pid),
                                    self.weapon_label(v.int().unwrap_or(INVALID_EHANDLE))
                                ),
                            ));
                        }
                    }
                    lines.sort_by_key(|x| x.0);
                    for (tt, s) in lines.iter().take(220) {
                        w!(o, "    {} {}", self.rel(*tt), s);
                    }
                    // correlation stats over the whole demo: rocket spawn vs nearest parity change of this soldier's vm
                    let mut par_ticks: Vec<u32> = Vec::new();
                    for vm in &vms {
                        if let Some(l) = self.log_of(vc, par_id) {
                            par_ticks
                                .extend(l.v.iter().filter(|e| e.ent as u32 == *vm).map(|e| e.tick));
                        }
                    }
                    par_ticks.sort();
                    let mut hist: BTreeMap<i64, u64> = BTreeMap::new();
                    for t in ticks {
                        let i = par_ticks.partition_point(|x| *x < *t);
                        let mut best: Option<i64> = None;
                        for j in [i.wrapping_sub(1), i] {
                            if let Some(pt) = par_ticks.get(j) {
                                let d = *pt as i64 - *t as i64;
                                if best.map(|b| d.abs() < b.abs()).unwrap_or(true) {
                                    best = Some(d);
                                }
                            }
                        }
                        *hist.entry(best.unwrap_or(9999).clamp(-20, 20)).or_default() += 1;
                    }
                    w!(o, "  rocket spawn tick minus nearest vm m_nAnimationParity change tick (clamped ±20): {:?}", hist);
                    let mut ahist: BTreeMap<i64, u64> = BTreeMap::new();
                    let atk: Vec<u32> = self
                        .anim_events
                        .iter()
                        .filter(|a| a.player == *sol as i64 && a.event == 0)
                        .map(|a| a.tick)
                        .collect();
                    for t in ticks {
                        let i = atk.partition_point(|x| *x < *t);
                        let mut best: Option<i64> = None;
                        for j in [i.wrapping_sub(1), i] {
                            if let Some(pt) = atk.get(j) {
                                let d = *pt as i64 - *t as i64;
                                if best.map(|b| d.abs() < b.abs()).unwrap_or(true) {
                                    best = Some(d);
                                }
                            }
                        }
                        *ahist
                            .entry(best.unwrap_or(9999).clamp(-20, 20))
                            .or_default() += 1;
                    }
                    w!(o, "  rocket spawn tick minus nearest TEPlayerAnimEvent ATTACK_PRIMARY tick (clamped ±20): {:?} (ATTACK_PRIMARY events for this player: {})", ahist, atk.len());
                }
                // sequence-vs-shot coincidence table per (model, sequence)
                let mut shot_ticks: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
                for p in &self.projectiles {
                    if let Some(ow) = h_index(p.owner).or(h_index(p.thrower)) {
                        shot_ticks.entry(ow).or_default().push(p.created);
                    }
                }
                for f in &self.fire_bullets {
                    shot_ticks
                        .entry((f.player + 1) as u32)
                        .or_default()
                        .push(f.tick);
                }
                for a in &self.anim_events {
                    if a.event == 0 || a.event == 40 {
                        shot_ticks.entry(a.player as u32).or_default().push(a.tick);
                    }
                }
                for v in shot_ticks.values_mut() {
                    v.sort();
                    v.dedup();
                }
                let mut seq_tab: BTreeMap<(String, i64), (u64, u64)> = BTreeMap::new();
                let seq_tl = self.timeline(vc, seq_id);
                if let Some(l) = self.log_of(vc, par_id) {
                    for e in l.v.iter().filter(|e| !e.init) {
                        let vm = e.ent as u32;
                        let owner = owner_tl
                            .at(vm, e.tick)
                            .and_then(|h| h_index(h.int().unwrap_or(0)));
                        let model = model_tl
                            .at(vm, e.tick)
                            .map(|m| self.model_name(m.int().unwrap_or(0)))
                            .unwrap_or_default();
                        let seq = seq_tl.at(vm, e.tick).and_then(|s| s.int()).unwrap_or(-1);
                        let shot = owner
                            .and_then(|ow| shot_ticks.get(&ow))
                            .map(|v| {
                                let i = v.partition_point(|x| *x + 3 < e.tick);
                                v.get(i).map(|x| *x <= e.tick + 3).unwrap_or(false)
                            })
                            .unwrap_or(false);
                        let ent = seq_tab.entry((model, seq)).or_default();
                        ent.0 += 1;
                        if shot {
                            ent.1 += 1;
                        }
                    }
                }
                w!(o, "  vm (model, sequence) at each m_nAnimationParity change: starts, starts within ±3 ticks of a shot by the owner (projectile spawn / FireBullets / ATTACK_PRIMARY anim event)");
                let mut rows: Vec<_> = seq_tab.iter().collect();
                rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
                for ((m, s), (n, shot)) in rows.iter().take(80) {
                    w!(
                        o,
                        "    {m} seq {s}: {n} starts, {shot} near shot ({:.0}%)",
                        100.0 * *shot as f32 / *n as f32
                    );
                }
            } else {
                w!(o, "  CTFViewModel not in server classes");
            }

            // ---------------------------------------------------------------- 7 sniper dot
            w!(o);
            w!(o, "## 7. CSniperDot");
            w!(o, "  instances: {}", self.dots.len());
            let mut zoomed_at_create = 0;
            let mut life: Vec<u32> = Vec::new();
            for d in &self.dots {
                if let Some(ow) = h_index(d.owner) {
                    if has_cond(cond_at(ow, d.created), 1)
                        || has_cond(cond_at(ow, d.created + 1), 1)
                    {
                        zoomed_at_create += 1;
                    }
                }
                if let Some(x) = d.deleted {
                    life.push(x - d.created);
                }
            }
            life.sort();
            w!(o, "  owner ZOOMED at creation (±1 tick): {}/{}; lifetime ticks min/median/max: {:?}/{:?}/{:?}", zoomed_at_create, self.dots.len(), life.first(), life.get(life.len() / 2), life.last());
            for d in self.dots.iter().take(12) {
                let ow = h_index(d.owner);
                w!(o, "  dot ent {} owner {:?}({}) created {} deleted {:?} life={:?} chargeStart={} simTime={} origin={:?} originChanges={} owner conds@create: {}",
                    d.ent, ow, ow.map(pname).unwrap_or_default(), self.rel(d.created), d.deleted.map(|x| self.rel(x)), d.deleted.map(|x| x - d.created),
                    ff(d.charge_start), ff(d.sim_time), d.origin, d.origin_changes,
                    ow.map(|ow| decode_diff([0; 6], cond_at(ow, d.created))).unwrap_or_default());
            }
            if let Some(dc) = self.class_id_by_name("CSniperDot") {
                if let Some(d) = self
                    .dots
                    .iter()
                    .find(|d| d.deleted.map(|x| x - d.created > 120).unwrap_or(false))
                {
                    w!(o, "  full change log of dot ent {} (first 60):", d.ent);
                    for (tt, pid, v) in self
                        .entity_changes(
                            dc,
                            d.ent,
                            d.created,
                            d.deleted.unwrap_or(u32::MAX),
                            &|p: &str| p == "m_flSimulationTime",
                        )
                        .iter()
                        .take(60)
                    {
                        w!(
                            o,
                            "    {} {} = {}",
                            self.rel(*tt),
                            self.pname(*pid),
                            self.fprop(*pid, v)
                        );
                    }
                }
            }
        }

        // ---------------------------------------------------------------- 8 buildings
        w!(o);
        w!(o, "## 8. Buildings");
        let noisy_table = |pid: &SendPropIdentifier| -> bool {
            self.names
                .get(pid)
                .map(|n| {
                    n.table == "m_flPoseParameter"
                        || n.table == "DT_Animationlayer"
                        || n.table.starts_with("_ST_m_AnimOverlay")
                        || n.table == "m_flexWeight"
                        || matches!(
                            n.prop.as_str(),
                            "m_flSimulationTime"
                                | "m_flAnimTime"
                                | "m_flCycle"
                                | "m_nNewSequenceParity"
                                | "m_nResetEventsParity"
                                | "m_flPrevCycle"
                                | "m_flWeight"
                                | "m_nOrder"
                                | "m_ubInterpolationFrame"
                        )
                })
                .unwrap_or(false)
        };
        for cn in [
            "CObjectSentrygun",
            "CObjectDispenser",
            "CObjectTeleporter",
            "CObjectSapper",
            "CObjectCartDispenser",
        ] {
            let Some(c) = self.class_id_by_name(cn) else {
                continue;
            };
            let inst: Vec<&MoverStat> = {
                let mut v: Vec<&MoverStat> =
                    self.movers.values().filter(|m| m.class == c).collect();
                v.sort_by_key(|m| m.created);
                v
            };
            w!(o, "  {cn}: {} instances", inst.len());
            for m in inst.iter().take(40) {
                let end = m.deleted.unwrap_or(u32::MAX);
                let in_inst =
                    |e: &&LogE| e.ent as u32 == m.ent && e.tick >= m.created && e.tick <= end;
                let vals = |t: &str, pn: &str| -> Vec<i64> {
                    self.log_of(c, id(t, pn))
                        .map(|l| {
                            l.v.iter()
                                .filter(in_inst)
                                .filter_map(|e| e.val.int())
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let lvl = vals("DT_BaseObject", "m_iUpgradeLevel")
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                let builder = vals("DT_BaseObject", "m_hBuilder")
                    .first()
                    .copied()
                    .unwrap_or(INVALID_EHANDLE);
                let mode = vals("DT_BaseObject", "m_iObjectMode")
                    .first()
                    .copied()
                    .unwrap_or(0);
                let mini = vals("DT_BaseObject", "m_bMiniBuilding")
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                let sapped = vals("DT_BaseObject", "m_bHasSapper")
                    .iter()
                    .filter(|v| **v == 1)
                    .count();
                let carried = vals("DT_BaseObject", "m_bCarried")
                    .iter()
                    .filter(|v| **v == 1)
                    .count();
                let placing = vals("DT_BaseObject", "m_bPlacing");
                let maps = vals("DT_BaseObject", "m_bWasMapPlaced")
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                w!(o, "    ent {} created {} deleted {:?} builder {} | max level {} mode {} mini {} mapPlaced {} | sapped-onsets {} carried-onsets {} placing-values {:?} | originChanges={} angleChanges={} leaves={} reenters={} | model {}",
                    m.ent, self.rel(m.created), m.deleted.map(|d| self.rel(d)), h_index(builder).map(|b| format!("{}({})", b, pname(b))).unwrap_or("?".into()),
                    lvl, mode, mini, maps, sapped, carried, placing.iter().take(6).collect::<Vec<_>>(), m.origin_changes, m.angle_changes, m.leaves, m.reenters, m.model);
            }
            if let Some(m) = inst.iter().max_by_key(|m| {
                self.entity_changes(c, m.ent, m.created, m.deleted.unwrap_or(u32::MAX), &|_| {
                    false
                })
                .len()
            }) {
                let end = m.deleted.unwrap_or(u32::MAX);
                let init: Vec<String> = self
                    .entity_log(c, m.ent, m.created, m.created, &|_| false, true)
                    .iter()
                    .filter(|(_, pid, v)| {
                        !v.is_zero()
                            && self
                                .names
                                .get(pid)
                                .map(|n| {
                                    n.table.starts_with("DT_BaseObject")
                                        || n.table.starts_with("DT_Object")
                                        || n.table.starts_with("DT_Sentry")
                                        || n.table.starts_with("DT_Teleporter")
                                        || n.table == "DT_BaseEntity"
                                })
                                .unwrap_or(false)
                    })
                    .map(|(_, pid, v)| format!("{}={}", self.pname(*pid), self.fprop(*pid, v)))
                    .collect();
                w!(
                    o,
                    "    ent {} creation values (object tables, non-zero): {}",
                    m.ent,
                    init.join(", ")
                );
                let changes: Vec<(u32, SendPropIdentifier, Val)> = self
                    .entity_changes(c, m.ent, m.created, end, &|_| false)
                    .into_iter()
                    .filter(|(_, pid, _)| !noisy_table(pid))
                    .collect();
                w!(
                    o,
                    "    ent {} change log ({} changes without pose/anim/sim noise; first 170):",
                    m.ent,
                    changes.len()
                );
                for (tt, pid, v) in changes.iter().take(170) {
                    w!(
                        o,
                        "      {} {} = {}",
                        self.rel(*tt),
                        self.pname(*pid),
                        self.fprop(*pid, v)
                    );
                }
                let mut pose_lines = Vec::new();
                for ((cc, pid), log) in &self.logs {
                    if *cc != c {
                        continue;
                    }
                    let n = self
                        .names
                        .get(pid)
                        .map(|n| (n.table.as_str(), n.prop.as_str()))
                        .unwrap_or(("", ""));
                    if n.0 == "m_flPoseParameter" {
                        let vals: Vec<&LogE> = log
                            .v
                            .iter()
                            .filter(|e| e.ent as u32 == m.ent && !e.init)
                            .collect();
                        if vals.is_empty() {
                            continue;
                        }
                        let (mn, mx) = vals
                            .iter()
                            .filter_map(|e| e.val.float())
                            .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
                        pose_lines.push(format!(
                            "{} changes={} range {}..{} first: {}",
                            self.pname(*pid),
                            vals.len(),
                            ff(mn),
                            ff(mx),
                            vals.iter()
                                .take(6)
                                .map(|e| format!("{}:{}", e.tick, self.fval(&e.val)))
                                .collect::<Vec<_>>()
                                .join(" ")
                        ));
                    }
                }
                pose_lines.sort();
                for l in pose_lines {
                    w!(o, "      pose: {l}");
                }
            }
        }

        // ---------------------------------------------------------------- 9 objectives
        w!(o);
        w!(o, "## 9. Objectives / game rules / timers (info classes)");
        for d in &self.info_dumps {
            let end = self.instance_end(d.ent, d.serial, d.tick);
            w!(o, "### {} ent {} serial {} alive {}..{} — non-zero props at creation (arrays grouped):", d.class, d.ent, d.serial, self.rel(d.tick), if end == u32::MAX { "end".to_string() } else { self.rel(end) });
            // group array elements (table = array name, prop = index) into one line
            let mut groups: BTreeMap<String, Vec<(String, String, bool)>> = BTreeMap::new();
            for (pid, v) in &d.props {
                let (t, pn) = self
                    .names
                    .get(pid)
                    .map(|n| (n.table.clone(), n.prop.clone()))
                    .unwrap_or_default();
                let is_elem = !pn.is_empty() && pn.chars().all(|c| c.is_ascii_digit());
                let key = if is_elem {
                    t.clone()
                } else {
                    format!("{t}.{pn}")
                };
                groups
                    .entry(key)
                    .or_default()
                    .push((pn.clone(), self.fprop(*pid, v), v.is_zero()));
            }
            for (k, mut vals) in groups {
                if vals.iter().all(|x| x.2) {
                    continue;
                }
                if vals.len() == 1 && !vals[0].0.chars().all(|c| c.is_ascii_digit()) {
                    w!(o, "    {k} = {}", vals[0].1);
                } else {
                    vals.sort_by(|a, b| a.0.cmp(&b.0));
                    let last_nz = vals.iter().rposition(|x| !x.2).unwrap_or(0);
                    w!(
                        o,
                        "    {k}[0..{}] = {}",
                        last_nz,
                        vals[..=last_nz]
                            .iter()
                            .map(|x| x.1.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            if let Some(c) = self.class_id_by_name(&d.class) {
                let changes = self.entity_changes(c, d.ent, d.tick, end, &|p: &str| {
                    p == "m_flSimulationTime" || p == "m_flAnimTime" || p == "m_flCycle"
                });
                w!(o, "  changes during this instance: {}", changes.len());
                let mut per: FnvHashMap<SendPropIdentifier, u32> = FnvHashMap::default();
                let mut printed = 0;
                for (tt, pid, v) in &changes {
                    let n = per.entry(*pid).or_default();
                    *n += 1;
                    if *n <= 30 && printed < 1200 {
                        printed += 1;
                        w!(
                            o,
                            "    {} {} = {}",
                            self.rel(*tt),
                            self.pname(*pid),
                            self.fprop(*pid, v)
                        );
                    }
                }
                let mut tot: Vec<(String, u32)> =
                    per.iter().map(|(p, n)| (self.pname(*p), *n)).collect();
                tot.sort_by(|a, b| b.1.cmp(&a.1));
                w!(
                    o,
                    "  change counts: {:?}",
                    tot.iter().take(80).collect::<Vec<_>>()
                );
            }
        }
        // children of trains / watchers
        let train = self.class_id_by_name("CFuncTrackTrain");
        if let Some(tc) = train {
            let trains: Vec<&MoverStat> = self.movers.values().filter(|m| m.class == tc).collect();
            for t in trains {
                w!(o, "  CFuncTrackTrain ent {} serial {} model {} created {} deleted {:?} originChanges={} angleChanges={} range {:?}..{:?}", t.ent, t.serial, t.model, self.rel(t.created), t.deleted.map(|d| self.rel(d)), t.origin_changes, t.angle_changes, t.min_o, t.max_o);
                for m in self.movers.values().filter(|m| {
                    h_index(m.parent) == Some(t.ent)
                        && m.created <= t.deleted.unwrap_or(u32::MAX)
                        && m.deleted.map(|d| d >= t.created).unwrap_or(true)
                }) {
                    w!(o, "    child (m_hMoveParent -> train): {} ent {} model {} local origin@create {:?} originChanges={} angleChanges={} effects {:?}", self.class_name_of(m.class), m.ent, m.model, m.first_origin, m.origin_changes, m.angle_changes, m.effects_vals);
                }
            }
        }
        // payload: sample train origin, watcher progress and any physics prop near the train
        if let Some(tc) = train {
            let org = id("DT_BaseEntity", "m_vecOrigin");
            if let Some(log) = self.log_of(tc, org) {
                let pts: Vec<&LogE> = log.v.iter().filter(|e| !e.init).collect();
                let watcher = self.class_id_by_name("CTeamTrainWatcher");
                let prog_tl = watcher
                    .map(|w2| self.timeline(w2, id("DT_TeamTrainWatcher", "m_flTotalProgress")));
                let phys = self.class_id_by_name("CPhysicsProp");
                let phys_tl = phys.map(|pc2| self.timeline(pc2, org));
                w!(o, "  CFuncTrackTrain origin samples (every 66th change; {} changes): tick | train origin | watcher m_flTotalProgress | nearest CPhysicsProp origin (dist)", pts.len());
                for e in pts.iter().step_by(66).take(25) {
                    let to = e.val.vec3().unwrap_or([0.0; 3]);
                    let prog = prog_tl
                        .as_ref()
                        .and_then(|tl| tl.by_ent.keys().next().and_then(|k| tl.at(*k, e.tick)))
                        .map(|v| self.fval(&v))
                        .unwrap_or_default();
                    let mut near = String::new();
                    if let Some(ptl) = &phys_tl {
                        let mut best: Option<(f32, [f32; 3], u32)> = None;
                        for k in ptl.by_ent.keys() {
                            if let Some(pv) = ptl.at(*k, e.tick).and_then(|v| v.vec3()) {
                                let d = ((pv[0] - to[0]).powi(2)
                                    + (pv[1] - to[1]).powi(2)
                                    + (pv[2] - to[2]).powi(2))
                                .sqrt();
                                if best.map(|b| d < b.0).unwrap_or(true) {
                                    best = Some((d, pv, *k));
                                }
                            }
                        }
                        if let Some((d, pv, k)) = best {
                            near = format!(
                                "ent {k} ({},{},{}) d={}",
                                ff(pv[0]),
                                ff(pv[1]),
                                ff(pv[2]),
                                ff(d)
                            );
                        }
                    }
                    w!(
                        o,
                        "    {} | ({},{},{}) | {} | {}",
                        self.rel(e.tick),
                        ff(to[0]),
                        ff(to[1]),
                        ff(to[2]),
                        prog,
                        near
                    );
                }
            }
        }
        // any entity parented to anything (for cart props, hats, etc.): count by class
        let mut parented: BTreeMap<(String, String), u64> = BTreeMap::new();
        for m in self.movers.values() {
            if let Some(pi) = h_index(m.parent) {
                let pc2 = self
                    .movers
                    .values()
                    .find(|x| x.ent == pi && x.created <= m.created)
                    .map(|x| self.class_name_of(x.class).to_string())
                    .unwrap_or_else(|| {
                        if pi <= 101 {
                            "player".into()
                        } else {
                            format!("ent{pi}")
                        }
                    });
                *parented
                    .entry((self.class_name_of(m.class).to_string(), pc2))
                    .or_default() += 1;
            }
        }
        w!(
            o,
            "  parented non-player entities (class, parent class): {:?}",
            parented
        );
        w!(
            o,
            "  GameStateAnalyser objectives at end: {}",
            self.gs.state.objectives.len()
        );
        for (e, obj) in &self.gs.state.objectives {
            w!(o, "    {e}: {:?}", obj);
        }

        // ---------------------------------------------------------------- 10 movers
        w!(o);
        w!(o, "## 10. Movers / visibility toggles (non player/weapon/projectile/viewmodel/wearable entities whose origin/angles/effects/render change)");
        let mut movers: Vec<&MoverStat> = self
            .movers
            .values()
            .filter(|m| {
                m.origin_changes
                    + m.angle_changes
                    + m.effects_changes
                    + m.render_changes
                    + m.color_changes
                    + m.model_changes
                    + m.parent_changes
                    + m.leaves
                    > 0
            })
            .collect();
        movers.sort_by_key(|m| (self.class_name_of(m.class).to_string(), m.ent));
        let mut per_class: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for m in &movers {
            let e = per_class
                .entry(self.class_name_of(m.class).to_string())
                .or_default();
            e.0 += 1;
            e.1 += (m.origin_changes + m.angle_changes) as u64;
        }
        w!(
            o,
            "  per class (entities, origin+angle changes): {:?}",
            per_class
        );
        let mut shown: BTreeMap<String, u32> = BTreeMap::new();
        for m in movers {
            let cn = self.class_name_of(m.class).to_string();
            let n = shown.entry(cn.clone()).or_default();
            *n += 1;
            if *n > 30 {
                continue;
            }
            w!(o, "  {cn} ent {} model {} created {} deleted {:?} | origin chg {} angle chg {} effects chg {} vals {:?} render {} color {} model {} parent {} ({} chg) leaves {} reenters {} | origin {:?} range {:?}..{:?} | active {:?}..{}",
                m.ent, m.model, m.created, m.deleted, m.origin_changes, m.angle_changes, m.effects_changes, m.effects_vals, m.render_changes, m.color_changes, m.model_changes, fh(m.parent), m.parent_changes, m.leaves, m.reenters,
                m.first_origin, m.min_o, m.max_o, m.first_change, m.last_change);
        }

        // ---------------------------------------------------------------- 11 pickups
        w!(o);
        w!(o, "## 11. Pickups / dropped items");
        let mut groups: BTreeMap<(String, String), (u64, u64, u64, u64, BTreeSet<i64>)> =
            BTreeMap::new();
        for m in self.movers.values() {
            let cn = self.class_name_of(m.class).to_string();
            let ml = m.model.to_lowercase();
            if ml.contains("items/")
                || ml.contains("medkit")
                || ml.contains("ammopack")
                || ml.contains("plate")
                || cn.contains("AmmoPack")
                || cn.contains("Dropped")
                || cn.contains("Powerup")
                || cn.contains("Pickup")
                || cn.contains("HealthKit")
            {
                let e = groups.entry((cn, m.model.clone())).or_default();
                e.0 += 1;
                e.1 += m.leaves as u64;
                e.2 += m.reenters as u64;
                e.3 += m.effects_changes as u64;
                e.4.extend(m.effects_vals.iter().copied());
            }
        }
        w!(
            o,
            "  (class, model): instances, leaves, reenters, m_fEffects changes, effects values"
        );
        for ((c, m), (n, l, r, ef, vals)) in &groups {
            w!(o, "    {c} {m}: {n} {l} {r} {ef} {:?}", vals);
        }
        if let Some(m) = self
            .movers
            .values()
            .filter(|m| m.model.contains("medkit") && m.leaves > 0)
            .min_by_key(|m| m.ent)
        {
            w!(o, "  lifecycle of medkit ent {} ({}):", m.ent, m.model);
            for l in self
                .lifecycle
                .iter()
                .filter(|l| l.ent as u32 == m.ent && l.serial == m.serial)
                .take(30)
            {
                w!(
                    o,
                    "    {} {}",
                    self.rel(l.tick),
                    ["create", "leave(PVS/dormant)", "re-enter", "delete"][l.kind as usize]
                );
            }
        }

        // ---------------------------------------------------------------- 12 projectiles
        w!(o);
        w!(o, "## 12. Projectiles");
        let mut pc: BTreeMap<u16, Vec<&ProjInfo>> = BTreeMap::new();
        for p in &self.projectiles {
            pc.entry(p.class).or_default().push(p);
        }
        for (c, list) in &pc {
            let mut types: BTreeMap<String, u64> = BTreeMap::new();
            let mut ptypes: BTreeMap<String, u64> = BTreeMap::new();
            let mut crit = 0;
            let mut defl = 0;
            let mut models: BTreeMap<String, u64> = BTreeMap::new();
            let mut owner_player = 0;
            let mut thrower_player = 0;
            let mut launcher_weapon: BTreeMap<String, u64> = BTreeMap::new();
            let mut orig_launcher_weapon = 0;
            let mut life: Vec<u32> = Vec::new();
            for p in list {
                if let Some(t) = p.pipe_type {
                    *types.entry(t.to_string()).or_default() += 1;
                }
                if let Some(t) = p.proj_type {
                    *ptypes
                        .entry(format!(
                            "{}({})",
                            t,
                            PROJECTILE_TYPES.get(t as usize).unwrap_or(&"?")
                        ))
                        .or_default() += 1;
                }
                if p.crit.unwrap_or(0) != 0 {
                    crit += 1;
                }
                if p.deflect_changes > 0 || p.deflected_max > 0 {
                    defl += 1;
                }
                *models.entry(p.model.clone()).or_default() += 1;
                if h_index(p.owner)
                    .map(|i| (1..=101).contains(&i))
                    .unwrap_or(false)
                {
                    owner_player += 1;
                }
                if h_index(p.thrower)
                    .map(|i| (1..=101).contains(&i))
                    .unwrap_or(false)
                {
                    thrower_player += 1;
                }
                let lw = h_index(p.launcher)
                    .and_then(|i| self.weapons.get(&(i, h_serial(p.launcher))))
                    .map(|w2| format!("{}[{}]", self.class_name_of(w2.class), w2.itemdef));
                if let Some(lw) = lw {
                    *launcher_weapon.entry(lw).or_default() += 1;
                }
                if h_index(p.orig_launcher)
                    .and_then(|i| self.weapons.get(&(i, h_serial(p.orig_launcher))))
                    .is_some()
                {
                    orig_launcher_weapon += 1;
                }
                if let Some(d) = p.deleted {
                    life.push(d - p.created);
                }
            }
            life.sort();
            w!(o, "  {}: {} | m_iType {:?} | m_iProjectileType {:?} | crit {} | deflected {} | owner→player {} thrower→player {} | m_hLauncher→weapon {:?} | m_hOriginalLauncher→weapon {} | life ticks min/med/max {:?}/{:?}/{:?} | models {:?}",
                self.class_name_of(*c), list.len(), types, ptypes, crit, defl, owner_player, thrower_player, launcher_weapon, orig_launcher_weapon,
                life.first(), life.get(life.len() / 2), life.last(), models);
        }

        // ---------------------------------------------------------------- 13 temp entities
        w!(o);
        w!(
            o,
            "## 13. Temp entities ({} events in {} svc_TempEntities messages)",
            self.te_total,
            self.te_msgs
        );
        for (c, st) in &self.te_stats {
            w!(o, "### {} (class {}): {} events (reliable {}, delayed {}, no-props {}), first at {:?}", self.class_name_of(*c), c, st.count, st.reliable, st.delayed, st.empty, st.first);
            let mut props: Vec<_> = st.props.iter().collect();
            props.sort_by_key(|(p, _)| self.pname(**p));
            for (pid, (n, vals, ov)) in props {
                let mut v: Vec<&(Val, u64)> = vals.iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1));
                let label = |x: &Val| -> String {
                    let pn = self.prop_only(*pid);
                    match (pn, x.int()) {
                        ("m_iWeaponID", Some(i)) => format!("{}({})", i, weapon_id_name(i)),
                        ("m_iEvent", Some(i)) => format!("{}({})", i, anim_event_name(i)),
                        ("m_iParticleSystemIndex", Some(i)) => format!(
                            "{}({})",
                            i,
                            self.string_tables
                                .get("ParticleEffectNames")
                                .and_then(|t| t.get(i as usize))
                                .cloned()
                                .unwrap_or_default()
                        ),
                        ("m_iEffectName", Some(i)) => format!(
                            "{}({})",
                            i,
                            self.string_tables
                                .get("EffectDispatch")
                                .and_then(|t| t.get(i as usize))
                                .cloned()
                                .unwrap_or_default()
                        ),
                        ("m_nModelIndex" | "m_nModel", Some(i)) => {
                            format!("{}({})", i, self.model_name(i))
                        }
                        _ => self.fprop(*pid, x),
                    }
                };
                w!(
                    o,
                    "    {} [{}] explicitly sent in {} events{}; effective values: {}",
                    self.pname(*pid),
                    self.names.get(pid).map(|n| n.def.as_str()).unwrap_or(""),
                    n,
                    if *ov { " (many distinct)" } else { "" },
                    v.iter()
                        .take(30)
                        .map(|(x, k)| format!("{}×{}", label(x), k))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            for s in &st.samples {
                w!(o, "    sample: {s}");
            }
        }
        // FireBullets analysis
        if !self.fire_bullets.is_empty() {
            w!(
                o,
                "### TEFireBullets analysis ({} events)",
                self.fire_bullets.len()
            );
            let mut by_w: BTreeMap<String, u64> = BTreeMap::new();
            let mut by_wc: BTreeMap<(String, &str), u64> = BTreeMap::new();
            let mut by_mode: BTreeMap<i64, u64> = BTreeMap::new();
            let mut crits = 0;
            let mut dxy: Vec<f32> = Vec::new();
            let mut dz: Vec<f32> = Vec::new();
            let mut dyaw: Vec<f32> = Vec::new();
            let mut dpitch: Vec<f32> = Vec::new();
            let mut per_p: BTreeMap<i64, u64> = BTreeMap::new();
            for f in &self.fire_bullets {
                *by_w.entry(weapon_id_name(f.weapon)).or_default() += 1;
                *by_wc
                    .entry((weapon_id_name(f.weapon), class_name(f.p_class)))
                    .or_default() += 1;
                *by_mode.entry(f.mode).or_default() += 1;
                *per_p.entry(f.player + 1).or_default() += 1;
                if f.crit != 0 {
                    crits += 1;
                }
                if let Some(po) = f.p_origin {
                    dxy.push(
                        ((f.origin[0] - po[0]).powi(2) + (f.origin[1] - po[1]).powi(2)).sqrt(),
                    );
                    dz.push(f.origin[2] - po[2]);
                }
                if let Some(pa) = f.p_ang {
                    let mut dy = (f.ang[1] - pa[1]).rem_euclid(360.0);
                    if dy > 180.0 {
                        dy -= 360.0;
                    }
                    dyaw.push(dy);
                    let mut p = f.ang[0];
                    if p > 180.0 {
                        p -= 360.0;
                    }
                    dpitch.push(p - pa[0]);
                }
            }
            let q = |v: &mut Vec<f32>| -> String {
                if v.is_empty() {
                    return "-".into();
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                format!(
                    "min {} p10 {} med {} p90 {} max {}",
                    ff(v[0]),
                    ff(v[v.len() / 10]),
                    ff(v[v.len() / 2]),
                    ff(v[v.len() * 9 / 10]),
                    ff(v[v.len() - 1])
                )
            };
            w!(o, "  by weapon id: {:?}", by_w);
            w!(o, "  by (weapon id, shooter class): {:?}", by_wc);
            w!(o, "  by mode: {:?}; crit: {}", by_mode, crits);
            w!(
                o,
                "  per player entity (m_iPlayer+1): {:?}",
                per_p
                    .iter()
                    .map(|(e, n)| format!("{}({}):{}", e, pname(*e as u32), n))
                    .collect::<Vec<_>>()
            );
            w!(o, "  |origin.xy - player.xy|: {}", q(&mut dxy));
            w!(o, "  origin.z - player.z: {}", q(&mut dz));
            w!(o, "  yaw(TE) - eyeYaw(player): {}", q(&mut dyaw));
            w!(
                o,
                "  pitch(TE, wrapped) - eyePitch(player): {}",
                q(&mut dpitch)
            );
            for f in self.fire_bullets.iter().take(10) {
                w!(o, "  sample {} player {}({}) {} mode {} seed {} spread {} crit {} origin {:?} ang {:?} | player origin {:?} eye {:?}",
                    self.rel(f.tick), f.player + 1, pname((f.player + 1) as u32), weapon_id_name(f.weapon), f.mode, f.seed, ff(f.spread), f.crit, f.origin, f.ang, f.p_origin, f.p_ang);
            }
            let mut gaps: BTreeMap<u32, u64> = BTreeMap::new();
            let mut last: BTreeMap<i64, u32> = BTreeMap::new();
            for f in &self.fire_bullets {
                if let Some(l) = last.insert(f.player, f.tick) {
                    *gaps.entry((f.tick - l).min(200)).or_default() += 1;
                }
            }
            w!(o, "  per-player tick gap between consecutive FireBullets (gap: count; 200=>=200): {:?}", gaps.iter().take(30).collect::<Vec<_>>());
        }
        if !self.effect_dispatch.is_empty() {
            w!(o, "### TEEffectDispatch by effect name (ParticleEffect resolved via m_nHitBox -> ParticleEffectNames)");
            let mut v: Vec<_> = self.effect_dispatch.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1));
            for (k, n) in v.iter().take(60) {
                w!(o, "    {k}: {n}");
            }
        }
        if !self.anim_events.is_empty() {
            w!(
                o,
                "### TEPlayerAnimEvent analysis ({} events)",
                self.anim_events.len()
            );
            let mut by: BTreeMap<(String, &str), u64> = BTreeMap::new();
            let ptl =
                player_class.map(|c| self.timeline(c, id("DT_TFPlayerClassShared", "m_iClass")));
            for a in &self.anim_events {
                let cls = ptl
                    .as_ref()
                    .and_then(|t| t.at(a.player as u32, a.tick))
                    .and_then(|v| v.int())
                    .unwrap_or(0);
                *by.entry((anim_event_name(a.event), class_name(cls)))
                    .or_default() += 1;
            }
            for ((e, c), n) in &by {
                w!(o, "    {e} / {c}: {n}");
            }
        }

        // ---------------------------------------------------------------- 14 game events
        w!(o);
        w!(o, "## 14. Game events");
        w!(o, "  definitions in demo: {}", self.ev_defs.len());
        for (n, st) in &self.ev_stats {
            w!(
                o,
                "### {} ×{} (first {} last {})",
                n,
                st.count,
                st.first,
                st.last
            );
            if let Some(d) = self.ev_defs.get(n) {
                w!(o, "  definition: {}", d.join(", "));
            }
            for (k, (vals, ov)) in &st.fields {
                let mut v = vals.clone();
                v.sort_by(|a, b| b.1.cmp(&a.1));
                w!(
                    o,
                    "    {k}: {}{}",
                    v.iter()
                        .take(16)
                        .map(|(s, c)| format!("{s}×{c}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    if *ov { ", …" } else { "" }
                );
            }
            for s in &st.samples {
                w!(o, "    sample {s}");
            }
        }
        w!(o, "### events of interest that are defined in the demo's event list but never occur (server-only FireEvent(.., true) or simply not happening)");
        for n in [
            "player_builtobject",
            "player_carryobject",
            "player_dropobject",
            "object_destroyed",
            "object_detonated",
            "object_removed",
            "player_upgradedobject",
            "player_sapped_object",
            "teamplay_point_captured",
            "teamplay_capture_blocked",
            "teamplay_point_startcapture",
            "teamplay_capture_broken",
            "player_chargedeployed",
            "medic_death",
            "object_deflected",
            "player_extinguished",
            "player_ignited",
            "teamplay_round_start",
            "teamplay_round_win",
            "teamplay_setup_finished",
            "teamplay_overtime_begin",
            "teamplay_overtime_end",
            "teamplay_suddendeath_begin",
            "teamplay_round_stalemate",
            "teamplay_game_over",
            "tf_game_over",
            "teamplay_win_panel",
            "teamplay_waiting_begins",
            "teamplay_waiting_ends",
            "teamplay_timer_time_added",
            "teamplay_point_locked",
            "teamplay_point_unlocked",
            "escort_progress",
            "escort_speed",
            "escort_recede",
            "payload_pushed",
            "rocket_jump",
            "rocket_jump_landed",
            "sticky_jump",
            "sticky_jump_landed",
            "player_stunned",
            "player_jarated",
            "player_jarated_fade",
            "player_shield_blocked",
            "player_healed",
            "player_invulned",
            "player_teleported",
            "player_hurt",
            "player_death",
            "arrow_impact",
            "player_damage_dodged",
            "deadringer_cheat_death",
            "sentry_on_go_active",
            "player_buff",
            "deploy_buff_banner",
            "player_highfive_start",
            "player_highfive_success",
            "player_stealsandvich",
            "medigun_shield_blocked",
            "player_mvp",
            "teamplay_alert",
            "localplayer_builtobject",
            "building_healed",
            "player_destroyed_pipebomb",
            "demoman_det_stickies",
            "killed_capping_player",
            "capper_killed",
            "player_healonhit",
            "item_pickup",
            "ammo_pickup",
            "entity_killed",
            "npc_hurt",
            "player_regenerate",
        ] {
            let seen = self.ev_stats.get(n).map(|e| e.count).unwrap_or(0);
            match self.all_ev_defs.get(n) {
                Some(d) => {
                    w!(
                        o,
                        "  {n}: defined ({} fields), occurrences {seen}{}",
                        d.len(),
                        if seen == 0 {
                            format!(" | fields: {}", d.join(", "))
                        } else {
                            String::new()
                        }
                    );
                }
                None => {
                    w!(o, "  {n}: NOT DEFINED in this demo's event list");
                }
            }
        }
        // objective-ish event timeline
        w!(o, "### objective / round event timeline");
        for (t, n, j) in &self.ev_log {
            if n.starts_with("teamplay_")
                || n.starts_with("ctf_")
                || n.contains("escort")
                || n.contains("round")
                || n.contains("stopwatch")
                || n.contains("game_over")
                || n.starts_with("tf_game")
                || n.contains("koth")
                || n.contains("halftime")
                || n.contains("arena")
                || n.contains("competitive")
            {
                if n == "teamplay_broadcast_audio" {
                    continue;
                }
                w!(o, "  {} {} {}", self.rel(*t), n, j);
            }
        }

        // ---------------------------------------------------------------- 15 game state analyser
        w!(o);
        w!(o, "## 15. GameStateAnalyser state at end");
        let gs = &self.gs.state;
        w!(o, "  players={} buildings={} projectiles={} kills={} events={} collisions={} objectives={} world={:?}",
            gs.players.len(), gs.buildings.len(), gs.projectiles.len(), gs.kills.len(), gs.events.len(), gs.collisions.len(), gs.objectives.len(), gs.world);
        for p in gs.players.iter().take(40) {
            w!(o, "    player ent {} class {:?} team {:?} hp {} state {:?} class_data {:?} weapons {:?} handle {}", p.entity, p.class, p.team, p.health, p.state, p.class_data, p.weapons, p.handle);
        }
        for (e, b) in gs.buildings.iter().take(20) {
            w!(o, "    building {e}: {:?}", b);
        }
        o
    }
}

// ---------------------------------------------------------------------------------------------
// Bench
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Combined {
    m: MatchAnalyser,
    g: GameStateAnalyser,
}

impl MessageHandler for Combined {
    type Output = ();
    fn does_handle(t: MessageType) -> bool {
        MatchAnalyser::does_handle(t) || GameStateAnalyser::does_handle(t)
    }
    fn handle_message(&mut self, message: &Message, tick: DemoTick, ps: &ParserState) {
        if MatchAnalyser::does_handle(message.get_message_type()) {
            self.m.handle_message(message, tick, ps);
        }
        if GameStateAnalyser::does_handle(message.get_message_type()) {
            self.g.handle_message(message, tick, ps);
        }
    }
    fn handle_string_entry(&mut self, t: &str, i: usize, e: &StringTableEntry, ps: &ParserState) {
        self.m.handle_string_entry(t, i, e, ps);
        self.g.handle_string_entry(t, i, e, ps);
    }
    fn handle_data_tables(&mut self, t: &[ParseSendTable], c: &[ServerClass], ps: &ParserState) {
        self.m.handle_data_tables(t, c, ps);
        self.g.handle_data_tables(t, c, ps);
    }
    fn handle_packet_meta(&mut self, tick: DemoTick, meta: &MessagePacketMeta, ps: &ParserState) {
        self.m.handle_packet_meta(tick, meta, ps);
        self.g.handle_packet_meta(tick, meta, ps);
    }
    fn into_output(self, _state: &ParserState) {}
}

fn bench(path: &str) {
    let file = fs::read(path).expect("read demo");
    println!(
        "file: {} ({:.1} MB) | after read: {}",
        path,
        file.len() as f64 / 1e6,
        vm_hwm()
    );

    let start = Instant::now();
    let demo = Demo::new(&file);
    let parser = DemoParser::new_with_analyser(demo.get_stream(), GameStateAnalyser::new());
    match parser.parse() {
        Ok((_, state)) => println!(
            "GameStateAnalyser via DemoParser::parse: {:.3}s, players={} kills={} events={} | {}",
            start.elapsed().as_secs_f64(),
            state.players.len(),
            state.kills.len(),
            state.events.len(),
            vm_hwm()
        ),
        Err(e) => println!("GameStateAnalyser parse error: {e}"),
    }

    // lib.rs-style loop: DemoHandler + RawPacketStream, borrowing both analyser outputs per packet
    let start = Instant::now();
    let demo = Demo::new(&file);
    let mut stream = demo.get_stream();
    let header = Header::read(&mut stream).expect("header");
    let mut handler = DemoHandler::with_analyser(Combined::default());
    handler.handle_header(&header);
    let mut packets = RawPacketStream::new(stream);
    let mut n = 0u64;
    let mut sum = 0usize;
    loop {
        match packets.next(handler.get_parser_state()) {
            Ok(Some(p)) => {
                if handler.handle_packet(p).is_err() {
                    break;
                }
                let ps = handler.get_parser_state();
                let a = handler.analyser();
                sum += a.g.borrow_output(ps).players.len() + a.m.borrow_output(ps).users.len();
                n += 1;
            }
            _ => break,
        }
    }
    println!(
        "GameState+Match analysers, lib.rs-style DemoHandler loop: {:.3}s, packets={} (checksum {}) | {}",
        start.elapsed().as_secs_f64(),
        n,
        sum,
        vm_hwm()
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: hl_probe <demo.dem> [report.txt] | hl_probe --bench <demo.dem>");
        std::process::exit(1);
    }
    if args[1] == "--bench" {
        bench(args.get(2).map(|s| s.as_str()).unwrap_or(""));
        return;
    }
    let file = fs::read(&args[1]).expect("read demo");
    let start = Instant::now();
    let demo = Demo::new(&file);
    let mut stream = demo.get_stream();
    let header = Header::read(&mut stream).expect("header");
    let mut handler = DemoHandler::with_analyser(Probe::default());
    handler.handle_header(&header);
    let mut packets = RawPacketStream::new(stream);
    loop {
        match packets.next(handler.get_parser_state()) {
            Ok(Some(packet)) => {
                if let Err(e) = handler.handle_packet(packet) {
                    eprintln!("packet error: {e}");
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => {
                eprintln!("parse error: {e}");
                break;
            }
        }
    }
    let probe = handler.into_output();
    let parsed = start.elapsed();
    eprintln!("parsed in {:.2}s | {}", parsed.as_secs_f32(), vm_hwm());
    let report = probe.report(&header, parsed);
    eprintln!(
        "report built in {:.2}s total",
        start.elapsed().as_secs_f32()
    );
    match args.get(2) {
        Some(path) => fs::write(path, report).expect("write report"),
        None => print!("{report}"),
    }
}
