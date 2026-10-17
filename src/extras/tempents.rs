//! `tempents` group, `players.anim` and `hurts` (section 2.5.8). Temp entities are delta-merged
//! (rule 2.3.7): an event of the same class as the previous event in the message is a delta
//! against it, any other event is a delta against zero.

use std::rc::Rc;

use crate::demo::message::tempentities::TempEntitiesMessage;
use crate::demo::sendprop::{SendPropIdentifier, SendPropValue};
use crate::ParserState;

use super::out::{ObjB, XNode};
use super::players::AnimRec;
use super::track::{record, EventBuilder, Record};
use super::tracker::{handle_parts, Role};
use super::{to_u8, Ex, Orders};

record!(BulletRec {
    player: u8 => "player", weapon_id: u8 => "weaponId", crit: u8 => "crit", seed: u8 => "seed",
    spread: f32 => "spread", x: f32 => "x", y: f32 => "y", z: f32 => "z", pitch: f32 => "pitch", yaw: f32 => "yaw"
});
record!(ExplosionRec {
    x: f32 => "x", y: f32 => "y", z: f32 => "z", nx: f32 => "nx", ny: f32 => "ny", nz: f32 => "nz",
    weapon_id: u8 => "weaponId", def: u16 => "def", entity: u16 => "entity", particle: u16 => "particle"
});
record!(TracerRec {
    sentry: u16 => "sentry", sx: f32 => "sx", sy: f32 => "sy", sz: f32 => "sz", ex: f32 => "ex", ey: f32 => "ey", ez: f32 => "ez"
});
record!(ParticleRec { name: u16 => "name", x: f32 => "x", y: f32 => "y", z: f32 => "z", entity: u16 => "entity" });
record!(ClientProjRec {
    kind: u8 => "kind", player: u8 => "player", crit: u8 => "crit", x: f32 => "x", y: f32 => "y", z: f32 => "z",
    vx: f32 => "vx", vy: f32 => "vy", vz: f32 => "vz"
});
record!(HurtRec {
    victim: u8 => "victim", attacker: u8 => "attacker", damage: u16 => "damage", health: i16 => "health",
    crit: u8 => "crit", weapon_id: u8 => "weaponId", custom: u16 => "custom"
});

#[derive(Clone, Copy, PartialEq, Eq)]
enum TeKind {
    FireBullets,
    Explosion,
    EffectDispatch,
    ParticleEffect,
    AnimEvent,
    Other,
}

/// Merged temp entity value by flattened index (absent = zero).
#[derive(Clone, Copy, Default)]
enum Tv {
    #[default]
    Zero,
    I(i64),
    F(f32),
    V([f32; 3]),
}

impl Tv {
    fn i(&self) -> i64 {
        match self {
            Tv::I(i) => *i,
            Tv::F(f) => *f as i64,
            _ => 0,
        }
    }
    fn f(&self) -> f32 {
        match self {
            Tv::F(f) => *f,
            Tv::I(i) => *i as f32,
            _ => 0.0,
        }
    }
    fn v(&self) -> [f32; 3] {
        match self {
            Tv::V(v) => *v,
            _ => [0.0; 3],
        }
    }
}

struct TeClass {
    kind: TeKind,
    n: usize,
    count_key: String,
    /// named slots, resolved per kind
    slots: Vec<Option<u32>>,
}

pub struct TeState {
    classes: Vec<Option<Rc<TeClass>>>,
    pub bullets: EventBuilder<BulletRec>,
    pub explosions: EventBuilder<ExplosionRec>,
    pub tracers: EventBuilder<TracerRec>,
    pub particles: EventBuilder<ParticleRec>,
    pub client: EventBuilder<ClientProjRec>,
    pub hurts: EventBuilder<HurtRec>,
}

impl TeState {
    pub fn new(ticks: u32) -> TeState {
        TeState {
            classes: Vec::new(),
            bullets: EventBuilder::new(ticks),
            explosions: EventBuilder::new(ticks),
            tracers: EventBuilder::new(ticks),
            particles: EventBuilder::new(ticks),
            client: EventBuilder::new(ticks),
            hurts: EventBuilder::new(ticks),
        }
    }
    pub fn reset_classes(&mut self, n: usize) {
        self.classes = (0..n).map(|_| None).collect();
    }
}

