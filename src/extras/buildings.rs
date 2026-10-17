//! `buildings` group (section 2.5.3) and the derived building events (section 2.5.8).

use super::movers::XfTarget;
use super::out::{HlEvent, ObjB, XNode};
use super::track::{empty_instances, empty_track, record, InstanceTable, TrackBuilder};
use super::tracker::{ArrVal, ClassInfo, Ent, Role, Val};
use super::{clamp_u16, to_u8, Ex, Orders, Slots};

record!(BldStateRec {
    level: u8 => "level", health: u16 => "health", max_health: u16 => "maxHealth", built: u8 => "built",
    flags: u16 => "flags", obj_state: u8 => "objState", upgrade_metal: u16 => "upgradeMetal",
    shells: u16 => "shells", rockets: u8 => "rockets", metal: u16 => "metal", recharge_tick: f32 => "rechargeTick",
    recharge_dur: f32 => "rechargeDur", times_used: u16 => "timesUsed"
});
record!(PoseRec { yaw: u8 => "yaw", pitch: u8 => "pitch" });
record!(XformRec { x: f32 => "x", y: f32 => "y", z: f32 => "z", qx: f32 => "qx", qy: f32 => "qy", qz: f32 => "qz", qw: f32 => "qw" });
record!(EnemyRec { kind: u8 => "kind", index: u16 => "index" });
record!(HealRec { lo: u32 => "lo", hi: u32 => "hi" });

pub const BLD_BUILDING: u16 = 1 << 0;
pub const BLD_SAPPED: u16 = 1 << 1;
pub const BLD_DISABLED: u16 = 1 << 2;
pub const BLD_CARRY_DEPLOY: u16 = 1 << 3;
pub const BLD_WRANGLED: u16 = 1 << 4;
pub const BLD_SHIELD: u16 = 1 << 5;
pub const BLD_PLASMA: u16 = 1 << 6;
pub const BLD_DORMANT: u16 = 1 << 7;
pub const BLD_UPGRADING: u16 = 1 << 8;

#[derive(Default)]
pub struct BuildingSlots {
    pub class_type: u8,
    pub cart: bool,
    pub obj_type: Option<u32>,
    pub obj_mode: Option<u32>,
    pub builder: Option<u32>,
    pub team: Option<u32>,
    pub mini: Option<u32>,
    pub map_placed: Option<u32>,
    pub built_on: Option<u32>,
    pub level: Option<u32>,
    pub health: Option<u32>,
    pub max_health: Option<u32>,
    pub pct: Option<u32>,
    pub building: Option<u32>,
    pub sapper: Option<u32>,
    pub disabled: Option<u32>,
    pub carry_deploy: Option<u32>,
    pub plasma: Option<u32>,
    pub sg_state: Option<u32>,
    pub disp_state: Option<u32>,
    pub tele_state: Option<u32>,
    pub upgrade_metal: Option<u32>,
    pub shells: Option<u32>,
    pub rockets: Option<u32>,
    pub metal: Option<u32>,
    pub recharge: Option<u32>,
    pub recharge_dur: Option<u32>,
    pub times_used: Option<u32>,
    pub pose0: Option<u32>,
    pub pose1: Option<u32>,
    pub controlled: Option<u32>,
    pub shield: Option<u32>,
    pub enemy: Option<u32>,
    pub heal: Option<u32>,
    pub origin: Option<u32>,
    pub angles: Option<u32>,
    pub parent: Option<u32>,
}

const B_STATE: u32 = 1;
const B_POSE: u32 = 2;
const B_ENEMY: u32 = 4;
const B_HEAL: u32 = 8;
const B_XF: u32 = 16;
const B_BUILT_ON: u32 = 32;

