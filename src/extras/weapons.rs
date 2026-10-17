//! `weapons` group (section 2.5.2) and wearables (loadout source). Weapons are every class whose
//! flattened props include `DT_BaseCombatWeapon.m_hOwner`.

use super::out::{ObjB, XNode};
use super::players::{
    FireRec, D_ALL, D_BUILDER, D_LOADOUT, D_MEDIGUN, D_MISC, D_SNIPER, D_WEAPON_DEPS, D_WSTATE,
};
use super::track::{empty_instances, InstanceTable};
use super::tracker::{ClassInfo, Ent, Role, Val};
use super::{def_u16, Ex, Orders, Slots};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WKind {
    Medigun,
    Sniper,
    Pipebomb,
    Builder,
    Other,
}

#[derive(Default)]
pub struct WeaponSlots {
    pub def: Option<u32>,
    pub owner: Option<u32>,
    pub disguise: Option<u32>,
    pub last_fire: Option<u32>,
    pub state_flame: Option<u32>,
    pub state_minigun: Option<u32>,
    pub reload: Option<u32>,
    pub clip1: Option<u32>,
    pub backstab: Option<u32>,
    pub crit_fire: Option<u32>,
    pub crit_shot: Option<u32>,
    pub lowered: Option<u32>,
    pub hit_target: Option<u32>,
    pub charge_begin: Option<u32>,
    pub med_charge_nl: Option<u32>,
    pub med_charge_l: Option<u32>,
    pub med_healing: Option<u32>,
    pub med_attacking: Option<u32>,
    pub med_release: Option<u32>,
    pub med_holstered: Option<u32>,
    pub med_target: Option<u32>,
    pub med_resist: Option<u32>,
    pub sniper_charge: Option<u32>,
    pub pipe_count: Option<u32>,
    pub build_state: Option<u32>,
    pub obj_type: Option<u32>,
    pub obj_mode: Option<u32>,
    pub kind: Option<WKind>,
}

/// Dependency bits of weapon props (meaning: which owner records to recompute).
const W_OWNER: u32 = 1 << 24;
const W_DISGUISE: u32 = 1 << 25;
const W_DEF: u32 = 1 << 26;
const W_FIRE: u32 = 1 << 27;