// slot indices per kind
const FB: [(&str, &str); 9] = [
    ("DT_TEFireBullets", "m_iPlayer"),
    ("DT_TEFireBullets", "m_iWeaponID"),
    ("DT_TEFireBullets", "m_bCritical"),
    ("DT_TEFireBullets", "m_iSeed"),
    ("DT_TEFireBullets", "m_flSpread"),
    ("DT_TEFireBullets", "m_vecOrigin"),
    ("DT_TEFireBullets", "m_vecAngles[0]"),
    ("DT_TEFireBullets", "m_vecAngles[1]"),
    ("DT_TEFireBullets", "m_iMode"),
];
const EX: [(&str, &str); 9] = [
    ("DT_TETFExplosion", "m_vecOrigin[0]"),
    ("DT_TETFExplosion", "m_vecOrigin[1]"),
    ("DT_TETFExplosion", "m_vecOrigin[2]"),
    ("DT_TETFExplosion", "m_vecNormal"),
    ("DT_TETFExplosion", "m_iWeaponID"),
    ("DT_TETFExplosion", "m_nDefID"),
    ("DT_TETFExplosion", "entindex"),
    ("DT_TETFExplosion", "m_iCustomParticleIndex"),
    ("DT_TETFExplosion", "m_nSound"),
];
const ED: [(&str, &str); 11] = [
    ("DT_EffectData", "m_iEffectName"),
    ("DT_EffectData", "m_vOrigin[0]"),
    ("DT_EffectData", "m_vOrigin[1]"),
    ("DT_EffectData", "m_vOrigin[2]"),
    ("DT_EffectData", "m_vStart[0]"),
    ("DT_EffectData", "m_vStart[1]"),
    ("DT_EffectData", "m_vStart[2]"),
    ("DT_EffectData", "entindex"),
    ("DT_EffectData", "m_nHitBox"),
    ("DT_EffectData", "m_nDamageType"),
    ("DT_EffectData", "m_fFlags"),
];
const PE: [(&str, &str); 5] = [
    ("DT_TETFParticleEffect", "m_iParticleSystemIndex"),
    ("DT_TETFParticleEffect", "m_vecOrigin[0]"),
    ("DT_TETFParticleEffect", "m_vecOrigin[1]"),
    ("DT_TETFParticleEffect", "m_vecOrigin[2]"),
    ("DT_TETFParticleEffect", "entindex"),
];
const AE: [(&str, &str); 4] = [
    ("DT_TEPlayerAnimEvent", "m_hPlayer"),
    ("DT_TEPlayerAnimEvent", "m_iEvent"),
    ("DT_TEPlayerAnimEvent", "m_nData"),
    // older demos send the entity index instead of the handle
    ("DT_TEPlayerAnimEvent", "m_iPlayerIndex"),
];

fn resolve(ids: &[SendPropIdentifier], names: &[(&str, &str)]) -> Vec<Option<u32>> {
    names
        .iter()
        .map(|(t, p)| {
            let id = SendPropIdentifier::new(t, p);
            ids.iter().position(|x| *x == id).map(|i| i as u32)
        })
        .collect()
}

impl Ex {
    fn te_class(&mut self, class: u16, ps: &ParserState) -> Option<Rc<TeClass>> {
        let i = class as usize;
        if self.te.classes.len() <= i {
            self.te.classes.resize_with(i + 1, || None);
        }
        if let Some(Some(c)) = self.te.classes.get(i) {
            return Some(c.clone());
        }
        let name = self.class_name(class).to_string();
        let ids: Vec<SendPropIdentifier> = ps
            .send_tables
            .get(i)
            .map(|t| t.flattened_props.iter().map(|p| p.identifier).collect())
            .unwrap_or_default();
        let (kind, names): (TeKind, &[(&str, &str)]) = match name.as_str() {
            "CTEFireBullets" => (TeKind::FireBullets, &FB),
            "CTETFExplosion" => (TeKind::Explosion, &EX),
            "CTEEffectDispatch" => (TeKind::EffectDispatch, &ED),
            "CTETFParticleEffect" => (TeKind::ParticleEffect, &PE),
            "CTEPlayerAnimEvent" => (TeKind::AnimEvent, &AE),
            _ => (TeKind::Other, &[]),
        };
        let slots = resolve(&ids, names);
        for (k, s) in slots.iter().enumerate() {
            // m_hPlayer / m_iPlayerIndex are alternatives
            let alt_ok = kind == TeKind::AnimEvent
                && (k == 0 || k == 3)
                && (slots.first().copied().flatten().is_some()
                    || slots.get(3).copied().flatten().is_some());
            if s.is_none() && !alt_ok {
                if let Some((t, p)) = names.get(k) {
                    self.diag.push(format!("{name}: prop {t}.{p} not found"));
                }
            }
        }
        let c = Rc::new(TeClass {
            kind,
            n: ids.len(),
            count_key: format!("te.{name}"),
            slots,
        });
        if let Some(cell) = self.te.classes.get_mut(i) {
            *cell = Some(c.clone());
        }
        Some(c)
    }

