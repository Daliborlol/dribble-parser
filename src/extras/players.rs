//! `players` group (section 2.5.1): per-player tracks keyed by player index p, plus the
//! `fire` and `anim` event lists and the alignment origin.

use super::out::{ObjB, XNode};
use super::track::{empty_track, record, EventBuilder, Record, TrackBuilder, NONE_U32};
use super::tracker::{ClassInfo, Ent, Role, Val};
use super::weapons::WKind;
use super::{clamp_u16, clamp_u8, to_u8, Ex, Orders, Slots};

pub const D_COND: u32 = 1 << 0;
pub const D_CLOAK: u32 = 1 << 1;
pub const D_INVIS: u32 = 1 << 2;
pub const D_DISGUISE: u32 = 1 << 3;
pub const D_WEAPON: u32 = 1 << 4;
pub const D_WSTATE: u32 = 1 << 5;
pub const D_AMMO: u32 = 1 << 6;
pub const D_MEDIGUN: u32 = 1 << 7;
pub const D_SNIPER: u32 = 1 << 8;
pub const D_FOV: u32 = 1 << 9;
pub const D_VIEW: u32 = 1 << 10;
pub const D_RESOURCE: u32 = 1 << 11;
pub const D_MISC: u32 = 1 << 12;
pub const D_BUILDER: u32 = 1 << 13;
pub const D_LOADOUT: u32 = 1 << 14;
pub const D_SCORING: u32 = 1 << 15;
pub const D_ITEMCHARGE: u32 = 1 << 16;
pub const D_VM0: u32 = 1 << 17;
pub const D_VM1: u32 = 1 << 18;
pub const D_ORIGIN: u32 = 1 << 19;
pub const D_CLASS: u32 = 1 << 20;
pub const D_ALL: u32 = (1 << 19) - 1;
/// Records that depend on owned or active weapons.
pub const D_WEAPON_DEPS: u32 = D_DISGUISE
    | D_WEAPON
    | D_WSTATE
    | D_MEDIGUN
    | D_SNIPER
    | D_MISC
    | D_BUILDER
    | D_LOADOUT
    | D_VM0
    | D_VM1;

record!(CondRec { w0: u32 => "w0", w1: u32 => "w1", w2: u32 => "w2", w3: u32 => "w3", w4: u32 => "w4" });
record!(CloakRec { meter: u8 => "meter" });
record!(InvisRec { complete_tick: f32 => "completeTick" });
record!(DisguiseRec {
    team: u8 => "team", cls: u8 => "cls", target: u8 => "target", health: u16 => "health",
    weapon_def: u16 => "weaponDef", desired_team: u8 => "desiredTeam", desired_cls: u8 => "desiredCls"
});
record!(WeaponRec { id: u32 => "id", def: u16 => "def", cls: u16 => "cls" });
record!(WStateRec { state: u8 => "state", reload: u8 => "reload", clip: i16 => "clip", flags: u8 => "flags", charge_tick: f32 => "chargeTick" });
record!(AmmoRec { primary: u16 => "primary", secondary: u16 => "secondary", metal: u16 => "metal" });
record!(MedigunRec { charge: u16 => "charge", flags: u8 => "flags", target: u8 => "target", def: u16 => "def", resist: u8 => "resist" });
record!(SniperRec { charge: u8 => "charge" });
record!(FovRec { fov: u8 => "fov", start: u8 => "start", tick: f32 => "tick", rate: f32 => "rate" });
record!(ViewRec { eye_z: u8 => "eyeZ", duck: u8 => "duck" });
record!(ResourceRec {
    respawn_tick: f32 => "respawnTick", max_health: u16 => "maxHealth", max_buffed: u16 => "maxBuffed",
    alive: u8 => "alive", score: u16 => "score", damage: u32 => "damage", healing: u32 => "healing"
});
record!(MiscRec {
    healers: u8 => "healers", carrying: u8 => "carrying", flip: u8 => "flip", obs_mode: u8 => "obsMode",
    obs_target: u8 => "obsTarget", revenge: u8 => "revenge", heads: u8 => "heads", charge: u8 => "charge",
    rage: u8 => "rage", stickies: u8 => "stickies", save_me: u8 => "saveMe"
});
record!(BuilderRec { state: u8 => "state", kind: u8 => "type", mode: u8 => "mode" });
record!(LoadoutRec {
    d0: u16 => "d0", d1: u16 => "d1", d2: u16 => "d2", d3: u16 => "d3", d4: u16 => "d4", d5: u16 => "d5",
    d6: u16 => "d6", d7: u16 => "d7", w0: u16 => "w0", w1: u16 => "w1", w2: u16 => "w2", w3: u16 => "w3"
});
record!(ScoringRec {
    kills: u16 => "kills", deaths: u16 => "deaths", assists: u16 => "assists", damage: u32 => "damage",
    heals: u32 => "heals", caps: u16 => "caps", defenses: u16 => "defenses", backstabs: u16 => "backstabs",
    headshots: u16 => "headshots", invulns: u16 => "invulns", teleports: u16 => "teleports", built: u16 => "built",
    destroyed: u16 => "destroyed", dominations: u16 => "dominations", revenges: u16 => "revenges", points: u16 => "points"
});
record!(ItemChargeRec { s0: u8 => "s0", s1: u8 => "s1", s2: u8 => "s2" });
record!(VmRec {
    model: u16 => "model", seq: u8 => "seq", parity: u8 => "parity", muzzle: u8 => "muzzle",
    weapon_def: u16 => "weaponDef", rate: f32 => "rate", nodraw: u8 => "nodraw"
});
record!(FireRec { player: u8 => "player", weapon_def: u16 => "weaponDef" });
record!(AnimRec { player: u8 => "player", event: u8 => "event", data: u16 => "data" });

const SCORING_PROPS: [&str; 16] = [
    "m_iKills",
    "m_iDeaths",
    "m_iKillAssists",
    "m_iDamageDone",
    "m_iHealPoints",
    "m_iCaptures",
    "m_iDefenses",
    "m_iBackstabs",
    "m_iHeadshots",
    "m_iInvulns",
    "m_iTeleports",
    "m_iBuildingsBuilt",
    "m_iBuildingsDestroyed",
    "m_iDominations",
    "m_iRevenge",
    "m_iPoints",
];

