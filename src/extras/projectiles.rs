//! `projectiles` group (section 2.5.4): one instance per projectile entity lifetime with a
//! per-tick position sample stream and a deflect track.

use super::out::{ObjB, XNode};
use super::track::{empty_instances, empty_track, record, InstanceTable, TrackBuilder};
use super::tracker::{ClassInfo, Ent, Role, Val};
use super::{to_u8, Ex, Orders, Slots};

record!(DeflectRec { team: u8 => "team", owner: u8 => "owner", count: u8 => "count" });

#[derive(Default)]
pub struct ProjSlots {
    pub class_kind: u8,
    pub pipe: bool,
    pub arrow: bool,
    pub pipe_type: Option<u32>,
    pub proj_type: Option<u32>,
    pub team: Option<u32>,
    pub owner_entity: Option<u32>,
    pub thrower: Option<u32>,
    pub orig_launcher: Option<u32>,
    pub launchers: [Option<u32>; 3],
    pub crit: [Option<u32>; 3],
    pub init_vel: Option<u32>,
    pub origin: Option<u32>,
    pub deflected: Option<u32>,
    pub deflect_owner: Option<u32>,
}

const P_ORIGIN: u32 = 1;
const P_CRIT: u32 = 2;
const P_DEFLECT: u32 = 4;

#[inline(never)]
pub fn setup(ci: &mut ClassInfo) -> Vec<String> {
    let name = ci.name.as_str();
    let class_kind = match name {
        "CTFProjectile_Rocket" => 1,
        "CTFGrenadePipebombProjectile" => 2,
        "CTFProjectile_SentryRocket" => 6,
        "CTFProjectile_Flare" => 7,
        "CTFProjectile_Arrow" => 8,
        "CTFProjectile_HealingBolt" => 10,
        "CTFProjectile_Jar" => 11,
        "CTFProjectile_JarMilk" => 12,
        "CTFProjectile_JarGas" => 13,
        "CTFProjectile_Cleaver" => 14,
        "CTFStunBall" => 15,
        "CTFBall_Ornament" => 16,
        "CTFProjectile_EnergyRing" => 17,
        "CTFProjectile_EnergyBall" => 18,
        "CTFProjectile_BallOfFire" => 19,
        "CTFProjectile_MechanicalArmOrb" => 20,
        _ => 0,
    };
    let origin = ci
        .find("DT_TFBaseRocket", "m_vecOrigin")
        .or_else(|| ci.find("DT_TFWeaponBaseGrenadeProj", "m_vecOrigin"))
        .or_else(|| ci.find("DT_BaseEntity", "m_vecOrigin"));
    let s = ProjSlots {
        class_kind,
        pipe: name == "CTFGrenadePipebombProjectile",
        arrow: name == "CTFProjectile_Arrow",
        pipe_type: ci.find("DT_TFProjectile_Pipebomb", "m_iType"),
        proj_type: ci.find("DT_TFProjectile_Arrow", "m_iProjectileType"),
        team: ci.find("DT_BaseEntity", "m_iTeamNum"),
        owner_entity: ci.find("DT_BaseEntity", "m_hOwnerEntity"),
        thrower: ci.find("DT_BaseGrenade", "m_hThrower"),
        orig_launcher: ci.find("DT_BaseProjectile", "m_hOriginalLauncher"),
        launchers: [
            ci.find("DT_TFBaseRocket", "m_hLauncher"),
            ci.find("DT_TFProjectile_Pipebomb", "m_hLauncher"),
            ci.find("DT_TFBaseProjectile", "m_hLauncher"),
        ],
        crit: [
            ci.find("DT_TFProjectile_Rocket", "m_bCritical"),
            ci.find("DT_TFWeaponBaseGrenadeProj", "m_bCritical"),
            ci.find("DT_TFProjectile_Arrow", "m_bCritical"),
        ],
        init_vel: ci
            .find("DT_TFBaseRocket", "m_vInitialVelocity")
            .or_else(|| ci.find("DT_TFWeaponBaseGrenadeProj", "m_vInitialVelocity")),
        origin,
        deflected: ci
            .find("DT_TFBaseRocket", "m_iDeflected")
            .or_else(|| ci.find("DT_TFWeaponBaseGrenadeProj", "m_iDeflected")),
        deflect_owner: ci.find("DT_TFWeaponBaseGrenadeProj", "m_hDeflectOwner"),
    };
    let mut missing = Vec::new();
    if s.origin.is_none() {
        missing.push(
            "origin (DT_TFBaseRocket/DT_TFWeaponBaseGrenadeProj/DT_BaseEntity.m_vecOrigin)"
                .to_string(),
        );
    }
    if s.pipe && s.pipe_type.is_none() {
        missing.push("DT_TFProjectile_Pipebomb.m_iType".to_string());
    }
    if s.arrow && s.proj_type.is_none() {
        missing.push("DT_TFProjectile_Arrow.m_iProjectileType".to_string());
    }
    ci.add_dep(s.origin, P_ORIGIN);
    for c in s.crit {
        ci.add_dep(c, P_CRIT);
    }
    ci.add_dep(s.deflected, P_DEFLECT);
    ci.keep_only(&[
        s.pipe_type,
        s.proj_type,
        s.team,
        s.owner_entity,
        s.thrower,
        s.orig_launcher,
        s.launchers[0],
        s.launchers[1],
        s.launchers[2],
        s.crit[0],
        s.crit[1],
        s.crit[2],
        s.init_vel,
        s.origin,
        s.deflected,
        s.deflect_owner,
    ]);
    ci.slots = Slots::Projectile(Box::new(s));
    missing
}