    #[inline(never)]
    pub(crate) fn on_temp_entities(&mut self, msg: &TempEntitiesMessage, ps: &ParserState) {
        let st = self.state_tick();
        let mut prev_class: Option<u16> = None;
        let mut merged: Vec<Tv> = Vec::new();
        for ev in &msg.events {
            let class = u16::from(ev.class_id);
            let Some(tc) = self.te_class(class, ps) else {
                continue;
            };
            if prev_class != Some(class) {
                merged.clear();
                merged.resize(tc.n, Tv::Zero);
            }
            prev_class = Some(class);
            if tc.kind == TeKind::Other {
                continue;
            }
            for p in &ev.props {
                if let Some(cell) = merged.get_mut(p.index as usize) {
                    *cell = match &p.value {
                        SendPropValue::Integer(i) => Tv::I(*i),
                        SendPropValue::Float(f) => Tv::F(*f),
                        SendPropValue::Vector(v) => Tv::V([v.x, v.y, v.z]),
                        SendPropValue::VectorXY(v) => Tv::V([v.x, v.y, 0.0]),
                        _ => Tv::Zero,
                    };
                }
            }
            if st < 0 || st >= self.ticks as i64 {
                continue;
            }
            let g = |k: usize| -> Tv {
                tc.slots
                    .get(k)
                    .copied()
                    .flatten()
                    .and_then(|s| merged.get(s as usize).copied())
                    .unwrap_or(Tv::Zero)
            };
            self.count(&tc.count_key);
            match tc.kind {
                TeKind::FireBullets => {
                    if !self.incl.tempents {
                        continue;
                    }
                    let p = self.p_of_ent((g(0).i() + 1).clamp(0, 4095) as u32);
                    if p == 255 {
                        continue;
                    }
                    let o = g(5).v();
                    let pitch = g(6).f();
                    self.te.bullets.push(
                        st,
                        BulletRec {
                            player: p,
                            weapon_id: to_u8(g(1).i()),
                            crit: to_u8(g(2).i()),
                            seed: to_u8(g(3).i()),
                            spread: g(4).f(),
                            x: o[0],
                            y: o[1],
                            z: o[2],
                            pitch: if pitch > 180.0 { pitch - 360.0 } else { pitch },
                            yaw: g(7).f(),
                        },
                    );
                }
                TeKind::Explosion => {
                    if !self.incl.tempents {
                        continue;
                    }
                    let n = g(3).v();
                    let def = g(5).i();
                    let particle = g(7).i();
                    self.te.explosions.push(
                        st,
                        ExplosionRec {
                            x: g(0).f(),
                            y: g(1).f(),
                            z: g(2).f(),
                            nx: n[0],
                            ny: n[1],
                            nz: n[2],
                            weapon_id: to_u8(g(4).i()),
                            def: if (0..65535).contains(&def) {
                                def as u16
                            } else {
                                65535
                            },
                            entity: g(6).i().clamp(0, 65535) as u16,
                            particle: if (0..65535).contains(&particle) {
                                particle as u16
                            } else {
                                65535
                            },
                        },
                    );
                }
                TeKind::ParticleEffect => {
                    if !self.incl.tempents {
                        continue;
                    }
                    self.te.particles.push(
                        st,
                        ParticleRec {
                            name: g(0).i().clamp(0, 65535) as u16,
                            x: g(1).f(),
                            y: g(2).f(),
                            z: g(3).f(),
                            entity: g(4).i().clamp(0, 65535) as u16,
                        },
                    );
                }
                TeKind::EffectDispatch => {
                    let effect = g(0).i();
                    let ename = self
                        .effects
                        .get(effect.clamp(0, i64::MAX) as usize)
                        .cloned()
                        .unwrap_or_default();
                    self.count(&format!("effect.{ename}"));
                    if !self.incl.tempents {
                        continue;
                    }
                    let origin = [g(1).f(), g(2).f(), g(3).f()];
                    let start = [g(4).f(), g(5).f(), g(6).f()];
                    let entindex = g(7).i();
                    match ename.as_str() {
                        "Tracer" => {
                            let sentry = self
                                .ents
                                .get(entindex.clamp(0, 4095) as usize)
                                .and_then(|e| e.as_ref())
                                .filter(|e| e.role == Role::Building)
                                .and_then(|e| {
                                    self.bl.inst.get(e.slot).map(|r| (e.slot, r.data.kind))
                                })
                                .filter(|(_, k)| *k == 2)
                                .map(|(s, _)| s);
                            if let Some(sentry) = sentry {
                                self.te.tracers.push(
                                    st,
                                    TracerRec {
                                        sentry: sentry.min(65534) as u16,
                                        sx: start[0],
                                        sy: start[1],
                                        sz: start[2],
                                        ex: origin[0],
                                        ey: origin[1],
                                        ez: origin[2],
                                    },
                                );
                            }
                        }
                        "ParticleEffect" => {
                            self.te.particles.push(
                                st,
                                ParticleRec {
                                    name: g(8).i().clamp(0, 65535) as u16,
                                    x: origin[0],
                                    y: origin[1],
                                    z: origin[2],
                                    entity: entindex.clamp(0, 65535) as u16,
                                },
                            );
                        }
                        "ClientProjectile_Syringe" | "ClientProjectile_EnergyRing" => {
                            let kind = if ename == "ClientProjectile_Syringe" {
                                21
                            } else {
                                17
                            };
                            let crit = (g(9).i() & (1 << 20) != 0) as u8;
                            self.te.client.push(
                                st,
                                ClientProjRec {
                                    kind,
                                    player: self.p_of_ent(entindex.clamp(0, 4095) as u32),
                                    crit,
                                    x: origin[0],
                                    y: origin[1],
                                    z: origin[2],
                                    vx: start[0],
                                    vy: start[1],
                                    vz: start[2],
                                },
                            );
                        }
                        _ => {}
                    }
                }
                TeKind::AnimEvent => {
                    if !self.incl.players {
                        continue;
                    }
                    let p = if tc.slots.first().copied().flatten().is_some() {
                        self.p_of_handle(handle_parts(g(0).i()))
                    } else {
                        self.p_of_ent(g(3).i().clamp(0, 4095) as u32)
                    };
                    if p == 255 {
                        continue;
                    }
                    self.pl.anim.push(
                        st,
                        AnimRec {
                            player: p,
                            event: to_u8(g(1).i()),
                            data: g(2).i().clamp(0, 65535) as u16,
                        },
                    );
                }
                TeKind::Other => {}
            }
        }
    }