#[derive(Default)]
pub struct PlayerSlots {
    /// m_nPlayerCond, m_nPlayerCondEx..Ex4, _condition_bits
    pub cond: [Option<u32>; 6],
    pub cloak: Option<u32>,
    pub invis: Option<u32>,
    pub dis_team: Option<u32>,
    pub dis_class: Option<u32>,
    pub dis_target: Option<u32>,
    /// older demos: DT_TFPlayerShared.m_iDisguiseTargetIndex (entity index)
    pub dis_target_index: Option<u32>,
    pub dis_health: Option<u32>,
    pub dis_weapon: Option<u32>,
    pub des_team: Option<u32>,
    pub des_class: Option<u32>,
    pub active: Option<u32>,
    pub ammo: [Option<u32>; 3],
    pub fov: Option<u32>,
    pub fov_start: Option<u32>,
    pub fov_time: Option<u32>,
    pub fov_rate: Option<u32>,
    pub view_z: Option<u32>,
    pub ducked: Option<u32>,
    pub ducking: Option<u32>,
    pub healers: Option<u32>,
    pub carrying: Option<u32>,
    pub flip: Option<u32>,
    pub obs_mode: Option<u32>,
    pub obs_target: Option<u32>,
    pub revenge: Option<u32>,
    pub heads: Option<u32>,
    pub charge: Option<u32>,
    pub rage: Option<u32>,
    pub save_me: Option<u32>,
    pub my_weapons: Vec<Option<u32>>,
    pub wearables: Vec<Option<u32>>,
    /// `_LPT_m_hMyWearables_8.lengthprop8`: the UtlVector length. Elements at or past it are stale (the server
    /// stops sending them when the vector shrinks), so the loadout reads only the first `length` handles.
    pub wearables_len: Option<u32>,
    pub scoring: [[Option<u32>; 16]; 2],
    pub item_charge: [Option<u32>; 3],
    /// local xy, local z, non-local xy, non-local z
    pub org: [Option<u32>; 4],
    pub life: Option<u32>,
    pub class: Option<u32>,
}