#[derive(Clone, Debug)]
pub struct PjRow {
    pub kind: u8,
    pub team: u8,
    pub owner: u8,
    pub launcher_def: u16,
    pub crit: u8,
    pub sentry: u16,
    pub hidden: Option<i64>,
    pub vel: [f32; 3],
    pub start: i64,
    pub samples: Vec<[f32; 3]>,
    pub last_t: i64,
    pub last_p: [f32; 3],
    pub has_pos: bool,
    pub deflects: i64,
}

pub struct PjState {
    pub inst: InstanceTable<PjRow>,
    pub deflects: TrackBuilder<DeflectRec>,
}

impl PjState {
    pub fn new(ticks: u32) -> PjState {
        PjState {
            inst: InstanceTable::new(),
            deflects: TrackBuilder::new(ticks),
        }
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

impl PjRow {
    fn emit(&mut self, t: i64, p: [f32; 3], ticks: u32) {
        if t < 0 || t >= ticks as i64 {
            return;
        }
        if let Some(h) = self.hidden {
            if t >= h {
                return;
            }
        }
        let i = t - self.start;
        if i < 0 {
            return;
        }
        let i = i as usize;
        if i < self.samples.len() {
            if let Some(s) = self.samples.get_mut(i) {
                *s = p;
            }
            return;
        }
        let fill = self.samples.last().copied().unwrap_or(p);
        while self.samples.len() < i {
            self.samples.push(fill);
        }
        self.samples.push(p);
    }

    /// Origin update at raw cache tick `t` (section 2.5.4 `pos`).
    fn sample(&mut self, t: i64, p: [f32; 3], ticks: u32) {
        if !self.has_pos {
            self.has_pos = true;
            self.last_t = t;
            self.last_p = p;
            self.emit(t, p, ticks);
            return;
        }
        if t <= self.last_t {
            if t == self.last_t {
                self.emit(t, p, ticks);
            }
            self.last_p = p;
            return;
        }
        let gap = t - self.last_t;
        let from = self.last_t + 1;
        // skip the part before the output window
        let first = from.max(self.start).max(0);
        for tt in first..t {
            let q = if gap <= 8 {
                lerp3(self.last_p, p, (tt - self.last_t) as f32 / gap as f32)
            } else {
                self.last_p
            };
            self.emit(tt, q, ticks);
        }
        self.emit(t, p, ticks);
        self.last_t = t;
        self.last_p = p;
    }

    /// Holds the last position up to `end - 1`.
    fn hold_until(&mut self, end: i64, ticks: u32) {
        if !self.has_pos {
            return;
        }
        let first = (self.last_t + 1).max(self.start).max(0);
        let last = self.last_p;
        for tt in first..end.min(ticks as i64) {
            self.emit(tt, last, ticks);
        }
        if end - 1 > self.last_t {
            self.last_t = end - 1;
        }
    }
}

impl Ex {
    fn pj_owner(&self, ent: &Ent, s: &ProjSlots, deflected: bool) -> u8 {
        if s.thrower.is_some() {
            if deflected {
                if let Some(h) = ent.h(s.deflect_owner) {
                    return self.p_of_handle(Some(h));
                }
            }
            self.p_of_handle(ent.h(s.thrower))
        } else {
            self.p_of_handle(ent.h(s.owner_entity))
        }
    }

    #[inline(never)]
    pub(crate) fn pj_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let Slots::Projectile(s) = &ci.slots else {
            return;
        };
        let kind = if s.pipe {
            match ent.i0(s.pipe_type) {
                1 => 3,
                2 => 4,
                3 => 5,
                _ => 2,
            }
        } else if s.arrow {
            if ent.i0(s.proj_type) == 18 {
                9
            } else {
                8
            }
        } else {
            s.class_kind
        };
        let mut launcher = None;
        for slot in std::iter::once(s.orig_launcher).chain(s.launchers.iter().copied()) {
            if let Some(w) = self.slot_of_handle(ent.h(slot), Role::Weapon) {
                launcher = self.wp.inst.get(w).map(|r| r.data.def);
                if launcher.is_some() {
                    break;
                }
            }
        }
        let crit = s.crit.iter().any(|c| ent.b(*c)) as u8;
        let vel = ent.v(s.init_vel).unwrap_or([f32::NAN; 3]);
        let pos = ent.v(s.origin);
        let sentry = if kind == 6 {
            pos.and_then(|p| self.sentry_near(p)).unwrap_or(65535)
        } else {
            65535
        };
        let row = PjRow {
            kind,
            team: to_u8(ent.i0(s.team)),
            owner: self.pj_owner(ent, s, false),
            launcher_def: launcher.unwrap_or(65535),
            crit,
            sentry,
            hidden: None,
            vel,
            start: st.max(0),
            samples: Vec::new(),
            last_t: st,
            last_p: [0.0; 3],
            has_pos: false,
            deflects: ent.i0(s.deflected),
        };
        let j = self.pj.inst.add(idx, st, row);
        ent.role = Role::Projectile;
        ent.slot = j;
        let ticks = self.ticks;
        if let (Some(p), Some(r)) = (pos, self.pj.inst.get_mut(j)) {
            r.data.sample(st, p, ticks);
        }
        self.count(&format!("proj.kind{kind}"));
    }

    /// The sentry instance whose pivot (origin + 46 z) is within 64 units of `p`.
    fn sentry_near(&self, p: [f32; 3]) -> Option<u16> {
        let mut best: Option<(f32, u32)> = None;
        for (i, e) in self.ents.iter().enumerate() {
            let Some(e) = e else { continue };
            if e.role != Role::Building {
                continue;
            }
            let Some(row) = self.bl.inst.get(e.slot) else {
                continue;
            };
            if row.data.kind != 2 {
                continue;
            }
            let Some(o) = self.xf_abs(i as u32) else {
                continue;
            };
            let d = ((o[0] - p[0]).powi(2) + (o[1] - p[1]).powi(2) + (o[2] + 46.0 - p[2]).powi(2))
                .sqrt();
            if d <= 64.0 && best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, e.slot));
            }
        }
        best.map(|(_, b)| b.min(65534) as u16)
    }

    pub(crate) fn pj_update(
        &mut self,
        _idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        st: i64,
    ) {
        let Slots::Projectile(s) = &ci.slots else {
            return;
        };
        let mut bits = 0u32;
        for (i, _) in changed {
            bits |= ci.dep.get(*i as usize).copied().unwrap_or(0);
        }
        let j = ent.slot;
        let ticks = self.ticks;
        if bits & P_ORIGIN != 0 {
            if let (Some(p), Some(r)) = (ent.v(s.origin), self.pj.inst.get_mut(j)) {
                r.data.sample(st, p, ticks);
            }
        }
        if bits & P_CRIT != 0 && s.crit.iter().any(|c| ent.b(*c)) {
            if let Some(r) = self.pj.inst.get_mut(j) {
                r.data.crit = 1;
            }
        }
        if bits & P_DEFLECT != 0 {
            let count = ent.i0(s.deflected);
            let prev = self.pj.inst.get(j).map(|r| r.data.deflects).unwrap_or(0);
            if count > prev {
                let rec = DeflectRec {
                    team: to_u8(ent.i0(s.team)),
                    owner: self.pj_owner(ent, s, true),
                    count: to_u8(count),
                };
                if self.incl.projectiles {
                    self.pj.deflects.push(j as usize, st, rec);
                }
            }
            if let Some(r) = self.pj.inst.get_mut(j) {
                r.data.deflects = count;
            }
        }
    }

    pub(crate) fn pj_leave(&mut self, ent: &Ent, st: i64) {
        let ticks = self.ticks;
        if let Some(r) = self.pj.inst.get_mut(ent.slot) {
            if r.data.hidden.is_none() {
                r.data.hold_until(st, ticks);
                r.data.hidden = Some(st);
            }
        }
    }

    pub(crate) fn pj_end(&mut self, ent: &Ent, st: i64) {
        let ticks = self.ticks;
        if let Some(r) = self.pj.inst.get_mut(ent.slot) {
            if r.data.hidden.is_none() {
                r.data.hold_until(st, ticks);
                r.data.hidden = Some(st);
            }
            r.deleted = Some(st);
        }
    }

    pub(crate) fn pj_after_packet(&mut self, _st: i64) {}

    /// Holds positions of projectiles still visible at the end of the demo.
    pub(crate) fn pj_finish_positions(&mut self) {
        let ticks = self.ticks;
        for r in self.pj.inst.rows.iter_mut() {
            if r.data.hidden.is_none() {
                r.data.hold_until(ticks as i64, ticks);
            }
        }
    }

    #[inline(never)]
    pub(crate) fn projectiles_out(&mut self, ord: &Orders) -> XNode {
        let o = &ord.projectiles;
        let n = o.count();
        let order = o.order.clone();
        let ticks = self.ticks;
        let bo = &ord.buildings;
        let t = &self.pj.inst;
        let base = t.base_fields(o, ticks);
        let kind = t.column(o, |r| r.data.kind);
        let team = t.column(o, |r| r.data.team);
        let owner = t.column(o, |r| r.data.owner);
        let launcher = t.column(o, |r| r.data.launcher_def);
        let crit = t.column(o, |r| r.data.crit);
        let sentry = t.column(o, |r| bo.map_u16(r.data.sentry));
        let hidden = t.column(o, |r| match r.data.hidden {
            Some(h) if h < ticks as i64 => h.max(0) as u32,
            _ => u32::MAX,
        });
        let vx = t.column(o, |r| r.data.vel[0]);
        let vy = t.column(o, |r| r.data.vel[1]);
        let vz = t.column(o, |r| r.data.vel[2]);
        let total: usize = order
            .iter()
            .filter_map(|i| t.rows.get(*i))
            .map(|r| r.data.samples.len())
            .sum();
        let mut pos: Vec<f32> = Vec::with_capacity(total * 3);
        let mut pos_start: Vec<u32> = Vec::with_capacity(n);
        let mut pos_count: Vec<u32> = Vec::with_capacity(n);
        for i in &order {
            if let Some(r) = self.pj.inst.rows.get_mut(*i) {
                pos_start.push((pos.len() / 3) as u32);
                pos_count.push(r.data.samples.len() as u32);
                for s in r.data.samples.drain(..) {
                    pos.extend_from_slice(&s);
                }
                r.data.samples.shrink_to_fit();
            }
        }
        let deflects = std::mem::replace(&mut self.pj.deflects, TrackBuilder::new(1))
            .into_fields(n, Some(&order));
        ObjB::new()
            .extend(base)
            .put("kind", kind)
            .put("team", team)
            .put("owner", owner)
            .put("launcherDef", launcher)
            .put("crit", crit)
            .put("sentry", sentry)
            .put("hidden", hidden)
            .put("vx", vx)
            .put("vy", vy)
            .put("vz", vz)
            .put("posStart", XNode::U32(pos_start))
            .put("posCount", XNode::U32(pos_count))
            .put("pos", XNode::F32(pos))
            .put("deflects", XNode::Obj(deflects))
            .build()
    }
}

#[inline(never)]
pub fn projectiles_empty() -> XNode {
    ObjB::new()
        .extend(empty_instances())
        .put("kind", XNode::U8(Vec::new()))
        .put("team", XNode::U8(Vec::new()))
        .put("owner", XNode::U8(Vec::new()))
        .put("launcherDef", XNode::U16(Vec::new()))
        .put("crit", XNode::U8(Vec::new()))
        .put("sentry", XNode::U16(Vec::new()))
        .put("hidden", XNode::U32(Vec::new()))
        .put("vx", XNode::F32(Vec::new()))
        .put("vy", XNode::F32(Vec::new()))
        .put("vz", XNode::F32(Vec::new()))
        .put("posStart", XNode::U32(Vec::new()))
        .put("posCount", XNode::U32(Vec::new()))
        .put("pos", XNode::F32(Vec::new()))
        .put("deflects", XNode::Obj(empty_track::<DeflectRec>(0)))
        .build()
}