    #[inline(never)]
    pub(crate) fn tempents_out(&mut self, ord: &Orders) -> XNode {
        let bo = &ord.buildings;
        self.te
            .tracers
            .map_records(|r| r.sentry = bo.map_u16(r.sentry));
        let te = &mut self.te;
        ObjB::new()
            .put(
                "bullets",
                XNode::Obj(std::mem::replace(&mut te.bullets, EventBuilder::new(1)).into_fields()),
            )
            .put(
                "explosions",
                XNode::Obj(
                    std::mem::replace(&mut te.explosions, EventBuilder::new(1)).into_fields(),
                ),
            )
            .put(
                "tracers",
                XNode::Obj(std::mem::replace(&mut te.tracers, EventBuilder::new(1)).into_fields()),
            )
            .put(
                "particles",
                XNode::Obj(
                    std::mem::replace(&mut te.particles, EventBuilder::new(1)).into_fields(),
                ),
            )
            .put(
                "clientProjectiles",
                XNode::Obj(std::mem::replace(&mut te.client, EventBuilder::new(1)).into_fields()),
            )
            .build()
    }

    #[inline(never)]
    pub(crate) fn hurts_out(&mut self) -> XNode {
        XNode::Obj(std::mem::replace(&mut self.te.hurts, EventBuilder::new(1)).into_fields())
    }
}

fn empty_events<R: Record>() -> XNode {
    let mut f = vec![("tick".to_string(), XNode::U32(Vec::new()))];
    f.extend(R::columns(&[]));
    XNode::Obj(f)
}

#[inline(never)]
pub fn tempents_empty() -> XNode {
    ObjB::new()
        .put("bullets", empty_events::<BulletRec>())
        .put("explosions", empty_events::<ExplosionRec>())
        .put("tracers", empty_events::<TracerRec>())
        .put("particles", empty_events::<ParticleRec>())
        .put("clientProjectiles", empty_events::<ClientProjRec>())
        .build()
}

pub fn hurts_empty() -> XNode {
    empty_events::<HurtRec>()
}