#[inline(never)]
pub fn setup_player(ci: &mut ClassInfo) -> Vec<String> {
    let mut missing = Vec::new();
    let mut f = |ci: &ClassInfo, t: &str, p: &str| -> Option<u32> {
        let r = ci.find(t, p);
        if r.is_none() {
            missing.push(format!("{t}.{p}"));
        }
        r
    };
    let mut s = PlayerSlots {
        cond: [
            f(ci, "DT_TFPlayerShared", "m_nPlayerCond"),
            f(ci, "DT_TFPlayerShared", "m_nPlayerCondEx"),
            f(ci, "DT_TFPlayerShared", "m_nPlayerCondEx2"),
            f(ci, "DT_TFPlayerShared", "m_nPlayerCondEx3"),
            f(ci, "DT_TFPlayerShared", "m_nPlayerCondEx4"),
            f(ci, "DT_TFPlayerConditionListExclusive", "_condition_bits"),
        ],
        cloak: f(ci, "DT_TFPlayerShared", "m_flCloakMeter"),
        invis: f(ci, "DT_TFPlayerShared", "m_flInvisChangeCompleteTime"),
        dis_team: f(ci, "DT_TFPlayerShared", "m_nDisguiseTeam"),
        dis_class: f(ci, "DT_TFPlayerShared", "m_nDisguiseClass"),
        dis_target: ci.find("DT_TFPlayerShared", "m_hDisguiseTarget"),
        dis_target_index: ci.find("DT_TFPlayerShared", "m_iDisguiseTargetIndex"),
        dis_health: f(ci, "DT_TFPlayerShared", "m_iDisguiseHealth"),
        dis_weapon: f(ci, "DT_TFPlayerShared", "m_hDisguiseWeapon"),
        des_team: f(ci, "DT_TFPlayerSharedLocal", "m_nDesiredDisguiseTeam"),
        des_class: f(ci, "DT_TFPlayerSharedLocal", "m_nDesiredDisguiseClass"),
        active: f(ci, "DT_BaseCombatCharacter", "m_hActiveWeapon"),
        ammo: [
            f(ci, "m_iAmmo", "001"),
            f(ci, "m_iAmmo", "002"),
            f(ci, "m_iAmmo", "003"),
        ],
        fov: f(ci, "DT_BasePlayer", "m_iFOV"),
        fov_start: f(ci, "DT_BasePlayer", "m_iFOVStart"),
        fov_time: f(ci, "DT_BasePlayer", "m_flFOVTime"),
        fov_rate: f(ci, "DT_Local", "m_flFOVRate"),
        view_z: f(ci, "DT_LocalPlayerExclusive", "m_vecViewOffset[2]"),
        ducked: f(ci, "DT_Local", "m_bDucked"),
        ducking: f(ci, "DT_Local", "m_bDucking"),
        healers: f(ci, "DT_TFPlayerShared", "m_nNumHealers"),
        carrying: f(ci, "DT_TFPlayerShared", "m_bCarryingObject"),
        flip: f(ci, "DT_TFPlayer", "m_bFlipViewModels"),
        obs_mode: f(ci, "DT_BasePlayer", "m_iObserverMode"),
        obs_target: f(ci, "DT_BasePlayer", "m_hObserverTarget"),
        revenge: f(ci, "DT_TFPlayerShared", "m_iRevengeCrits"),
        heads: f(ci, "DT_TFPlayerShared", "m_iDecapitations"),
        charge: f(ci, "DT_TFPlayerShared", "m_flChargeMeter"),
        rage: f(ci, "DT_TFPlayerSharedLocal", "m_flRageMeter"),
        save_me: f(ci, "DT_TFPlayer", "m_bSaveMeParity"),
        my_weapons: (0..48).map(|i| ci.elem("m_hMyWeapons", i)).collect(),
        wearables: (0..8).map(|i| ci.elem("_ST_m_hMyWearables_8", i)).collect(),
        wearables_len: ci.find("_LPT_m_hMyWearables_8", "lengthprop8"),
        scoring: [[None; 16]; 2],
        item_charge: [
            f(ci, "m_flItemChargeMeter", "000"),
            f(ci, "m_flItemChargeMeter", "001"),
            f(ci, "m_flItemChargeMeter", "002"),
        ],
        org: [
            f(ci, "DT_TFLocalPlayerExclusive", "m_vecOrigin"),
            f(ci, "DT_TFLocalPlayerExclusive", "m_vecOrigin[2]"),
            f(ci, "DT_TFNonLocalPlayerExclusive", "m_vecOrigin"),
            f(ci, "DT_TFNonLocalPlayerExclusive", "m_vecOrigin[2]"),
        ],
        life: f(ci, "DT_BasePlayer", "m_lifeState"),
        class: f(ci, "DT_TFPlayerClassShared", "m_iClass"),
    };
    if s.dis_target.is_none() && s.dis_target_index.is_none() {
        missing.push("DT_TFPlayerShared.m_hDisguiseTarget".into());
    }
    if s.my_weapons.iter().all(|x| x.is_none()) {
        missing.push("m_hMyWeapons.000..047".into());
    }
    if s.wearables.iter().all(|x| x.is_none()) {
        missing.push("_ST_m_hMyWearables_8.000..007".into());
    }
    for (fi, name) in SCORING_PROPS.iter().enumerate() {
        let all = ci.find_all("DT_TFPlayerScoringDataExclusive", name);
        if all.len() < 2 {
            missing.push(format!(
                "DT_TFPlayerScoringDataExclusive.{name} x2 (found {})",
                all.len()
            ));
        }
        for (k, slot) in all.iter().take(2).enumerate() {
            if let Some(row) = s.scoring.get_mut(k) {
                if let Some(cell) = row.get_mut(fi) {
                    *cell = Some(*slot);
                }
            }
        }
    }
    for c in s.cond {
        ci.add_dep(c, D_COND);
    }
    ci.add_dep(s.cloak, D_CLOAK);
    ci.add_dep(s.invis, D_INVIS);
    for x in [
        s.dis_team,
        s.dis_class,
        s.dis_target,
        s.dis_target_index,
        s.dis_health,
        s.dis_weapon,
        s.des_team,
        s.des_class,
    ] {
        ci.add_dep(x, D_DISGUISE);
    }
    ci.add_dep(s.active, D_WEAPON | D_WSTATE | D_BUILDER);
    for a in s.ammo {
        ci.add_dep(a, D_AMMO);
    }
    for x in [s.fov, s.fov_start, s.fov_time, s.fov_rate] {
        ci.add_dep(x, D_FOV);
    }
    for x in [s.view_z, s.ducked, s.ducking] {
        ci.add_dep(x, D_VIEW);
    }
    for x in [
        s.healers,
        s.carrying,
        s.flip,
        s.obs_mode,
        s.obs_target,
        s.revenge,
        s.heads,
        s.charge,
        s.rage,
        s.save_me,
    ] {
        ci.add_dep(x, D_MISC);
    }
    for x in s.my_weapons.iter().chain(s.wearables.iter()) {
        ci.add_dep(*x, D_LOADOUT);
    }
    ci.add_dep(s.wearables_len, D_LOADOUT);
    for row in s.scoring {
        for x in row {
            ci.add_dep(x, D_SCORING);
        }
    }
    for x in s.item_charge {
        ci.add_dep(x, D_ITEMCHARGE);
    }
    for x in s.org {
        ci.add_dep(x, D_ORIGIN);
    }
    ci.add_dep(s.life, D_ORIGIN);
    ci.add_dep(s.class, D_CLASS);
    // keep only the props read here
    let mut keep: Vec<Option<u32>> = Vec::new();
    keep.extend(s.cond);
    keep.extend([
        s.cloak,
        s.invis,
        s.dis_team,
        s.dis_class,
        s.dis_target,
        s.dis_target_index,
        s.dis_health,
        s.dis_weapon,
        s.des_team,
        s.des_class,
        s.active,
        s.fov,
        s.fov_start,
        s.fov_time,
        s.fov_rate,
        s.view_z,
        s.ducked,
        s.ducking,
        s.healers,
        s.carrying,
        s.flip,
        s.obs_mode,
        s.obs_target,
        s.revenge,
        s.heads,
        s.charge,
        s.rage,
        s.save_me,
        s.life,
        s.class,
    ]);
    keep.extend(s.ammo);
    keep.extend(s.my_weapons.iter().copied());
    keep.extend(s.wearables.iter().copied());
    keep.push(s.wearables_len);
    for row in s.scoring {
        keep.extend(row);
    }
    keep.extend(s.item_charge);
    keep.extend(s.org);
    ci.keep_only(&keep);
    ci.slots = Slots::Player(Box::new(s));
    missing
}

/// `CTFPlayerResource` arrays, element = player entity id.
pub struct ResourceSlots {
    pub respawn: Vec<Option<u32>>,
    pub max_health: Vec<Option<u32>>,
    pub max_buffed: Vec<Option<u32>>,
    pub alive: Vec<Option<u32>>,
    pub score: Vec<Option<u32>>,
    pub damage: Vec<Option<u32>>,
    pub healing: Vec<Option<u32>>,
}

const RES_ELEMS: usize = 102;

#[inline(never)]
pub fn setup_resource(ci: &mut ClassInfo) -> Vec<String> {
    let arr = |ci: &ClassInfo, t: &str| -> Vec<Option<u32>> {
        (0..RES_ELEMS).map(|e| ci.elem(t, e)).collect()
    };
    let s = ResourceSlots {
        respawn: arr(ci, "m_flNextRespawnTime"),
        max_health: arr(ci, "m_iMaxHealth"),
        max_buffed: arr(ci, "m_iMaxBuffedHealth"),
        alive: arr(ci, "m_bAlive"),
        score: arr(ci, "m_iTotalScore"),
        damage: arr(ci, "m_iDamage"),
        healing: arr(ci, "m_iHealing"),
    };
    let mut missing = Vec::new();
    let mut keep = Vec::new();
    for (name, v) in [
        ("m_flNextRespawnTime", &s.respawn),
        ("m_iMaxHealth", &s.max_health),
        ("m_iMaxBuffedHealth", &s.max_buffed),
        ("m_bAlive", &s.alive),
        ("m_iTotalScore", &s.score),
        ("m_iDamage", &s.damage),
        ("m_iHealing", &s.healing),
    ] {
        if v.iter().all(|x| x.is_none()) {
            missing.push(format!("{name}.NNN"));
        }
        for (e, slot) in v.iter().enumerate() {
            // dep = element + 1
            if let Some(sl) = slot {
                if let Some(d) = ci.dep.get_mut(*sl as usize) {
                    *d = e as u32 + 1;
                }
            }
            keep.push(*slot);
        }
    }
    ci.keep_only(&keep);
    ci.slots = Slots::PlayerResource(Box::new(s));
    missing
}

