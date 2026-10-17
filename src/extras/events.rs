//! `events` (JSON) and `hurts` from game events (section 2.5.8). User ids map to player indices
//! through the `userinfo` table (rule 2.3.9). References to building, projectile and timer
//! instances are resolved at the end against the instances alive at the event tick.

use crate::demo::gamevent::GameEvent;
use crate::demo::message::gameevent::GameEventMessage;

use super::out::{HlEvent, XNode};
use super::tempents::HurtRec;
use super::track::{InstOrder, InstanceTable, NONE_U32};
use super::{to_u8, Ex, Orders};

#[derive(Clone, Copy)]
enum Ref {
    /// building alive with this entity index
    Building(u32),
    /// the building a sapper with this entity index is built on
    SapperOf(u32),
    /// projectile alive with this entity index
    Projectile(u32),
    /// timer alive with this entity index
    Timer(u32),
}

pub struct EvState {
    pub list: Vec<HlEvent>,
    pending: Vec<(usize, Ref)>,
    ticks: u32,
}

impl EvState {
    pub fn new(ticks: u32) -> EvState {
        EvState {
            list: Vec::new(),
            pending: Vec::new(),
            ticks,
        }
    }
}

fn s(v: &crate::demo::data::MaybeUtf8String) -> String {
    let r: &str = v.as_ref();
    r.to_string()
}

fn bytes(v: &crate::demo::data::MaybeUtf8String) -> Vec<u8> {
    match v {
        crate::demo::data::MaybeUtf8String::Valid(s) => s.as_bytes().to_vec(),
        crate::demo::data::MaybeUtf8String::Invalid(b) => b.clone(),
    }
}

/// Latest instance with `entity` alive at `tick` (created <= tick <= deleted).
fn alive_at<T>(table: &InstanceTable<T>, entity: u32, tick: i64) -> Option<u32> {
    let mut best: Option<(i64, u32)> = None;
    for (i, r) in table.rows.iter().enumerate() {
        if r.entity as u32 != entity || r.created > tick {
            continue;
        }
        if let Some(d) = r.deleted {
            if d < tick {
                continue;
            }
        }
        if best.map(|b| r.created >= b.0).unwrap_or(true) {
            best = Some((r.created, i as u32));
        }
    }
    best.map(|b| b.1)
}

fn map_ref(ord: &InstOrder, prov: Option<u32>) -> i32 {
    match prov {
        Some(p) => {
            let f = ord.map(p);
            if f == NONE_U32 {
                -1
            } else {
                f as i32
            }
        }
        None => -1,
    }
}

impl Ex {
    /// Player index of an event user id, 255 unknown.
    fn p_of_user(&self, uid: u32) -> u32 {
        match self.user_ent.get(&uid) {
            Some(e) => self.p_of_ent(*e) as u32,
            None => 255,
        }
    }

    /// Pushes a derived or decoded event at cache tick `t` (dropped outside `0..ticks`).
    pub(crate) fn ev_push(&mut self, t: i64, mut ev: HlEvent) -> Option<usize> {
        if !self.incl.events || t < 0 || t >= self.ev.ticks as i64 {
            return None;
        }
        ev.set_tick(t as u32);
        self.ev.list.push(ev);
        Some(self.ev.list.len() - 1)
    }

    fn ev_push_ref(&mut self, t: i64, ev: HlEvent, r: Ref) {
        if let Some(i) = self.ev_push(t, ev) {
            self.ev.pending.push((i, r));
        }
    }