#[inline(never)]
pub fn setup_weapon(ci: &mut ClassInfo) -> Vec<String> {
    let kind = match ci.name.as_str() {
        "CWeaponMedigun" => WKind::Medigun,
        "CTFSniperRifle" | "CTFSniperRifleDecap" | "CTFSniperRifleClassic" => WKind::Sniper,
        "CTFPipebombLauncher" => WKind::Pipebomb,
        "CTFWeaponBuilder" | "CTFWeaponSapper" => WKind::Builder,
        _ => WKind::Other,
    };
    let s = WeaponSlots {
        def: ci.find("DT_ScriptCreatedItem", "m_iItemDefinitionIndex"),
        owner: ci.find("DT_BaseCombatWeapon", "m_hOwner"),
        disguise: ci.find("DT_TFWeaponBase", "m_bDisguiseWeapon"),
        last_fire: ci.find("DT_LocalTFWeaponData", "m_flLastFireTime"),
        state_flame: ci.find("DT_WeaponFlameThrower", "m_iWeaponState"),
        state_minigun: ci.find("DT_WeaponMinigun", "m_iWeaponState"),
        reload: ci.find("DT_TFWeaponBase", "m_iReloadMode"),
        clip1: ci.find("DT_LocalWeaponData", "m_iClip1"),
        backstab: ci.find("DT_TFWeaponKnife", "m_bReadyToBackstab"),
        crit_fire: ci.find("DT_WeaponFlameThrower", "m_bCritFire"),
        crit_shot: ci.find("DT_WeaponMinigun", "m_bCritShot"),
        lowered: ci.find("DT_TFWeaponBase", "m_bLowered"),
        hit_target: ci.find("DT_WeaponFlameThrower", "m_bHitTarget"),
        charge_begin: ci.find("DT_PipebombLauncherLocalData", "m_flChargeBeginTime"),
        med_charge_nl: ci.find("DT_TFWeaponMedigunDataNonLocal", "m_flChargeLevel"),
        med_charge_l: ci.find("DT_LocalTFWeaponMedigunData", "m_flChargeLevel"),
        med_healing: ci.find("DT_WeaponMedigun", "m_bHealing"),
        med_attacking: ci.find("DT_WeaponMedigun", "m_bAttacking"),
        med_release: ci.find("DT_WeaponMedigun", "m_bChargeRelease"),
        med_holstered: ci.find("DT_WeaponMedigun", "m_bHolstered"),
        med_target: ci.find("DT_WeaponMedigun", "m_hHealingTarget"),
        med_resist: ci.find("DT_WeaponMedigun", "m_nChargeResistType"),
        sniper_charge: ci.find("DT_SniperRifleLocalData", "m_flChargedDamage"),
        pipe_count: ci.find("DT_PipebombLauncherLocalData", "m_iPipebombCount"),
        build_state: ci.find("DT_TFWeaponBuilder", "m_iBuildState"),
        obj_type: ci.find("DT_BuilderLocalData", "m_iObjectType"),
        obj_mode: ci.find("DT_TFWeaponBuilder", "m_iObjectMode"),
        kind: Some(kind),
    };
    let mut missing = Vec::new();
    if s.def.is_none() {
        missing.push("DT_ScriptCreatedItem.m_iItemDefinitionIndex".to_string());
    }
    if s.disguise.is_none() {
        missing.push("DT_TFWeaponBase.m_bDisguiseWeapon".to_string());
    }
    ci.add_dep(s.owner, W_OWNER);
    ci.add_dep(s.disguise, W_DISGUISE);
    ci.add_dep(s.def, W_DEF);
    ci.add_dep(s.last_fire, W_FIRE);
    for x in [
        s.state_flame,
        s.state_minigun,
        s.reload,
        s.clip1,
        s.backstab,
        s.crit_fire,
        s.crit_shot,
        s.lowered,
        s.hit_target,
        s.charge_begin,
    ] {
        ci.add_dep(x, D_WSTATE);
    }
    for x in [
        s.med_charge_nl,
        s.med_charge_l,
        s.med_healing,
        s.med_attacking,
        s.med_release,
        s.med_holstered,
        s.med_target,
        s.med_resist,
    ] {
        ci.add_dep(x, D_MEDIGUN);
    }
    ci.add_dep(s.sniper_charge, D_SNIPER);
    ci.add_dep(s.pipe_count, D_MISC);
    for x in [s.build_state, s.obj_type, s.obj_mode] {
        ci.add_dep(x, D_BUILDER);
    }
    ci.keep_only(&[
        s.def,
        s.owner,
        s.disguise,
        s.last_fire,
        s.state_flame,
        s.state_minigun,
        s.reload,
        s.clip1,
        s.backstab,
        s.crit_fire,
        s.crit_shot,
        s.lowered,
        s.hit_target,
        s.charge_begin,
        s.med_charge_nl,
        s.med_charge_l,
        s.med_healing,
        s.med_attacking,
        s.med_release,
        s.med_holstered,
        s.med_target,
        s.med_resist,
        s.sniper_charge,
        s.pipe_count,
        s.build_state,
        s.obj_type,
        s.obj_mode,
    ]);
    ci.slots = Slots::Weapon(Box::new(s));
    missing
}

#[derive(Default, Clone, Copy)]
pub struct WearableSlots {
    pub def: Option<u32>,
    pub owner: Option<u32>,
    /// `DT_TFWearable.m_bDisguiseWearable`: the wearables a disguised spy shows for his disguise class. Like
    /// disguise weapons (rule 2.3.12) they are not part of the spy's loadout.
    pub disguise: Option<u32>,
}

pub fn setup_wearable(ci: &mut ClassInfo) -> Vec<String> {
    let s = WearableSlots {
        def: ci.find("DT_ScriptCreatedItem", "m_iItemDefinitionIndex"),
        owner: ci.find("DT_BaseEntity", "m_hOwnerEntity"),
        disguise: ci.find("DT_TFWearable", "m_bDisguiseWearable"),
    };
    ci.add_dep(s.def, 1);
    ci.add_dep(s.owner, 1);
    ci.add_dep(s.disguise, 1);
    ci.keep_only(&[s.def, s.owner, s.disguise]);
    ci.slots = Slots::Wearable(s);
    let mut missing = Vec::new();
    if s.def.is_none() {
        missing.push("DT_ScriptCreatedItem.m_iItemDefinitionIndex".to_string());
    }
    missing
}