pub struct PlState {
    pub dirty: Vec<u32>,
    dirty_list: Vec<usize>,
    pub cond: TrackBuilder<CondRec>,
    pub cloak: TrackBuilder<CloakRec>,
    pub invis: TrackBuilder<InvisRec>,
    pub disguise: TrackBuilder<DisguiseRec>,
    pub weapon: TrackBuilder<WeaponRec>,
    pub wstate: TrackBuilder<WStateRec>,
    pub ammo: TrackBuilder<AmmoRec>,
    pub medigun: TrackBuilder<MedigunRec>,
    pub sniper: TrackBuilder<SniperRec>,
    pub fov: TrackBuilder<FovRec>,
    pub view: TrackBuilder<ViewRec>,
    pub resource: TrackBuilder<ResourceRec>,
    pub misc: TrackBuilder<MiscRec>,
    pub builder: TrackBuilder<BuilderRec>,
    pub loadout: TrackBuilder<LoadoutRec>,
    pub scoring: [TrackBuilder<ScoringRec>; 2],
    pub kill_decreases: [u32; 2],
    pub item_charge: TrackBuilder<ItemChargeRec>,
    pub vm0: TrackBuilder<VmRec>,
    pub vm1: TrackBuilder<VmRec>,
    pub fire: EventBuilder<FireRec>,
    pub anim: EventBuilder<AnimRec>,
    /// weapon provisional ids owned per player (alive)
    pub owned: Vec<Vec<u32>>,
    /// viewmodel entity per player and viewmodel index
    pub vm_ent: Vec<[Option<u32>; 2]>,
    pub res_ent: Option<u32>,
    /// last write of the origin props: (xy from non-local, z from non-local)
    pub org_src: Vec<[bool; 2]>,
    /// classes seen per player (native diagnostics)
    pub classes: Vec<Vec<u8>>,
    /// medigun records with an item def pushed while the player was a spy (native diagnostics)
    pub spy_medigun_defs: u32,
}

impl PlState {
    pub fn new(pc: usize, ticks: u32) -> PlState {
        PlState {
            dirty: vec![0; pc],
            dirty_list: Vec::new(),
            cond: TrackBuilder::new(ticks),
            cloak: TrackBuilder::new(ticks),
            invis: TrackBuilder::new(ticks),
            disguise: TrackBuilder::new(ticks),
            weapon: TrackBuilder::new(ticks),
            wstate: TrackBuilder::new(ticks),
            ammo: TrackBuilder::new(ticks),
            medigun: TrackBuilder::new(ticks),
            sniper: TrackBuilder::new(ticks),
            fov: TrackBuilder::new(ticks),
            view: TrackBuilder::new(ticks),
            resource: TrackBuilder::new(ticks),
            misc: TrackBuilder::new(ticks),
            builder: TrackBuilder::new(ticks),
            loadout: TrackBuilder::new(ticks),
            scoring: [TrackBuilder::new(ticks), TrackBuilder::new(ticks)],
            kill_decreases: [0, 0],
            item_charge: TrackBuilder::new(ticks),
            vm0: TrackBuilder::new(ticks),
            vm1: TrackBuilder::new(ticks),
            fire: EventBuilder::new(ticks),
            anim: EventBuilder::new(ticks),
            owned: vec![Vec::new(); pc],
            vm_ent: vec![[None, None]; pc],
            res_ent: None,
            org_src: vec![[true, true]; pc],
            classes: vec![Vec::new(); pc],
            spy_medigun_defs: 0,
        }
    }

    pub fn mark(&mut self, p: usize, bits: u32) {
        if bits == 0 {
            return;
        }
        if let Some(d) = self.dirty.get_mut(p) {
            if *d == 0 {
                self.dirty_list.push(p);
            }
            *d |= bits;
        }
    }
}

#[derive(Default)]
struct PlRecs {
    cond: Option<CondRec>,
    cloak: Option<CloakRec>,
    invis: Option<InvisRec>,
    disguise: Option<DisguiseRec>,
    weapon: Option<WeaponRec>,
    wstate: Option<WStateRec>,
    ammo: Option<AmmoRec>,
    medigun: Option<MedigunRec>,
    sniper: Option<SniperRec>,
    fov: Option<FovRec>,
    view: Option<ViewRec>,
    resource: Option<ResourceRec>,
    misc: Option<MiscRec>,
    builder: Option<BuilderRec>,
    loadout: Option<LoadoutRec>,
    scoring: [Option<ScoringRec>; 2],
    item_charge: Option<ItemChargeRec>,
    vm0: Option<VmRec>,
    vm1: Option<VmRec>,
}

impl Ex {
    pub(crate) fn player_ent(&self, p: usize) -> Option<&Ent> {
        let e = *self.p_ent.get(p)?;
        if e == 0 {
            return None;
        }
        let ent = self.ents.get(e as usize)?.as_ref()?;
        if ent.role == Role::Player {
            Some(ent)
        } else {
            None
        }
    }

    pub(crate) fn pl_create(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        _st: i64,
    ) {
        let p = self.p_of_ent(idx);
        if p == 255 {
            return;
        }
        ent.role = Role::Player;
        ent.slot = p as u32;
        self.pl.mark(p as usize, D_ALL);
        self.pl_note_origin(p as usize, ci, changed);
        self.pl_note_class(p as usize, ent, ci);
    }

    fn pl_note_class(&mut self, p: usize, ent: &Ent, ci: &ClassInfo) {
        let super::Slots::Player(s) = &ci.slots else {
            return;
        };
        let c = to_u8(ent.i0(s.class));
        if let Some(list) = self.pl.classes.get_mut(p) {
            if c > 0 && !list.contains(&c) {
                list.push(c);
            }
        }
    }