#[inline(never)]
pub fn setup(ci: &mut ClassInfo) -> Vec<String> {
    let (class_type, cart) = match ci.name.as_str() {
        "CObjectDispenser" => (0, false),
        "CObjectTeleporter" => (1, false),
        "CObjectSentrygun" => (2, false),
        "CObjectSapper" => (3, false),
        _ => (0, true),
    };
    let o = "DT_BaseObject";
    let s = BuildingSlots {
        class_type,
        cart,
        obj_type: ci.find(o, "m_iObjectType"),
        obj_mode: ci.find(o, "m_iObjectMode"),
        builder: ci.find(o, "m_hBuilder"),
        team: ci.find("DT_BaseEntity", "m_iTeamNum"),
        mini: ci.find(o, "m_bMiniBuilding"),
        map_placed: ci.find(o, "m_bWasMapPlaced"),
        built_on: ci.find(o, "m_hBuiltOnEntity"),
        level: ci.find(o, "m_iUpgradeLevel"),
        health: ci.find(o, "m_iHealth"),
        max_health: ci.find(o, "m_iMaxHealth"),
        pct: ci.find(o, "m_flPercentageConstructed"),
        building: ci.find(o, "m_bBuilding"),
        sapper: ci.find(o, "m_bHasSapper"),
        disabled: ci.find(o, "m_bDisabled"),
        carry_deploy: ci.find(o, "m_bCarryDeploy"),
        plasma: ci.find(o, "m_bPlasmaDisable"),
        sg_state: ci.find("DT_ObjectSentrygun", "m_iState"),
        disp_state: ci.find("DT_ObjectDispenser", "m_iState"),
        tele_state: ci.find("DT_ObjectTeleporter", "m_iState"),
        upgrade_metal: ci.find(o, "m_iUpgradeMetal"),
        shells: ci.find("DT_ObjectSentrygun", "m_iAmmoShells"),
        rockets: ci.find("DT_ObjectSentrygun", "m_iAmmoRockets"),
        metal: ci.find("DT_ObjectDispenser", "m_iAmmoMetal"),
        recharge: ci.find("DT_ObjectTeleporter", "m_flRechargeTime"),
        recharge_dur: ci.find("DT_ObjectTeleporter", "m_flCurrentRechargeDuration"),
        times_used: ci.find("DT_ObjectTeleporter", "m_iTimesUsed"),
        pose0: ci.elem("m_flPoseParameter", 0),
        pose1: ci.elem("m_flPoseParameter", 1),
        controlled: ci.find("DT_ObjectSentrygun", "m_bPlayerControlled"),
        shield: ci.find("DT_ObjectSentrygun", "m_nShieldLevel"),
        enemy: ci.find("DT_ObjectSentrygun", "m_hEnemy"),
        heal: ci
            .find("DT_ObjectDispenser", "\"healing_array\"")
            .or_else(|| ci.find("DT_ObjectDispenser", "healing_array")),
        origin: ci.find("DT_BaseEntity", "m_vecOrigin"),
        angles: ci.find("DT_BaseEntity", "m_angRotation"),
        parent: ci.find("DT_BaseEntity", "moveparent"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("DT_BaseObject.m_iHealth", s.health),
        ("DT_BaseObject.m_hBuilder", s.builder),
        ("DT_BaseEntity.m_vecOrigin", s.origin),
        ("DT_BaseEntity.m_angRotation", s.angles),
        ("DT_BaseEntity.moveparent", s.parent),
    ] {
        if x.is_none() {
            missing.push(n.to_string());
        }
    }
    if class_type == 2 {
        for (n, x) in [
            ("m_flPoseParameter.000", s.pose0),
            ("m_flPoseParameter.001", s.pose1),
            ("DT_ObjectSentrygun.m_hEnemy", s.enemy),
        ] {
            if x.is_none() {
                missing.push(n.to_string());
            }
        }
    }
    if class_type == 0 && s.heal.is_none() {
        missing.push("DT_ObjectDispenser.healing_array".to_string());
    }
    for x in [
        s.level,
        s.health,
        s.max_health,
        s.pct,
        s.building,
        s.sapper,
        s.disabled,
        s.carry_deploy,
        s.plasma,
        s.sg_state,
        s.disp_state,
        s.tele_state,
        s.upgrade_metal,
        s.shells,
        s.rockets,
        s.metal,
        s.recharge,
        s.recharge_dur,
        s.times_used,
        s.controlled,
        s.shield,
    ] {
        ci.add_dep(x, B_STATE);
    }
    ci.add_dep(s.pose0, B_POSE);
    ci.add_dep(s.pose1, B_POSE);
    ci.add_dep(s.enemy, B_ENEMY);
    ci.add_dep(s.heal, B_HEAL);
    ci.add_dep(s.origin, B_XF);
    ci.add_dep(s.angles, B_XF);
    ci.add_dep(s.parent, B_XF);
    ci.add_dep(s.built_on, B_BUILT_ON);
    if let Some(h) = s.heal {
        ci.keep_arrays.push(h);
    }
    ci.slots = Slots::Building(Box::new(s));
    missing
}

#[derive(Clone, Debug)]
pub struct BldRow {
    pub kind: u8,
    pub mode: u8,
    pub builder: u8,
    pub team: u8,
    pub mini: u8,
    pub map_placed: u8,
    pub built_on: i32,
    pub last_flags: u16,
    pub last_level: u8,
    pub has_state: bool,
}

pub struct BlState {
    pub inst: InstanceTable<BldRow>,
    pub state: TrackBuilder<BldStateRec>,
    pub pose: TrackBuilder<PoseRec>,
    pub xform: TrackBuilder<XformRec>,
    pub enemy: TrackBuilder<EnemyRec>,
    pub heal: TrackBuilder<HealRec>,
    dirty: Vec<(u32, u32)>,
}

impl BlState {
    pub fn new(ticks: u32) -> BlState {
        BlState {
            inst: InstanceTable::new(),
            state: TrackBuilder::new(ticks),
            pose: TrackBuilder::new(ticks),
            xform: TrackBuilder::new(ticks),
            enemy: TrackBuilder::new(ticks),
            heal: TrackBuilder::new(ticks),
            dirty: Vec::new(),
        }
    }
}

impl Ex {
    fn bl_mark(&mut self, idx: u32, bits: u32) {
        if bits == 0 {
            return;
        }
        match self.bl.dirty.iter_mut().find(|(i, _)| *i == idx) {
            Some(d) => d.1 |= bits,
            None => self.bl.dirty.push((idx, bits)),
        }
    }

    pub(crate) fn bl_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let Slots::Building(s) = &ci.slots else {
            return;
        };
        let kind = ent.i(s.obj_type).map(to_u8).unwrap_or(s.class_type);
        let kind = if s.cart { 0 } else { kind };
        let map_placed = if s.cart { 1 } else { ent.b(s.map_placed) as u8 };
        let built_on = self
            .slot_of_handle(ent.h(s.built_on), Role::Building)
            .map(|b| b as i32)
            .unwrap_or(-1);
        let row = BldRow {
            kind,
            mode: to_u8(ent.i0(s.obj_mode)),
            builder: if map_placed == 1 {
                255
            } else {
                self.p_of_handle(ent.h(s.builder))
            },
            team: to_u8(ent.i0(s.team)),
            mini: ent.b(s.mini) as u8,
            map_placed,
            built_on,
            last_flags: 0,
            last_level: 0,
            has_state: false,
        };
        let b = self.bl.inst.add(idx, st, row.clone());
        ent.role = Role::Building;
        ent.slot = b;
        if map_placed == 0 && self.incl.buildings {
            self.ev_push(
                st,
                HlEvent::BuildStart {
                    tick: 0,
                    building: b as i32,
                    builder: row.builder as u32,
                    kind: row.kind as u32,
                    mode: row.mode as u32,
                },
            );
        }
        self.xf_register(
            idx,
            ent,
            s.origin,
            s.angles,
            None,
            s.parent,
            XfTarget::Building(b),
            st,
        );
        self.bl_mark(idx, B_STATE | B_POSE | B_ENEMY | B_HEAL);
    }

    pub(crate) fn bl_update(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        st: i64,
        revived: bool,
    ) {
        let Slots::Building(s) = &ci.slots else {
            return;
        };
        let mut bits = 0u32;
        for (i, _) in changed {
            bits |= ci.dep.get(*i as usize).copied().unwrap_or(0);
        }
        let b = ent.slot;
        if bits & B_BUILT_ON != 0 {
            let on = self.slot_of_handle(ent.h(s.built_on), Role::Building);
            if let (Some(on), Some(r)) = (on, self.bl.inst.get_mut(b)) {
                if r.data.built_on < 0 {
                    r.data.built_on = on as i32;
                }
            }
        }
        if bits & B_XF != 0 {
            self.xf_local(idx, ent, s.origin, s.angles, None, s.parent, st);
        }
        if revived {
            let builder = self.bl.inst.get(b).map(|r| r.data.builder).unwrap_or(255);
            if self.incl.buildings {
                self.ev_push(
                    st,
                    HlEvent::Redeploy {
                        tick: 0,
                        building: b as i32,
                        builder: builder as u32,
                    },
                );
            }
            bits |= B_STATE;
        }
        self.bl_mark(idx, bits & (B_STATE | B_POSE | B_ENEMY | B_HEAL));
    }

    pub(crate) fn bl_leave(&mut self, idx: u32, ent: &mut Ent, st: i64) {
        let b = ent.slot;
        let builder = self.bl.inst.get(b).map(|r| r.data.builder).unwrap_or(255);
        if self.incl.buildings {
            self.ev_push(
                st,
                HlEvent::Carry {
                    tick: 0,
                    building: b as i32,
                    builder: builder as u32,
                },
            );
        }
        self.bl_mark(idx, B_STATE);
    }

    pub(crate) fn bl_end(&mut self, idx: u32, ent: &Ent, st: i64) {
        if let Some(r) = self.bl.inst.get_mut(ent.slot) {
            r.deleted = Some(st);
        }
        self.bl.dirty.retain(|(i, _)| *i != idx);
        self.xf_unregister(idx);
    }

    #[inline(never)]
    pub(crate) fn bl_flush(&mut self, st: i64) {
        if self.bl.dirty.is_empty() {
            return;
        }
        let dirty = std::mem::take(&mut self.bl.dirty);
        for (idx, bits) in dirty {
            let Some(ent) = self.ents.get(idx as usize).and_then(|e| e.as_ref()) else {
                continue;
            };
            if ent.role != Role::Building {
                continue;
            }
            let Some(ci) = self.class_info(ent.class) else {
                continue;
            };
            let Slots::Building(s) = &ci.slots else {
                continue;
            };
            let b = ent.slot;
            let Some(mut row) = self.bl.inst.get(b).map(|r| r.data.clone()) else {
                continue;
            };
            if row.built_on < 0 {
                if let Some(on) = self.slot_of_handle(ent.h(s.built_on), Role::Building) {
                    row.built_on = on as i32;
                    if let Some(r) = self.bl.inst.get_mut(b) {
                        r.data.built_on = on as i32;
                    }
                }
            }
            let mut state_rec = None;
            if bits & B_STATE != 0 {
                let obj_state = ent
                    .i(s.sg_state)
                    .or_else(|| ent.i(s.disp_state))
                    .or_else(|| ent.i(s.tele_state))
                    .unwrap_or(0);
                let upgrading = match row.kind {
                    2 => ent.i0(s.sg_state) == 3,
                    0 => ent.i0(s.disp_state) == 1,
                    1 => ent.i0(s.tele_state) == 7,
                    _ => false,
                };
                let mut flags = 0u16;
                if ent.b(s.building) {
                    flags |= BLD_BUILDING;
                }
                if ent.b(s.sapper) {
                    flags |= BLD_SAPPED;
                }
                if ent.b(s.disabled) {
                    flags |= BLD_DISABLED;
                }
                if ent.b(s.carry_deploy) {
                    flags |= BLD_CARRY_DEPLOY;
                }
                if ent.b(s.controlled) {
                    flags |= BLD_WRANGLED;
                }
                if ent.i0(s.shield) > 0 {
                    flags |= BLD_SHIELD;
                }
                if ent.b(s.plasma) {
                    flags |= BLD_PLASMA;
                }
                if ent.dormant {
                    flags |= BLD_DORMANT;
                }
                if upgrading {
                    flags |= BLD_UPGRADING;
                }
                let rec = BldStateRec {
                    level: to_u8(ent.i0(s.level)),
                    health: clamp_u16(ent.i0(s.health)),
                    max_health: clamp_u16(ent.i0(s.max_health)),
                    built: ((ent.f0(s.pct) as f64).clamp(0.0, 1.0) * 255.0).round() as u8,
                    flags,
                    obj_state: to_u8(obj_state),
                    upgrade_metal: clamp_u16(ent.i0(s.upgrade_metal)),
                    shells: clamp_u16(ent.i0(s.shells)),
                    rockets: to_u8(ent.i0(s.rockets)),
                    metal: clamp_u16(ent.i0(s.metal)),
                    recharge_tick: self.tick_f_nan(ent.f0(s.recharge)),
                    recharge_dur: ent.f0(s.recharge_dur),
                    times_used: clamp_u16(ent.i0(s.times_used)),
                };
                state_rec = Some(rec);
            }
            let pose_rec = if bits & B_POSE != 0 && row.kind == 2 {
                let q = |v: f32| ((v as f64).clamp(0.0, 1.0) * 255.0).round() as u8;
                Some(PoseRec {
                    yaw: q(ent.f0(s.pose1)),
                    pitch: q(ent.f0(s.pose0)),
                })
            } else {
                None
            };
            let enemy_rec = if bits & B_ENEMY != 0 && s.enemy.is_some() {
                let h = ent.h(s.enemy);
                let rec = match h {
                    None => EnemyRec {
                        kind: 0,
                        index: 65535,
                    },
                    Some(hh) => {
                        let p = self.p_of_ent(hh.0);
                        if p != 255 {
                            EnemyRec {
                                kind: 1,
                                index: p as u16,
                            }
                        } else if let Some(bb) = self.slot_of_handle(h, Role::Building) {
                            EnemyRec {
                                kind: 2,
                                index: bb.min(65534) as u16,
                            }
                        } else {
                            EnemyRec {
                                kind: 3,
                                index: 65535,
                            }
                        }
                    }
                };
                Some(rec)
            } else {
                None
            };
            let heal_rec = if bits & B_HEAL != 0 && s.heal.is_some() {
                let mut lo = 0u32;
                let mut hi = 0u32;
                if let Some(ArrVal::Ints(list)) = ent.array(s.heal) {
                    for h in list {
                        let p = self.p_of_handle(super::tracker::handle_parts(*h));
                        if p < 32 {
                            lo |= 1 << p;
                        } else if p < 64 {
                            hi |= 1 << (p - 32);
                        }
                    }
                }
                Some(HealRec { lo, hi })
            } else {
                None
            };
            // derived events
            let mut events = Vec::new();
            if let Some(rec) = state_rec {
                if row.has_state {
                    let was_building = row.last_flags & BLD_BUILDING != 0;
                    let now_building = rec.flags & BLD_BUILDING != 0;
                    if was_building && !now_building && rec.flags & BLD_CARRY_DEPLOY == 0 {
                        events.push(HlEvent::BuildDone {
                            tick: 0,
                            building: b as i32,
                            builder: row.builder as u32,
                            kind: row.kind as u32,
                        });
                    }
                    if rec.level > row.last_level {
                        events.push(HlEvent::Upgrade {
                            tick: 0,
                            building: b as i32,
                            builder: row.builder as u32,
                            level: rec.level as u32,
                        });
                    }
                }
            }
            if let (Some(rec), Some(r)) = (state_rec, self.bl.inst.get_mut(b)) {
                r.data.last_flags = rec.flags;
                r.data.last_level = rec.level;
                r.data.has_state = true;
            }
            if self.incl.buildings {
                for e in events {
                    self.ev_push(st, e);
                }
                if let Some(r) = state_rec {
                    self.bl.state.push(b as usize, st, r);
                }
                if let Some(r) = pose_rec {
                    self.bl.pose.push(b as usize, st, r);
                }
                if let Some(r) = enemy_rec {
                    self.bl.enemy.push(b as usize, st, r);
                }
                if let Some(r) = heal_rec {
                    self.bl.heal.push(b as usize, st, r);
                }
            }
        }
    }

    #[inline(never)]
    pub(crate) fn buildings_out(&mut self, ord: &Orders) -> XNode {
        let o = &ord.buildings;
        let order: Vec<usize> = o.order.clone();
        let n = o.count();
        self.bl.enemy.map_records(|r| {
            if r.kind == 2 {
                r.index = o.map_u16(r.index);
            }
        });
        let t = &self.bl.inst;
        let base = t.base_fields(o, self.ticks);
        let kind = t.column(o, |r| r.data.kind);
        let mode = t.column(o, |r| r.data.mode);
        let builder = t.column(o, |r| r.data.builder);
        let team = t.column(o, |r| r.data.team);
        let mini = t.column(o, |r| r.data.mini);
        let map_placed = t.column(o, |r| r.data.map_placed);
        let built_on = t.column(o, |r| o.map_i32(r.data.built_on));
        let bl = &mut self.bl;
        let state =
            std::mem::replace(&mut bl.state, TrackBuilder::new(1)).into_fields(n, Some(&order));
        let pose =
            std::mem::replace(&mut bl.pose, TrackBuilder::new(1)).into_fields(n, Some(&order));
        let xform =
            std::mem::replace(&mut bl.xform, TrackBuilder::new(1)).into_fields(n, Some(&order));
        let enemy =
            std::mem::replace(&mut bl.enemy, TrackBuilder::new(1)).into_fields(n, Some(&order));
        let heal =
            std::mem::replace(&mut bl.heal, TrackBuilder::new(1)).into_fields(n, Some(&order));
        ObjB::new()
            .extend(base)
            .put("type", kind)
            .put("mode", mode)
            .put("builder", builder)
            .put("team", team)
            .put("mini", mini)
            .put("mapPlaced", map_placed)
            .put("builtOn", built_on)
            .put("state", XNode::Obj(state))
            .put("pose", XNode::Obj(pose))
            .put("xform", XNode::Obj(xform))
            .put("enemy", XNode::Obj(enemy))
            .put("heal", XNode::Obj(heal))
            .build()
    }
}

#[inline(never)]
pub fn buildings_empty() -> XNode {
    ObjB::new()
        .extend(empty_instances())
        .put("type", XNode::U8(Vec::new()))
        .put("mode", XNode::U8(Vec::new()))
        .put("builder", XNode::U8(Vec::new()))
        .put("team", XNode::U8(Vec::new()))
        .put("mini", XNode::U8(Vec::new()))
        .put("mapPlaced", XNode::U8(Vec::new()))
        .put("builtOn", XNode::I32(Vec::new()))
        .put("state", XNode::Obj(empty_track::<BldStateRec>(0)))
        .put("pose", XNode::Obj(empty_track::<PoseRec>(0)))
        .put("xform", XNode::Obj(empty_track::<XformRec>(0)))
        .put("enemy", XNode::Obj(empty_track::<EnemyRec>(0)))
        .put("heal", XNode::Obj(empty_track::<HealRec>(0)))
        .build()
}
