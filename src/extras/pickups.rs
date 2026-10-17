//! `pickups` and `ragdolls` groups (section 2.5.7).

use super::movers::VisRec;
use super::out::{ObjB, XNode};
use super::track::{empty_instances, empty_track, InstanceTable, TrackBuilder};
use super::tracker::{ClassInfo, Ent, Role};
use super::{to_u8, Ex, Slots};

/// `/items\/(medkit|ammopack)_(small|medium|large)\.mdl$/` -> (kind, size), or
/// `/plate[^/]*\.mdl$/` -> (health, medium).
pub fn pickup_kind(model: &str) -> Option<(u8, u8)> {
    let m = model.replace('\\', "/");
    if let Some(pos) = m.rfind("items/") {
        let rest = m.get(pos + 6..).unwrap_or("");
        for (kname, kind) in [("medkit_", 0u8), ("ammopack_", 1u8)] {
            if let Some(r) = rest.strip_prefix(kname) {
                for (sname, size) in [("small.mdl", 0u8), ("medium.mdl", 1u8), ("large.mdl", 2u8)] {
                    if r == sname {
                        return Some((kind, size));
                    }
                }
            }
        }
    }
    let file = m.rsplit('/').next().unwrap_or("");
    if file.ends_with(".mdl") && m.contains("plate") {
        // the match must be within the last path component
        if let Some(pos) = m.rfind("plate") {
            let tail = m.get(pos..).unwrap_or("");
            if !tail.contains('/') && tail.ends_with(".mdl") {
                return Some((0, 1));
            }
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct PickupRow {
    pub kind: u8,
    pub size: u8,
    pub model: u16,
    pub pos: [f32; 3],
}

#[derive(Default)]
pub struct RagdollSlots {
    pub player: Option<u32>,
    /// older demos: DT_TFRagdoll.m_iPlayerIndex (entity index)
    pub player_index: Option<u32>,
    pub cls: Option<u32>,
    pub team: Option<u32>,
    pub custom: Option<u32>,
    pub origin: Option<u32>,
    pub velocity: Option<u32>,
    /// m_bGib, m_bBurning, m_bFeignDeath, m_bIceRagdoll, m_bOnGround, m_bCloaked, m_bBecomeAsh,
    /// m_bElectrocuted, m_bGoldRagdoll (RAG bits 0..8)
    pub flags: [Option<u32>; 9],
}

pub fn setup_ragdoll(ci: &mut ClassInfo) -> Vec<String> {
    let t = "DT_TFRagdoll";
    let s = RagdollSlots {
        player: ci.find(t, "m_hPlayer"),
        player_index: ci.find(t, "m_iPlayerIndex"),
        cls: ci.find(t, "m_iClass"),
        team: ci.find(t, "m_iTeam"),
        custom: ci.find(t, "m_iDamageCustom"),
        origin: ci.find(t, "m_vecRagdollOrigin"),
        velocity: ci.find(t, "m_vecRagdollVelocity"),
        flags: [
            ci.find(t, "m_bGib"),
            ci.find(t, "m_bBurning"),
            ci.find(t, "m_bFeignDeath"),
            ci.find(t, "m_bIceRagdoll"),
            ci.find(t, "m_bOnGround"),
            ci.find(t, "m_bCloaked"),
            ci.find(t, "m_bBecomeAsh"),
            ci.find(t, "m_bElectrocuted"),
            ci.find(t, "m_bGoldRagdoll"),
        ],
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("m_hPlayer", s.player.or(s.player_index)),
        ("m_vecRagdollOrigin", s.origin),
        ("m_bFeignDeath", s.flags[2]),
    ] {
        if x.is_none() {
            missing.push(format!("{t}.{n}"));
        }
    }
    ci.slots = Slots::Ragdoll(Box::new(s));
    missing
}

#[derive(Clone, Debug)]
pub struct RagRow {
    pub player: u8,
    pub cls: u8,
    pub team: u8,
    pub flags: u16,
    pub custom: u16,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
}

pub struct PkState {
    pub inst: InstanceTable<PickupRow>,
    pub vis: TrackBuilder<VisRec>,
    pub rag: InstanceTable<RagRow>,
}

impl PkState {
    pub fn new(ticks: u32) -> PkState {
        PkState {
            inst: InstanceTable::new(),
            vis: TrackBuilder::new(ticks),
            rag: InstanceTable::new(),
        }
    }
}

impl Ex {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pk_create(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        kind: u8,
        size: u8,
        model: i64,
        origin: Option<u32>,
        st: i64,
    ) {
        let row = PickupRow {
            kind,
            size,
            model: model.clamp(0, 65535) as u16,
            pos: ent.v(origin).unwrap_or([0.0; 3]),
        };
        let k = self.pk.inst.add(idx, st, row);
        ent.role = Role::Pickup;
        ent.slot = k;
        if self.incl.pickups {
            self.pk.vis.push(k as usize, st, VisRec { v: 1 });
        }
    }

    pub(crate) fn pk_update(&mut self, _idx: u32, ent: &mut Ent, st: i64, revived: bool) {
        if revived && self.incl.pickups {
            self.pk.vis.push(ent.slot as usize, st, VisRec { v: 1 });
        }
    }

    pub(crate) fn pk_leave(&mut self, ent: &Ent, st: i64) {
        if self.incl.pickups {
            self.pk.vis.push(ent.slot as usize, st, VisRec { v: 0 });
        }
    }

    pub(crate) fn rag_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let Slots::Ragdoll(s) = &ci.slots else { return };
        let mut flags = 0u16;
        for (bit, slot) in s.flags.iter().enumerate() {
            if ent.b(*slot) {
                flags |= 1 << bit;
            }
        }
        let row = RagRow {
            player: if s.player.is_some() {
                self.p_of_handle(ent.h(s.player))
            } else {
                self.p_of_ent(ent.i0(s.player_index).clamp(0, 4095) as u32)
            },
            cls: to_u8(ent.i0(s.cls)),
            team: to_u8(ent.i0(s.team)),
            flags,
            custom: ent.i0(s.custom).clamp(0, 65535) as u16,
            pos: ent.v(s.origin).unwrap_or([f32::NAN; 3]),
            vel: ent.v(s.velocity).unwrap_or([f32::NAN; 3]),
        };
        let r = self.pk.rag.add(idx, st, row);
        ent.role = Role::Ragdoll;
        ent.slot = r;
    }

    #[inline(never)]
    pub(crate) fn pickups_out(&mut self) -> XNode {
        let o = self.pk.inst.order(self.ticks);
        let order = o.order.clone();
        let n = o.count();
        let t = &self.pk.inst;
        let base = t.base_fields(&o, self.ticks);
        let kind = t.column(&o, |r| r.data.kind);
        let size = t.column(&o, |r| r.data.size);
        let model = t.column(&o, |r| r.data.model);
        let x = t.column(&o, |r| r.data.pos[0]);
        let y = t.column(&o, |r| r.data.pos[1]);
        let z = t.column(&o, |r| r.data.pos[2]);
        let vis =
            std::mem::replace(&mut self.pk.vis, TrackBuilder::new(1)).into_fields(n, Some(&order));
        ObjB::new()
            .extend(base)
            .put("kind", kind)
            .put("size", size)
            .put("model", model)
            .put("x", x)
            .put("y", y)
            .put("z", z)
            .put("vis", XNode::Obj(vis))
            .build()
    }

    #[inline(never)]
    pub(crate) fn ragdolls_out(&mut self) -> XNode {
        let o = self.pk.rag.order(self.ticks);
        let t = &self.pk.rag;
        ObjB::new()
            .extend(t.base_fields(&o, self.ticks))
            .put("player", t.column(&o, |r| r.data.player))
            .put("cls", t.column(&o, |r| r.data.cls))
            .put("team", t.column(&o, |r| r.data.team))
            .put("flags", t.column(&o, |r| r.data.flags))
            .put("custom", t.column(&o, |r| r.data.custom))
            .put("x", t.column(&o, |r| r.data.pos[0]))
            .put("y", t.column(&o, |r| r.data.pos[1]))
            .put("z", t.column(&o, |r| r.data.pos[2]))
            .put("vx", t.column(&o, |r| r.data.vel[0]))
            .put("vy", t.column(&o, |r| r.data.vel[1]))
            .put("vz", t.column(&o, |r| r.data.vel[2]))
            .build()
    }
}

#[inline(never)]
pub fn pickups_empty() -> XNode {
    ObjB::new()
        .extend(empty_instances())
        .put("kind", XNode::U8(Vec::new()))
        .put("size", XNode::U8(Vec::new()))
        .put("model", XNode::U16(Vec::new()))
        .put("x", XNode::F32(Vec::new()))
        .put("y", XNode::F32(Vec::new()))
        .put("z", XNode::F32(Vec::new()))
        .put("vis", XNode::Obj(empty_track::<VisRec>(0)))
        .build()
}

#[inline(never)]
pub fn ragdolls_empty() -> XNode {
    let mut b = ObjB::new().extend(empty_instances());
    for k in ["player", "cls", "team"] {
        b = b.put(k, XNode::U8(Vec::new()));
    }
    b = b
        .put("flags", XNode::U16(Vec::new()))
        .put("custom", XNode::U16(Vec::new()));
    for k in ["x", "y", "z", "vx", "vy", "vz"] {
        b = b.put(k, XNode::F32(Vec::new()));
    }
    b.build()
}