    fn pl_class_now(&self, p: usize) -> u8 {
        let Some(ent) = self.player_ent(p) else {
            return 0;
        };
        let Some(ci) = self.class_info(ent.class) else {
            return 0;
        };
        let Slots::Player(s) = &ci.slots else {
            return 0;
        };
        to_u8(ent.i0(s.class))
    }

    pub(crate) fn pl_update(
        &mut self,
        _idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        _st: i64,
    ) {
        let p = ent.slot as usize;
        let mut bits = 0u32;
        for (i, _) in changed {
            bits |= ci.dep.get(*i as usize).copied().unwrap_or(0);
        }
        if bits & D_SCORING != 0 {
            if let super::Slots::Player(s) = &ci.slots {
                for (k, row) in s.scoring.iter().enumerate() {
                    let Some(kslot) = row.first().copied().flatten() else {
                        continue;
                    };
                    for (i, old) in changed {
                        if *i == kslot {
                            let new = ent.i0(Some(kslot));
                            if let Some(o) = old.int() {
                                if new < o {
                                    if let Some(c) = self.pl.kill_decreases.get_mut(k) {
                                        *c += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if bits & D_ORIGIN != 0 {
            self.pl_note_origin(p, ci, changed);
        }
        if bits & D_CLASS != 0 {
            self.pl_note_class(p, ent, ci);
        }
        self.pl.mark(p, bits & D_ALL);
    }

    fn pl_note_origin(&mut self, p: usize, ci: &ClassInfo, changed: &[(u32, Val)]) {
        let super::Slots::Player(s) = &ci.slots else {
            return;
        };
        let Some(src) = self.pl.org_src.get_mut(p) else {
            return;
        };
        for (i, _) in changed {
            let i = Some(*i);
            if i == s.org[0] {
                src[0] = false;
            } else if i == s.org[2] {
                src[0] = true;
            } else if i == s.org[1] {
                src[1] = false;
            } else if i == s.org[3] {
                src[1] = true;
            }
        }
    }

    pub(crate) fn pl_end(&mut self, _ent: &Ent) {}

    pub(crate) fn pr_create(&mut self, idx: u32, ent: &mut Ent, _ci: &ClassInfo, _st: i64) {
        ent.role = Role::PlayerResource;
        self.pl.res_ent = Some(idx);
        for p in 0..self.pc() {
            self.pl.mark(p, D_RESOURCE);
        }
    }

    pub(crate) fn pr_update(
        &mut self,
        _idx: u32,
        _ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        _st: i64,
    ) {
        for (i, _) in changed {
            let d = ci.dep.get(*i as usize).copied().unwrap_or(0);
            if d > 0 {
                let p = self.p_of_ent(d - 1);
                if p != 255 {
                    self.pl.mark(p as usize, D_RESOURCE);
                }
            }
        }
    }

    /// Alignment origin of player p (section 2.5.1): present and `m_lifeState == 0`.
    pub(crate) fn pl_align_pos(&self, p: usize) -> Option<[f32; 3]> {
        let ent = self.player_ent(p)?;
        let ci = self.class_info(ent.class)?;
        let Slots::Player(s) = &ci.slots else {
            return None;
        };
        if ent.i(s.life) != Some(0) {
            return None;
        }
        let src = self.pl.org_src.get(p).copied().unwrap_or([true, true]);
        let (xy_a, xy_b) = if src[0] {
            (s.org[2], s.org[0])
        } else {
            (s.org[0], s.org[2])
        };
        let (z_a, z_b) = if src[1] {
            (s.org[3], s.org[1])
        } else {
            (s.org[1], s.org[3])
        };
        let xy = ent.v(xy_a).or_else(|| ent.v(xy_b))?;
        let z = ent.f(z_a).or_else(|| ent.f(z_b))?;
        Some([xy[0], xy[1], z])
    }

    #[inline(never)]
    pub(crate) fn pl_flush(&mut self, st: i64) {
        if self.pl.dirty_list.is_empty() {
            return;
        }
        let list = std::mem::take(&mut self.pl.dirty_list);
        for p in list {
            let bits = self.pl.dirty.get(p).copied().unwrap_or(0);
            if let Some(d) = self.pl.dirty.get_mut(p) {
                *d = 0;
            }
            if !self.incl.players {
                continue;
            }
            let recs = self.pl_records(p, bits);
            let recs_class = if recs.medigun.is_some() {
                self.pl_class_now(p)
            } else {
                0
            };
            let pl = &mut self.pl;
            if let Some(r) = recs.cond {
                pl.cond.push(p, st, r);
            }
            if let Some(r) = recs.cloak {
                pl.cloak.push(p, st, r);
            }
            if let Some(r) = recs.invis {
                pl.invis.push(p, st, r);
            }
            if let Some(r) = recs.disguise {
                pl.disguise.push(p, st, r);
            }
            if let Some(r) = recs.weapon {
                pl.weapon.push(p, st, r);
            }
            if let Some(r) = recs.wstate {
                pl.wstate.push(p, st, r);
            }
            if let Some(r) = recs.ammo {
                pl.ammo.push(p, st, r);
            }
            if let Some(r) = recs.medigun {
                if r.def != 65535 && recs_class == 8 {
                    pl.spy_medigun_defs += 1;
                }
                pl.medigun.push(p, st, r);
            }
            if let Some(r) = recs.sniper {
                pl.sniper.push(p, st, r);
            }
            if let Some(r) = recs.fov {
                pl.fov.push(p, st, r);
            }
            if let Some(r) = recs.view {
                pl.view.push(p, st, r);
            }
            if let Some(r) = recs.resource {
                pl.resource.push(p, st, r);
            }
            if let Some(r) = recs.misc {
                pl.misc.push(p, st, r);
            }
            if let Some(r) = recs.builder {
                pl.builder.push(p, st, r);
            }
            if let Some(r) = recs.loadout {
                pl.loadout.push(p, st, r);
            }
            for (k, r) in recs.scoring.iter().enumerate() {
                if let (Some(r), Some(t)) = (r, pl.scoring.get_mut(k)) {
                    t.push(p, st, *r);
                }
            }
            if let Some(r) = recs.item_charge {
                pl.item_charge.push(p, st, r);
            }
            if let Some(r) = recs.vm0 {
                pl.vm0.push(p, st, r);
            }
            if let Some(r) = recs.vm1 {
                pl.vm1.push(p, st, r);
            }
        }
    }

    /// Newest alive, non-disguise owned weapon of kind `k` (rule 2.3.12).
    pub(crate) fn owned_weapon(&self, p: usize, pred: impl Fn(WKind) -> bool) -> Option<u32> {
        let owned = self.pl.owned.get(p)?;
        owned
            .iter()
            .copied()
            .filter(|w| {
                self.wp
                    .inst
                    .get(*w)
                    .map(|r| r.deleted.is_none() && r.data.disguise == 0 && pred(r.data.kind))
                    .unwrap_or(false)
            })
            .max()
    }

    #[inline(never)]
    fn pl_records(&self, p: usize, bits: u32) -> PlRecs {
        let mut out = PlRecs::default();
        if bits & (D_VM0 | D_VM1) != 0 {
            if bits & D_VM0 != 0 {
                out.vm0 = Some(self.vm_record(p, 0));
            }
            if bits & D_VM1 != 0 {
                out.vm1 = Some(self.vm_record(p, 1));
            }
        }
        if bits & D_RESOURCE != 0 {
            out.resource = self.resource_record(p);
        }
        let Some(ent) = self.player_ent(p) else {
            return out;
        };
        let Some(ci) = self.class_info(ent.class) else {
            return out;
        };
        let Slots::Player(s) = &ci.slots else {
            return out;
        };
        if bits & D_COND != 0 {
            let w = |i: usize| ent.i0(s.cond.get(i).copied().flatten()) as u32;
            out.cond = Some(CondRec {
                w0: w(0) | w(5),
                w1: w(1),
                w2: w(2),
                w3: w(3),
                w4: w(4),
            });
        }
        if bits & D_CLOAK != 0 {
            let v = ent.f0(s.cloak) as f64;
            out.cloak = Some(CloakRec {
                meter: v.clamp(0.0, 100.0).ceil() as u8,
            });
        }
        if bits & D_INVIS != 0 {
            out.invis = Some(InvisRec {
                complete_tick: self.tick_f(ent.f0(s.invis)),
            });
        }
        if bits & D_DISGUISE != 0 {
            let wdef = self
                .slot_of_handle(ent.h(s.dis_weapon), Role::Weapon)
                .and_then(|w| self.wp.inst.get(w))
                .map(|r| r.data.def)
                .unwrap_or(65535);
            out.disguise = Some(DisguiseRec {
                team: to_u8(ent.i0(s.dis_team)),
                cls: to_u8(ent.i0(s.dis_class)),
                target: if s.dis_target.is_some() {
                    self.p_of_handle(ent.h(s.dis_target))
                } else {
                    self.p_of_ent(ent.i0(s.dis_target_index).clamp(0, 4095) as u32)
                },
                health: ent.i0(s.dis_health).clamp(0, 65534) as u16,
                weapon_def: wdef,
                desired_team: to_u8(ent.i0(s.des_team)),
                desired_cls: to_u8(ent.i0(s.des_class)),
            });
        }
        let active = self.slot_of_handle(ent.h(s.active), Role::Weapon);
        if bits & D_WEAPON != 0 {
            out.weapon = Some(
                match active.and_then(|w| self.wp.inst.get(w).map(|r| (w, r))) {
                    Some((w, r)) => WeaponRec {
                        id: w,
                        def: r.data.def,
                        cls: r.data.cls,
                    },
                    None => WeaponRec {
                        id: NONE_U32,
                        def: 65535,
                        cls: 65535,
                    },
                },
            );
        }
        if bits & D_WSTATE != 0 {
            let rec = active
                .and_then(|w| self.with_weapon(w, |we, ws| self.wstate_of(we, ws)))
                .unwrap_or(WStateRec {
                    state: 0,
                    reload: 0,
                    clip: -1,
                    flags: 0,
                    charge_tick: f32::NAN,
                });
            out.wstate = Some(rec);
        }
        if bits & D_AMMO != 0 {
            out.ammo = Some(AmmoRec {
                primary: clamp_u16(ent.i0(s.ammo[0])),
                secondary: clamp_u16(ent.i0(s.ammo[1])),
                metal: clamp_u16(ent.i0(s.ammo[2])),
            });
        }
        if bits & D_MEDIGUN != 0 {
            let rec = self
                .owned_weapon(p, |k| k == WKind::Medigun)
                .and_then(|w| {
                    let def = self.wp.inst.get(w).map(|r| r.data.def).unwrap_or(65535);
                    self.with_weapon(w, |we, ws| {
                        let charge = we
                            .f(ws.med_charge_nl)
                            .or_else(|| we.f(ws.med_charge_l))
                            .unwrap_or(0.0);
                        let flags = (we.b(ws.med_healing) as u8)
                            | ((we.b(ws.med_attacking) as u8) << 1)
                            | ((we.b(ws.med_release) as u8) << 2)
                            | ((we.b(ws.med_holstered) as u8) << 3);
                        MedigunRec {
                            charge: ((charge as f64).clamp(0.0, 1.0) * 1000.0).round() as u16,
                            flags,
                            target: self.p_of_handle(we.h(ws.med_target)),
                            def,
                            resist: to_u8(we.i0(ws.med_resist)),
                        }
                    })
                })
                .unwrap_or(MedigunRec {
                    charge: 0,
                    flags: 0,
                    target: 255,
                    def: 65535,
                    resist: 0,
                });
            out.medigun = Some(rec);
        }
        if bits & D_SNIPER != 0 {
            let charge = self
                .owned_weapon(p, |k| k == WKind::Sniper)
                .and_then(|w| self.with_weapon(w, |we, ws| we.f0(ws.sniper_charge)))
                .unwrap_or(0.0);
            out.sniper = Some(SniperRec {
                charge: (charge as f64).round().clamp(0.0, 150.0) as u8,
            });
        }
        if bits & D_FOV != 0 {
            out.fov = Some(FovRec {
                fov: to_u8(ent.i0(s.fov)),
                start: to_u8(ent.i0(s.fov_start)),
                tick: self.tick_f(ent.f0(s.fov_time)),
                rate: ent.f0(s.fov_rate),
            });
        }
        if bits & D_VIEW != 0 {
            out.view = Some(ViewRec {
                eye_z: clamp_u8(ent.f0(s.view_z) as f64),
                duck: (ent.b(s.ducked) as u8) | ((ent.b(s.ducking) as u8) << 1),
            });
        }
        if bits & D_MISC != 0 {
            let stickies = self
                .owned_weapon(p, |k| k == WKind::Pipebomb)
                .and_then(|w| self.with_weapon(w, |we, ws| we.i0(ws.pipe_count)))
                .unwrap_or(0);
            out.misc = Some(MiscRec {
                healers: to_u8(ent.i0(s.healers)),
                carrying: to_u8(ent.i0(s.carrying)),
                flip: to_u8(ent.i0(s.flip)),
                obs_mode: to_u8(ent.i0(s.obs_mode)),
                obs_target: self.p_of_handle(ent.h(s.obs_target)),
                revenge: to_u8(ent.i0(s.revenge)),
                heads: to_u8(ent.i0(s.heads)),
                charge: clamp_u8((ent.f0(s.charge) as f64).clamp(0.0, 100.0)),
                rage: clamp_u8((ent.f0(s.rage) as f64).clamp(0.0, 100.0)),
                stickies: to_u8(stickies),
                save_me: to_u8(ent.i0(s.save_me)),
            });
        }
        if bits & D_BUILDER != 0 {
            let builder = self.pl.owned.get(p).and_then(|owned| {
                owned
                    .iter()
                    .copied()
                    .filter(|w| {
                        self.wp
                            .inst
                            .get(*w)
                            .map(|r| {
                                r.deleted.is_none()
                                    && r.data.disguise == 0
                                    && r.data.kind == WKind::Builder
                            })
                            .unwrap_or(false)
                            && self
                                .with_weapon(*w, |we, ws| we.i0(ws.obj_type) != 3)
                                .unwrap_or(false)
                    })
                    .max()
            });
            let rec = builder
                .and_then(|w| {
                    self.with_weapon(w, |we, ws| BuilderRec {
                        state: if active == Some(w) {
                            to_u8(we.i0(ws.build_state))
                        } else {
                            0
                        },
                        kind: to_u8(we.i0(ws.obj_type)),
                        mode: to_u8(we.i0(ws.obj_mode)),
                    })
                })
                .unwrap_or(BuilderRec {
                    state: 0,
                    kind: 255,
                    mode: 0,
                });
            out.builder = Some(rec);
        }
        if bits & D_LOADOUT != 0 {
            out.loadout = Some(self.loadout_record(ent, s));
        }
        if bits & D_SCORING != 0 {
            for k in 0..2 {
                let Some(row) = s.scoring.get(k) else {
                    continue;
                };
                let g = |f: usize| ent.i0(row.get(f).copied().flatten());
                let u16c = |f: usize| clamp_u16(g(f));
                let u32c = |f: usize| g(f).clamp(0, u32::MAX as i64) as u32;
                if let Some(cell) = out.scoring.get_mut(k) {
                    *cell = Some(ScoringRec {
                        kills: u16c(0),
                        deaths: u16c(1),
                        assists: u16c(2),
                        damage: u32c(3),
                        heals: u32c(4),
                        caps: u16c(5),
                        defenses: u16c(6),
                        backstabs: u16c(7),
                        headshots: u16c(8),
                        invulns: u16c(9),
                        teleports: u16c(10),
                        built: u16c(11),
                        destroyed: u16c(12),
                        dominations: u16c(13),
                        revenges: u16c(14),
                        points: u16c(15),
                    });
                }
            }
        }
        if bits & D_ITEMCHARGE != 0 {
            let c = |i: usize| {
                clamp_u8((ent.f0(s.item_charge.get(i).copied().flatten()) as f64).clamp(0.0, 100.0))
            };
            out.item_charge = Some(ItemChargeRec {
                s0: c(0),
                s1: c(1),
                s2: c(2),
            });
        }
        out
    }

    fn wstate_of(&self, we: &Ent, ws: &super::weapons::WeaponSlots) -> WStateRec {
        let state = we
            .i(ws.state_flame)
            .or_else(|| we.i(ws.state_minigun))
            .unwrap_or(0);
        let clip = match we.i(ws.clip1) {
            Some(raw) => (raw - 1).clamp(-1, i16::MAX as i64) as i16,
            None => -1,
        };
        let flags = (we.b(ws.backstab) as u8)
            | (((we.b(ws.crit_fire) || we.b(ws.crit_shot)) as u8) << 1)
            | ((we.b(ws.lowered) as u8) << 2)
            | ((we.b(ws.hit_target) as u8) << 3);
        let charge_tick = match we.f(ws.charge_begin) {
            Some(t) if t != 0.0 => self.tick_f(t),
            _ => f32::NAN,
        };
        WStateRec {
            state: to_u8(state),
            reload: to_u8(we.i0(ws.reload)),
            clip,
            flags,
            charge_tick,
        }
    }

    #[inline(never)]
    fn loadout_record(&self, ent: &Ent, s: &PlayerSlots) -> LoadoutRec {
        let mut d = [65535u16; 8];
        let mut n = 0usize;
        for slot in &s.my_weapons {
            if n >= 8 {
                break;
            }
            let Some(h) = ent.h(*slot) else { continue };
            let def = self
                .slot_of_handle(Some(h), Role::Weapon)
                .and_then(|w| self.wp.inst.get(w))
                .map(|r| r.data.def)
                .unwrap_or(65535);
            if let Some(cell) = d.get_mut(n) {
                *cell = def;
            }
            n += 1;
        }
        let mut w = [65535u16; 4];
        let mut m = 0usize;
        // UtlVector length; without the length prop (older demos) every element counts
        let len = match s.wearables_len {
            Some(_) => ent.i0(s.wearables_len).clamp(0, 8) as usize,
            None => s.wearables.len(),
        };
        for slot in s.wearables.iter().take(len) {
            if m >= 4 {
                break;
            }
            let Some(h) = ent.h(*slot) else { continue };
            let Some(wear) = self
                .ent_by_handle(Some(h))
                .filter(|e| e.role == Role::Wearable && !self.wearable_is_disguise(e))
            else {
                continue;
            };
            let def = self.wearable_def(wear);
            let ok = def != 65535
                && match &self.wearable_defs {
                    Some(set) => set.contains(&def),
                    None => true,
                };
            if ok {
                if let Some(cell) = w.get_mut(m) {
                    *cell = def;
                }
                m += 1;
            }
        }
        let [d0, d1, d2, d3, d4, d5, d6, d7] = d;
        let [w0, w1, w2, w3] = w;
        LoadoutRec {
            d0,
            d1,
            d2,
            d3,
            d4,
            d5,
            d6,
            d7,
            w0,
            w1,
            w2,
            w3,
        }
    }

    #[inline(never)]
    fn resource_record(&self, p: usize) -> Option<ResourceRec> {
        let e = *self.p_ent.get(p)? as usize;
        let ent = self.ents.get(self.pl.res_ent? as usize)?.as_ref()?;
        let ci = self.class_info(ent.class)?;
        let Slots::PlayerResource(s) = &ci.slots else {
            return None;
        };
        let at = |v: &Vec<Option<u32>>| v.get(e).copied().flatten();
        Some(ResourceRec {
            respawn_tick: self.tick_f_nan(ent.f0(at(&s.respawn))),
            max_health: clamp_u16(ent.i0(at(&s.max_health))),
            max_buffed: clamp_u16(ent.i0(at(&s.max_buffed))),
            alive: to_u8(ent.i0(at(&s.alive))),
            score: clamp_u16(ent.i0(at(&s.score))),
            damage: ent.i0(at(&s.damage)).clamp(0, u32::MAX as i64) as u32,
            healing: ent.i0(at(&s.healing)).clamp(0, u32::MAX as i64) as u32,
        })
    }

    #[inline(never)]
    pub(crate) fn players_out(&mut self, ord: &Orders) -> XNode {
        let pc = self.pc();
        let pl = &mut self.pl;
        // scoring element (section 2.6.4): the element whose m_iKills decreased fewer times
        let dec = pl.kill_decreases;
        let element = if dec[1] < dec[0] { 1 } else { 0 };
        if dec[0] == dec[1] && (pl.scoring[0].total() > 0 || pl.scoring[1].total() > 0) {
            self.warnings.push(format!(
                "scoring elements decreased equally often ({} kills decreases each); using element 0",
                dec[0]
            ));
        }
        self.counts.insert("scoring.element".into(), element as f64);
        self.counts
            .insert("scoring.decreases0".into(), dec[0] as f64);
        self.counts
            .insert("scoring.decreases1".into(), dec[1] as f64);
        let pl = &mut self.pl;
        pl.weapon.map_records(|r| r.id = ord.weapons.map(r.id));
        let [sc0, sc1] = std::mem::replace(
            &mut pl.scoring,
            [TrackBuilder::new(1), TrackBuilder::new(1)],
        );
        let scoring = if element == 1 { sc1 } else { sc0 };
        let fire = std::mem::replace(&mut pl.fire, EventBuilder::new(1));
        let anim = std::mem::replace(&mut pl.anim, EventBuilder::new(1));
        macro_rules! tr {
            ($f:ident) => {
                XNode::Obj(
                    std::mem::replace(&mut pl.$f, TrackBuilder::new(1)).into_fields(pc, None),
                )
            };
        }
        let entity: Vec<u32> = self.p_ent.clone();
        ObjB::new()
            .put("count", XNode::Num(pc as f64))
            .put("entityId", XNode::U32(entity))
            .put("cond", tr!(cond))
            .put("cloak", tr!(cloak))
            .put("invis", tr!(invis))
            .put("disguise", tr!(disguise))
            .put("weapon", tr!(weapon))
            .put("wstate", tr!(wstate))
            .put("ammo", tr!(ammo))
            .put("medigun", tr!(medigun))
            .put("sniper", tr!(sniper))
            .put("fov", tr!(fov))
            .put("view", tr!(view))
            .put("resource", tr!(resource))
            .put("misc", tr!(misc))
            .put("builder", tr!(builder))
            .put("loadout", tr!(loadout))
            .put("scoring", XNode::Obj(scoring.into_fields(pc, None)))
            .put("itemCharge", tr!(item_charge))
            .put("vm0", tr!(vm0))
            .put("vm1", tr!(vm1))
            .put("fire", XNode::Obj(fire.into_fields()))
            .put("anim", XNode::Obj(anim.into_fields()))
            .build()
    }
}

/// `players` of an excluded group.
#[inline(never)]
pub fn players_empty(pc: usize, p_ent: &[u32]) -> XNode {
    fn t<R: Record>(pc: usize) -> XNode {
        XNode::Obj(empty_track::<R>(pc))
    }
    let ev = |cols: Vec<(String, XNode)>| -> XNode {
        let mut f = vec![("tick".to_string(), XNode::U32(Vec::new()))];
        f.extend(cols);
        XNode::Obj(f)
    };
    ObjB::new()
        .put("count", XNode::Num(pc as f64))
        .put("entityId", XNode::U32(p_ent.to_vec()))
        .put("cond", t::<CondRec>(pc))
        .put("cloak", t::<CloakRec>(pc))
        .put("invis", t::<InvisRec>(pc))
        .put("disguise", t::<DisguiseRec>(pc))
        .put("weapon", t::<WeaponRec>(pc))
        .put("wstate", t::<WStateRec>(pc))
        .put("ammo", t::<AmmoRec>(pc))
        .put("medigun", t::<MedigunRec>(pc))
        .put("sniper", t::<SniperRec>(pc))
        .put("fov", t::<FovRec>(pc))
        .put("view", t::<ViewRec>(pc))
        .put("resource", t::<ResourceRec>(pc))
        .put("misc", t::<MiscRec>(pc))
        .put("builder", t::<BuilderRec>(pc))
        .put("loadout", t::<LoadoutRec>(pc))
        .put("scoring", t::<ScoringRec>(pc))
        .put("itemCharge", t::<ItemChargeRec>(pc))
        .put("vm0", t::<VmRec>(pc))
        .put("vm1", t::<VmRec>(pc))
        .put("fire", ev(FireRec::columns(&[])))
        .put("anim", ev(AnimRec::columns(&[])))
        .build()
}