    #[inline(never)]
    pub(crate) fn on_game_event(&mut self, msg: &GameEventMessage) {
        *self
            .gev_counts
            .entry(msg.event_type.as_str().to_string())
            .or_insert(0) += 1;
        let t = self.event_tick();
        if t < 0 || t >= self.ticks as i64 {
            return;
        }
        let u = |ex: &Ex, id: u16| ex.p_of_user(id as u32);
        match &msg.event {
            GameEvent::PlayerDeath(d) => {
                let assister = if d.assister == 65535 || d.assister == 255 {
                    255
                } else {
                    u(self, d.assister)
                };
                let ev = HlEvent::Kill {
                    tick: 0,
                    victim: u(self, d.user_id),
                    attacker: u(self, d.attacker),
                    assister,
                    inflictor: d.inflictor_ent_index,
                    weapon: s(&d.weapon),
                    weapon_log: s(&d.weapon_log_class_name),
                    weapon_def: d.weapon_def_index,
                    weapon_id: d.weapon_id as u32,
                    death_flags: d.death_flags as u32,
                    custom_kill: d.custom_kill as u32,
                    crit_type: d.crit_type as u32,
                    feign: d.death_flags & 0x20 != 0,
                };
                self.ev_push(t, ev);
            }
            GameEvent::PlayerHurt(h) => {
                let victim = u(self, h.user_id);
                if victim != 255 && self.incl.hurts {
                    let crit = if h.crit {
                        2
                    } else if h.mini_crit {
                        1
                    } else {
                        0
                    };
                    self.te.hurts.push(
                        t,
                        HurtRec {
                            victim: victim as u8,
                            attacker: u(self, h.attacker) as u8,
                            damage: h.damage_amount,
                            health: h.health.min(i16::MAX as u16) as i16,
                            crit,
                            weapon_id: to_u8(h.weapon_id as i64),
                            custom: h.custom,
                        },
                    );
                }
            }
            GameEvent::PlayerChargeDeployed(e) => {
                let ev = HlEvent::UberDeployed {
                    tick: 0,
                    medic: u(self, e.user_id),
                    target: u(self, e.target_id),
                };
                self.ev_push(t, ev);
            }
            GameEvent::MedicDeath(e) => {
                let ev = HlEvent::MedicDeath {
                    tick: 0,
                    medic: u(self, e.user_id),
                    attacker: u(self, e.attacker),
                    healing: e.healing as u32,
                    charged: e.charged,
                };
                self.ev_push(t, ev);
            }
            GameEvent::ObjectDeflected(e) => {
                let entity = e.object_ent_index as u32;
                let ev = HlEvent::Deflect {
                    tick: 0,
                    pyro: u(self, e.user_id),
                    owner: u(self, e.owner_id),
                    weapon_id: e.weapon_id as u32,
                    entity,
                    projectile: -1,
                };
                self.ev_push_ref(t, ev, Ref::Projectile(entity));
            }
            GameEvent::ObjectDestroyed(e) => {
                let ev = HlEvent::Destroyed {
                    tick: 0,
                    building: -1,
                    owner: u(self, e.user_id),
                    attacker: u(self, e.attacker),
                    assister: u(self, e.assister),
                    weapon: s(&e.weapon),
                    kind: e.object_type as u32,
                    was_building: e.was_building,
                };
                self.ev_push_ref(t, ev, Ref::Building(e.index as u32));
            }
            GameEvent::ObjectDetonated(e) => {
                let ev = HlEvent::Detonated {
                    tick: 0,
                    building: -1,
                    owner: u(self, e.user_id),
                    kind: e.object_type as u32,
                };
                self.ev_push_ref(t, ev, Ref::Building(e.index as u32));
            }
            GameEvent::PlayerSappedObject(e) => {
                let ev = HlEvent::Sapped {
                    tick: 0,
                    building: -1,
                    spy: u(self, e.user_id),
                    owner: u(self, e.owner_id),
                    kind: e.object as u32,
                };
                self.ev_push_ref(t, ev, Ref::SapperOf(e.sapper_id as u32));
            }
            GameEvent::PlayerTeleported(e) => {
                let ev = HlEvent::Teleported {
                    tick: 0,
                    player: u(self, e.user_id),
                    builder: u(self, e.builder_id),
                    dist: e.dist,
                };
                self.ev_push(t, ev);
            }
            GameEvent::TeamPlayPointStartCapture(e) => {
                let cappers = bytes(&e.cappers)
                    .iter()
                    .map(|b| self.p_of_ent(*b as u32) as u32)
                    .collect();
                let ev = HlEvent::CapStart {
                    tick: 0,
                    cp: e.cp as u32,
                    cp_name: s(&e.cp_name),
                    team: e.team as u32,
                    cap_team: e.cap_team as u32,
                    cappers,
                    cap_time: e.cap_time,
                };
                self.ev_push(t, ev);
            }
            GameEvent::TeamPlayPointCaptured(e) => {
                let cappers = bytes(&e.cappers)
                    .iter()
                    .map(|b| self.p_of_ent(*b as u32) as u32)
                    .collect();
                let ev = HlEvent::Captured {
                    tick: 0,
                    cp: e.cp as u32,
                    cp_name: s(&e.cp_name),
                    team: e.team as u32,
                    cappers,
                };
                self.ev_push(t, ev);
            }
            GameEvent::TeamPlayCaptureBlocked(e) => {
                let ev = HlEvent::CapBlocked {
                    tick: 0,
                    cp: e.cp as u32,
                    cp_name: s(&e.cp_name),
                    blocker: self.p_of_ent(e.blocker as u32) as u32,
                    victim: self.p_of_ent(e.victim as u32) as u32,
                };
                self.ev_push(t, ev);
            }
            GameEvent::TeamPlayCaptureBroken(e) => {
                let ev = HlEvent::CapBroken {
                    tick: 0,
                    cp: e.cp as u32,
                    cp_name: s(&e.cp_name),
                    time_remaining: e.time_remaining,
                };
                self.ev_push(t, ev);
            }
            GameEvent::TeamPlayPointLocked(e) => {
                self.ev_push(
                    t,
                    HlEvent::CpLocked {
                        tick: 0,
                        cp: e.cp as u32,
                    },
                );
            }
            GameEvent::TeamPlayPointUnlocked(e) => {
                self.ev_push(
                    t,
                    HlEvent::CpUnlocked {
                        tick: 0,
                        cp: e.cp as u32,
                    },
                );
            }
            GameEvent::TeamPlayRoundStart(e) => {
                self.ev_push(
                    t,
                    HlEvent::RoundStart {
                        tick: 0,
                        full_reset: e.full_reset,
                    },
                );
            }
            GameEvent::TeamPlayRoundActive(_) => {
                self.ev_push(t, HlEvent::RoundActive { tick: 0 });
            }
            GameEvent::TeamPlaySetupFinished(_) => {
                self.ev_push(t, HlEvent::SetupFinished { tick: 0 });
            }
            GameEvent::TeamPlayRoundWin(e) => {
                self.ev_push(
                    t,
                    HlEvent::RoundWin {
                        tick: 0,
                        team: e.team as u32,
                        win_reason: e.win_reason as u32,
                        full_round: e.full_round != 0,
                        round_time: e.round_time,
                        sudden_death: e.was_sudden_death != 0,
                    },
                );
            }
            GameEvent::TeamPlayTimerTimeAdded(e) => {
                let ev = HlEvent::TimeAdded {
                    tick: 0,
                    timer: -1,
                    seconds: e.seconds_added as u32,
                };
                self.ev_push_ref(t, ev, Ref::Timer(e.timer as u32));
            }
            GameEvent::TeamPlayGameOver(e) => {
                self.ev_push(
                    t,
                    HlEvent::GameOver {
                        tick: 0,
                        reason: s(&e.reason),
                    },
                );
            }
            GameEvent::TfGameOver(e) => {
                self.ev_push(
                    t,
                    HlEvent::GameOver {
                        tick: 0,
                        reason: s(&e.reason),
                    },
                );
            }
            GameEvent::DeployBuffBanner(e) => {
                let ev = HlEvent::Banner {
                    tick: 0,
                    owner: u(self, e.buff_owner),
                    buff_type: e.buff_type as u32,
                };
                self.ev_push(t, ev);
            }
            GameEvent::ItemPickup(e) => {
                let ev = HlEvent::Pickup {
                    tick: 0,
                    player: u(self, e.user_id),
                    item: s(&e.item),
                };
                self.ev_push(t, ev);
            }
            GameEvent::PlayerSpawn(e) => {
                let ev = HlEvent::Spawn {
                    tick: 0,
                    player: u(self, e.user_id),
                    cls: e.class as u32,
                    team: e.team as u32,
                };
                self.ev_push(t, ev);
            }
            GameEvent::PlayerChangeClass(e) => {
                let ev = HlEvent::ClassChange {
                    tick: 0,
                    player: u(self, e.user_id),
                    cls: e.class as u32,
                };
                self.ev_push(t, ev);
            }
            GameEvent::PlayerTeam(e) => {
                let ev = HlEvent::TeamChange {
                    tick: 0,
                    player: u(self, e.user_id),
                    team: e.team as u32,
                    old_team: e.old_team as u32,
                    disconnect: e.disconnect,
                };
                self.ev_push(t, ev);
            }
            GameEvent::PlayerDisconnect(e) => {
                let ev = HlEvent::Disconnect {
                    tick: 0,
                    player: u(self, e.user_id),
                    reason: s(&e.reason),
                };
                self.ev_push(t, ev);
            }
            _ => {}
        }
    }

