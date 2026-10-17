//! `flames` group (section 2.5.5): `CTFFlameManager` firing state and flame bounding box.

use super::out::{ObjB, XNode};
use super::track::{empty_instances, empty_track, record, InstanceTable, TrackBuilder};
use super::tracker::{ClassInfo, Ent, Role};
use super::{Ex, Slots};

record!(BoxRec {
    firing: u8 => "firing", cx: f32 => "cx", cy: f32 => "cy", cz: f32 => "cz",
    hx: f32 => "hx", hy: f32 => "hy", hz: f32 => "hz"
});

#[derive(Default, Clone, Copy)]
pub struct FlameSlots {
    pub attacker: Option<u32>,
    pub firing: Option<u32>,
    pub origin: Option<u32>,
    pub mins: Option<u32>,
    pub maxs: Option<u32>,
}

pub fn setup(ci: &mut ClassInfo) -> Vec<String> {
    let s = FlameSlots {
        attacker: ci.find("DT_TFFlameManager", "m_hAttacker"),
        firing: ci.find("DT_TFFlameManager", "m_bIsFiring"),
        origin: ci.find("DT_BaseEntity", "m_vecOrigin"),
        mins: ci.find("DT_CollisionProperty", "m_vecMins"),
        maxs: ci.find("DT_CollisionProperty", "m_vecMaxs"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("DT_TFFlameManager.m_hAttacker", s.attacker),
        ("DT_TFFlameManager.m_bIsFiring", s.firing),
        ("DT_BaseEntity.m_vecOrigin", s.origin),
        ("DT_CollisionProperty.m_vecMins", s.mins),
        ("DT_CollisionProperty.m_vecMaxs", s.maxs),
    ] {
        if x.is_none() {
            missing.push(n.to_string());
        }
    }
    ci.keep_only(&[s.attacker, s.firing, s.origin, s.mins, s.maxs]);
    ci.slots = Slots::Flame(s);
    missing
}

pub struct FlState {
    pub inst: InstanceTable<u8>,
    pub boxes: TrackBuilder<BoxRec>,
}

impl FlState {
    pub fn new(ticks: u32) -> FlState {
        FlState {
            inst: InstanceTable::new(),
            boxes: TrackBuilder::new(ticks),
        }
    }
}

impl Ex {
    pub(crate) fn fl_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let Slots::Flame(s) = &ci.slots else { return };
        let owner = self.p_of_handle(ent.h(s.attacker));
        let f = self.fl.inst.add(idx, st, owner);
        ent.role = Role::Flame;
        ent.slot = f;
        self.fl_update(idx, ent, ci, st);
    }

    pub(crate) fn fl_update(&mut self, _idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        if !self.incl.flames {
            return;
        }
        let Slots::Flame(s) = &ci.slots else { return };
        let o = ent.v(s.origin).unwrap_or([0.0; 3]);
        let mn = ent.v(s.mins).unwrap_or([0.0; 3]);
        let mx = ent.v(s.maxs).unwrap_or([0.0; 3]);
        let rec = BoxRec {
            firing: ent.b(s.firing) as u8,
            cx: o[0] + (mn[0] + mx[0]) / 2.0,
            cy: o[1] + (mn[1] + mx[1]) / 2.0,
            cz: o[2] + (mn[2] + mx[2]) / 2.0,
            hx: (mx[0] - mn[0]) / 2.0,
            hy: (mx[1] - mn[1]) / 2.0,
            hz: (mx[2] - mn[2]) / 2.0,
        };
        self.fl.boxes.push(ent.slot as usize, st, rec);
    }

    #[inline(never)]
    pub(crate) fn flames_out(&mut self) -> XNode {
        let o = self.fl.inst.order(self.ticks);
        let order = o.order.clone();
        let n = o.count();
        let base = self.fl.inst.base_fields(&o, self.ticks);
        let owner = self.fl.inst.column(&o, |r| r.data);
        let boxes = std::mem::replace(&mut self.fl.boxes, TrackBuilder::new(1))
            .into_fields(n, Some(&order));
        ObjB::new()
            .extend(base)
            .put("owner", owner)
            .put("box", XNode::Obj(boxes))
            .build()
    }
}

pub fn flames_empty() -> XNode {
    ObjB::new()
        .extend(empty_instances())
        .put("owner", XNode::U8(Vec::new()))
        .put("box", XNode::Obj(empty_track::<BoxRec>(0)))
        .build()
}
