//! `dots` group (section 2.5.5): `CSniperDot` (laser 0) and `CLaserDot` (laser 1, Wrangler).

use super::out::{ObjB, XNode};
use super::track::{empty_instances, empty_track, record, InstanceTable, TrackBuilder};
use super::tracker::{ClassInfo, Ent, Role};
use super::{to_u8, Ex, Slots};

record!(DotPosRec { x: f32 => "x", y: f32 => "y", z: f32 => "z", charge_tick: f32 => "chargeTick" });

#[derive(Default, Clone, Copy)]
pub struct DotSlots {
    pub laser: u8,
    pub owner: Option<u32>,
    pub team: Option<u32>,
    pub origin: Option<u32>,
    pub charge: Option<u32>,
}

pub fn setup(ci: &mut ClassInfo) -> Vec<String> {
    let s = DotSlots {
        laser: (ci.name == "CLaserDot") as u8,
        owner: ci.find("DT_BaseEntity", "m_hOwnerEntity"),
        team: ci.find("DT_BaseEntity", "m_iTeamNum"),
        origin: ci.find("DT_BaseEntity", "m_vecOrigin"),
        charge: ci.find("DT_SniperDot", "m_flChargeStartTime"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("DT_BaseEntity.m_hOwnerEntity", s.owner),
        ("DT_BaseEntity.m_vecOrigin", s.origin),
        ("DT_SniperDot.m_flChargeStartTime", s.charge),
    ] {
        if x.is_none() {
            missing.push(n.to_string());
        }
    }
    ci.keep_only(&[s.owner, s.team, s.origin, s.charge]);
    ci.slots = Slots::Dot(s);
    missing
}

#[derive(Clone, Debug)]
pub struct DotRow {
    pub owner: u8,
    pub team: u8,
    pub laser: u8,
}

pub struct DtState {
    pub inst: InstanceTable<DotRow>,
    pub pos: TrackBuilder<DotPosRec>,
}

impl DtState {
    pub fn new(ticks: u32) -> DtState {
        DtState {
            inst: InstanceTable::new(),
            pos: TrackBuilder::new(ticks),
        }
    }
}

impl Ex {
    pub(crate) fn dt_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let Slots::Dot(s) = &ci.slots else { return };
        let row = DotRow {
            owner: self.p_of_handle(ent.h(s.owner)),
            team: to_u8(ent.i0(s.team)),
            laser: s.laser,
        };
        let d = self.dt.inst.add(idx, st, row);
        ent.role = Role::Dot;
        ent.slot = d;
        self.dt_update(idx, ent, ci, st);
    }

    pub(crate) fn dt_update(&mut self, _idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        if !self.incl.dots {
            return;
        }
        let Slots::Dot(s) = &ci.slots else { return };
        let o = ent.v(s.origin).unwrap_or([f32::NAN; 3]);
        let rec = DotPosRec {
            x: o[0],
            y: o[1],
            z: o[2],
            charge_tick: self.tick_f(ent.f0(s.charge)),
        };
        self.dt.pos.push(ent.slot as usize, st, rec);
    }

    #[inline(never)]
    pub(crate) fn dots_out(&mut self) -> XNode {
        let o = self.dt.inst.order(self.ticks);
        let order = o.order.clone();
        let n = o.count();
        let t = &self.dt.inst;
        let base = t.base_fields(&o, self.ticks);
        let owner = t.column(&o, |r| r.data.owner);
        let team = t.column(&o, |r| r.data.team);
        let laser = t.column(&o, |r| r.data.laser);
        let pos =
            std::mem::replace(&mut self.dt.pos, TrackBuilder::new(1)).into_fields(n, Some(&order));
        ObjB::new()
            .extend(base)
            .put("owner", owner)
            .put("team", team)
            .put("laser", laser)
            .put("pos", XNode::Obj(pos))
            .build()
    }
}

pub fn dots_empty() -> XNode {
    ObjB::new()
        .extend(empty_instances())
        .put("owner", XNode::U8(Vec::new()))
        .put("team", XNode::U8(Vec::new()))
        .put("laser", XNode::U8(Vec::new()))
        .put("pos", XNode::Obj(empty_track::<DotPosRec>(0)))
        .build()
}