    #[inline(never)]
    pub(crate) fn events_out(&mut self, ord: &Orders) -> XNode {
        let pending = std::mem::take(&mut self.ev.pending);
        for (i, r) in pending {
            let tick = self.ev.list.get(i).map(|e| e.tick() as i64).unwrap_or(0);
            let value = match r {
                Ref::Building(e) => map_ref(&ord.buildings, alive_at(&self.bl.inst, e, tick)),
                Ref::SapperOf(e) => {
                    let on = alive_at(&self.bl.inst, e, tick)
                        .and_then(|sap| self.bl.inst.get(sap))
                        .map(|r| r.data.built_on)
                        .filter(|b| *b >= 0)
                        .map(|b| b as u32);
                    map_ref(&ord.buildings, on)
                }
                Ref::Projectile(e) => {
                    if self.p_of_ent(e) != 255 || (1..=101).contains(&e) {
                        -1
                    } else {
                        map_ref(&ord.projectiles, alive_at(&self.pj.inst, e, tick))
                    }
                }
                Ref::Timer(e) => map_ref(&ord.timers, alive_at(&self.ob.timers, e, tick)),
            };
            if let Some(ev) = self.ev.list.get_mut(i) {
                match ev {
                    HlEvent::Destroyed { building, .. }
                    | HlEvent::Detonated { building, .. }
                    | HlEvent::Sapped { building, .. } => *building = value,
                    HlEvent::Deflect { projectile, .. } => *projectile = value,
                    HlEvent::TimeAdded { timer, .. } => *timer = value,
                    _ => {}
                }
            }
        }
        // remap building indices of derived building events
        let bo = &ord.buildings;
        for ev in self.ev.list.iter_mut() {
            match ev {
                HlEvent::BuildStart { building, .. }
                | HlEvent::BuildDone { building, .. }
                | HlEvent::Upgrade { building, .. }
                | HlEvent::Carry { building, .. }
                | HlEvent::Redeploy { building, .. } => *building = bo.map_i32(*building),
                _ => {}
            }
        }
        let mut list = std::mem::take(&mut self.ev.list);
        list.sort_by_key(|e| e.tick());
        XNode::Events(list)
    }
}