#[derive(Clone, Debug)]
pub struct WeaponRow {
    pub owner: u8,
    pub def: u16,
    pub cls: u16,
    pub disguise: u8,
    pub kind: WKind,
    pub ent: u32,
    pub serial: u32,
}

pub struct WpState {
    pub inst: InstanceTable<WeaponRow>,
    /// wearable entity -> owner player index
    pub wear_owner: Vec<u8>,
}

impl WpState {
    pub fn new(_ticks: u32) -> WpState {
        WpState {
            inst: InstanceTable::new(),
            wear_owner: vec![255; super::tracker::MAX_EDICTS],
        }
    }
}

impl Ex {
    /// Runs `f` with the weapon entity of provisional instance `w` and its slots.
    pub(crate) fn with_weapon<R>(
        &self,
        w: u32,
        f: impl FnOnce(&Ent, &WeaponSlots) -> R,
    ) -> Option<R> {
        let row = self.wp.inst.get(w)?;
        let ent = self.ents.get(row.data.ent as usize)?.as_ref()?;
        if ent.serial != row.data.serial || ent.role != Role::Weapon || ent.slot != w {
            return None;
        }
        let ci = self.class_info(ent.class)?;
        let Slots::Weapon(ws) = &ci.slots else {
            return None;
        };
        Some(f(ent, ws))
    }

    pub(crate) fn wp_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let Slots::Weapon(s) = &ci.slots else { return };
        let owner = self.p_of_handle(ent.h(s.owner));
        let row = WeaponRow {
            owner,
            def: def_u16(ent.i(s.def)),
            cls: ent.class,
            disguise: ent.b(s.disguise) as u8,
            kind: s.kind.unwrap_or(WKind::Other),
            ent: idx,
            serial: ent.serial,
        };
        let w = self.wp.inst.add(idx, st, row);
        ent.role = Role::Weapon;
        ent.slot = w;
        if owner != 255 {
            if let Some(list) = self.pl.owned.get_mut(owner as usize) {
                list.push(w);
            }
            self.pl.mark(owner as usize, D_WEAPON_DEPS);
        }
    }

    pub(crate) fn wp_update(
        &mut self,
        _idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        st: i64,
    ) {
        let Slots::Weapon(s) = &ci.slots else { return };
        let w = ent.slot;
        let mut bits = 0u32;
        for (i, _) in changed {
            bits |= ci.dep.get(*i as usize).copied().unwrap_or(0);
        }
        let Some(row) = self.wp.inst.get(w).map(|r| r.data.clone()) else {
            return;
        };
        let mut owner = row.owner;
        if bits & W_OWNER != 0 {
            let new_owner = self.p_of_handle(ent.h(s.owner));
            if new_owner != owner {
                if let Some(list) = self.pl.owned.get_mut(owner as usize) {
                    list.retain(|x| *x != w);
                }
                self.pl.mark(owner as usize, D_WEAPON_DEPS);
                if let Some(list) = self.pl.owned.get_mut(new_owner as usize) {
                    list.push(w);
                }
                owner = new_owner;
                if let Some(r) = self.wp.inst.get_mut(w) {
                    r.data.owner = new_owner;
                }
                self.pl.mark(owner as usize, D_WEAPON_DEPS);
            }
        }
        if bits & W_DISGUISE != 0 && ent.b(s.disguise) {
            if let Some(r) = self.wp.inst.get_mut(w) {
                r.data.disguise = 1;
            }
            self.pl.mark(owner as usize, D_WEAPON_DEPS);
        }
        if bits & W_DEF != 0 {
            if let Some(r) = self.wp.inst.get_mut(w) {
                r.data.def = def_u16(ent.i(s.def));
            }
            self.pl.mark(owner as usize, D_WEAPON_DEPS);
        }
        if bits & W_FIRE != 0 {
            for (i, old) in changed {
                if Some(*i) == s.last_fire {
                    let new = ent.f0(s.last_fire);
                    let disguise = self.wp.inst.get(w).map(|r| r.data.disguise).unwrap_or(0);
                    if let Some(o) = old.float() {
                        if new > o && disguise == 0 && owner != 255 {
                            let def = self.wp.inst.get(w).map(|r| r.data.def).unwrap_or(65535);
                            self.pl.fire.push(
                                st,
                                FireRec {
                                    player: owner,
                                    weapon_def: def,
                                },
                            );
                        }
                    }
                }
            }
        }
        self.pl.mark(owner as usize, bits & D_ALL);
    }

    pub(crate) fn wp_end(&mut self, ent: &Ent, st: i64) {
        let w = ent.slot;
        let owner = self.wp.inst.get(w).map(|r| r.data.owner).unwrap_or(255);
        if let Some(r) = self.wp.inst.get_mut(w) {
            r.deleted = Some(st);
        }
        if let Some(list) = self.pl.owned.get_mut(owner as usize) {
            list.retain(|x| *x != w);
        }
        self.pl.mark(owner as usize, D_WEAPON_DEPS);
    }

    pub(crate) fn wearable_def(&self, ent: &Ent) -> u16 {
        let Some(ci) = self.class_info(ent.class) else {
            return 65535;
        };
        let Slots::Wearable(s) = &ci.slots else {
            return 65535;
        };
        def_u16(ent.i(s.def))
    }

    /// True for a disguise wearable (`m_bDisguiseWearable`), which the loadout skips.
    pub(crate) fn wearable_is_disguise(&self, ent: &Ent) -> bool {
        let Some(ci) = self.class_info(ent.class) else {
            return false;
        };
        let Slots::Wearable(s) = &ci.slots else {
            return false;
        };
        ent.i0(s.disguise) != 0
    }

    pub(crate) fn wr_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, _st: i64) {
        let Slots::Wearable(s) = &ci.slots else {
            return;
        };
        ent.role = Role::Wearable;
        let owner = self.p_of_handle(ent.h(s.owner));
        if let Some(o) = self.wp.wear_owner.get_mut(idx as usize) {
            *o = owner;
        }
        self.pl.mark(owner as usize, D_LOADOUT);
    }

    pub(crate) fn wr_update(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        _changed: &[(u32, Val)],
        _st: i64,
    ) {
        let Slots::Wearable(s) = &ci.slots else {
            return;
        };
        let owner = self.p_of_handle(ent.h(s.owner));
        let old = self.wp.wear_owner.get(idx as usize).copied().unwrap_or(255);
        if let Some(o) = self.wp.wear_owner.get_mut(idx as usize) {
            *o = owner;
        }
        self.pl.mark(old as usize, D_LOADOUT);
        self.pl.mark(owner as usize, D_LOADOUT);
    }

    pub(crate) fn wr_end(&mut self, idx: u32) {
        let owner = self.wp.wear_owner.get(idx as usize).copied().unwrap_or(255);
        if let Some(o) = self.wp.wear_owner.get_mut(idx as usize) {
            *o = 255;
        }
        self.pl.mark(owner as usize, D_LOADOUT);
    }

    #[inline(never)]
    pub(crate) fn weapons_out(&mut self, ord: &Orders) -> XNode {
        let t = &self.wp.inst;
        let o = &ord.weapons;
        ObjB::new()
            .extend(t.base_fields(o, self.ticks))
            .put("owner", t.column(o, |r| r.data.owner))
            .put("def", t.column(o, |r| r.data.def))
            .put("cls", t.column(o, |r| r.data.cls))
            .put("disguise", t.column(o, |r| r.data.disguise))
            .build()
    }
}

pub fn weapons_empty() -> XNode {
    ObjB::new()
        .extend(empty_instances())
        .put("owner", XNode::U8(Vec::new()))
        .put("def", XNode::U16(Vec::new()))
        .put("cls", XNode::U16(Vec::new()))
        .put("disguise", XNode::U8(Vec::new()))
        .build()
}
