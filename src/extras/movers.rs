//! Absolute transforms with parent composition (section 2.6.2), the `movers` group and the
//! `drops` group (section 2.5.7).

use fnv::{FnvHashMap, FnvHashSet};

use super::buildings::XformRec;
use super::out::{ObjB, XNode};
use super::track::{empty_instances, empty_track, record, InstOrder, InstanceTable, TrackBuilder};
use super::tracker::{angle_quat, quat_mul, quat_rotate, ClassInfo, Ent, Group, Quat, Role, Val};
use super::{def_u16, Ex, Orders, Slots};

record!(VisRec { v: u8 => "v" });

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XfTarget {
    Building(u32),
    Mover(u32),
    Drop(u32),
}

pub struct XfNode {
    local_p: [f32; 3],
    local_q: Quat,
    parent: Option<(u32, u32)>,
    abs_p: [f32; 3],
    abs_q: Quat,
    has_abs: bool,
    target: XfTarget,
    class: u16,
}

#[derive(Default, Clone, Copy)]
pub struct MoverSlots {
    pub cls: u8,
    pub origin: Option<u32>,
    pub angles: Option<u32>,
    /// separate angle floats (CFuncRotating: DT_FuncRotating.m_angRotation[0..2])
    pub ang3: Option<[Option<u32>; 3]>,
    pub parent: Option<u32>,
    pub model: Option<u32>,
}

const M_XF: u32 = 1;

pub fn setup_mover(ci: &mut ClassInfo) -> Vec<String> {
    let cls = match ci.name.as_str() {
        "CBaseDoor" => 0,
        "CFuncRotating" => 1,
        "CFuncTrackTrain" => 2,
        "CPhysicsProp" | "CPhysicsPropMultiplayer" => 4,
        "CDynamicProp" => 5,
        _ => 3,
    };
    let rot3 = [
        ci.find("DT_FuncRotating", "m_angRotation[0]"),
        ci.find("DT_FuncRotating", "m_angRotation[1]"),
        ci.find("DT_FuncRotating", "m_angRotation[2]"),
    ];
    let s = MoverSlots {
        cls,
        origin: ci
            .find("DT_BaseEntity", "m_vecOrigin")
            .or_else(|| ci.find("DT_FuncRotating", "m_vecOrigin")),
        angles: ci.find("DT_BaseEntity", "m_angRotation"),
        ang3: if rot3.iter().any(|x| x.is_some()) {
            Some(rot3)
        } else {
            None
        },
        parent: ci.find("DT_BaseEntity", "moveparent"),
        model: ci.find("DT_BaseEntity", "m_nModelIndex"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("DT_BaseEntity.m_vecOrigin", s.origin),
        ("DT_BaseEntity.m_nModelIndex", s.model),
    ] {
        if x.is_none() {
            missing.push(n.to_string());
        }
    }
    ci.add_dep(s.origin, M_XF);
    ci.add_dep(s.angles, M_XF);
    ci.add_dep(s.parent, M_XF);
    let a3 = s.ang3.unwrap_or([None; 3]);
    for a in a3 {
        ci.add_dep(a, M_XF);
    }
    ci.keep_only(&[s.origin, s.angles, s.parent, s.model, a3[0], a3[1], a3[2]]);
    ci.slots = Slots::Mover(s);
    missing
}

#[derive(Default, Clone, Copy)]
pub struct DropSlots {
    pub weapon: bool,
    pub origin: Option<u32>,
    pub angles: Option<u32>,
    pub ang3: [Option<u32>; 3],
    pub parent: Option<u32>,
    pub model: Option<u32>,
    pub def: Option<u32>,
}

pub fn setup_drop(ci: &mut ClassInfo) -> Vec<String> {
    let weapon = ci.name == "CTFDroppedWeapon";
    let s = DropSlots {
        weapon,
        origin: ci.find("DT_BaseEntity", "m_vecOrigin"),
        angles: if weapon {
            ci.find("DT_BaseEntity", "m_angRotation")
        } else {
            None
        },
        ang3: if weapon {
            [None; 3]
        } else {
            [
                ci.find("DT_AmmoPack", "m_angRotation[0]"),
                ci.find("DT_AmmoPack", "m_angRotation[1]"),
                ci.find("DT_AmmoPack", "m_angRotation[2]"),
            ]
        },
        parent: ci.find("DT_BaseEntity", "moveparent"),
        model: ci.find("DT_BaseEntity", "m_nModelIndex"),
        def: ci.find("DT_ScriptCreatedItem", "m_iItemDefinitionIndex"),
    };
    let mut missing = Vec::new();
    if s.origin.is_none() {
        missing.push("DT_BaseEntity.m_vecOrigin".to_string());
    }
    if weapon && s.def.is_none() {
        missing.push("DT_ScriptCreatedItem.m_iItemDefinitionIndex".to_string());
    }
    if !weapon && s.ang3.iter().any(|x| x.is_none()) {
        missing.push("DT_AmmoPack.m_angRotation[0..2]".to_string());
    }
    for x in [
        s.origin, s.angles, s.parent, s.ang3[0], s.ang3[1], s.ang3[2],
    ] {
        ci.add_dep(x, M_XF);
    }
    ci.keep_only(&[
        s.origin, s.angles, s.parent, s.ang3[0], s.ang3[1], s.ang3[2], s.model, s.def,
    ]);
    ci.slots = Slots::Drop(s);
    missing
}

#[derive(Clone, Debug)]
pub struct MoverRow {
    pub cls: u8,
    pub bsp_model: i16,
    pub model: u16,
    pub parent: i32,
    /// moveparent handle at creation, resolved to `parent` after the message
    pub parent_h: Option<(u32, u32)>,
    pub serial: u32,
    pub cart: bool,
    pub train: bool,
}

#[derive(Clone, Debug)]
pub struct DropRow {
    pub kind: u8,
    pub model: u16,
    pub def: u16,
}

pub struct MvState {
    pub xf: FnvHashMap<u32, XfNode>,
    children: FnvHashMap<u32, Vec<u32>>,
    dirty: Vec<u32>,
    pub inst: InstanceTable<MoverRow>,
    pub xform: TrackBuilder<XformRec>,
    pub vis: TrackBuilder<VisRec>,
    pub drops: InstanceTable<DropRow>,
    pub drop_xform: TrackBuilder<XformRec>,
}

impl MvState {
    pub fn new(ticks: u32) -> MvState {
        MvState {
            xf: FnvHashMap::default(),
            children: FnvHashMap::default(),
            dirty: Vec::new(),
            inst: InstanceTable::new(),
            xform: TrackBuilder::new(ticks),
            vis: TrackBuilder::new(ticks),
            drops: InstanceTable::new(),
            drop_xform: TrackBuilder::new(ticks),
        }
    }
}

/// `/bomb_cart|minecart|_cart\b|cart\.mdl/i`
pub fn is_cart_model(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("bomb_cart") || n.contains("minecart") || n.contains("cart.mdl") {
        return true;
    }
    let mut from = 0;
    while let Some(pos) = n.get(from..).and_then(|s| s.find("_cart")) {
        let end = from + pos + 5;
        let next = n.as_bytes().get(end).copied();
        match next {
            None => return true,
            Some(c) if !(c.is_ascii_alphanumeric() || c == b'_') => return true,
            _ => {}
        }
        from = from + pos + 1;
    }
    false
}

/// `*N` brush model index.
pub fn bsp_model_of(name: &str) -> i16 {
    match name.strip_prefix('*') {
        Some(rest) => rest.parse::<i16>().unwrap_or(-1),
        None => -1,
    }
}

fn local_of(
    ent: &Ent,
    origin: Option<u32>,
    angles: Option<u32>,
    ang3: Option<[Option<u32>; 3]>,
) -> ([f32; 3], Quat) {
    let p = ent.v(origin).unwrap_or([0.0; 3]);
    let a = match ang3 {
        Some(a3) => [ent.f0(a3[0]), ent.f0(a3[1]), ent.f0(a3[2])],
        None => ent.v(angles).unwrap_or([0.0; 3]),
    };
    (p, angle_quat(a))
}

impl Ex {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn xf_register(
        &mut self,
        idx: u32,
        ent: &Ent,
        origin: Option<u32>,
        angles: Option<u32>,
        ang3: Option<[Option<u32>; 3]>,
        parent: Option<u32>,
        target: XfTarget,
        _st: i64,
    ) {
        let (lp, lq) = local_of(ent, origin, angles, ang3);
        let par = ent.h(parent);
        if let Some((pi, _)) = par {
            self.mv.children.entry(pi).or_default().push(idx);
        }
        self.mv.xf.insert(
            idx,
            XfNode {
                local_p: lp,
                local_q: lq,
                parent: par,
                abs_p: lp,
                abs_q: lq,
                has_abs: false,
                target,
                class: ent.class,
            },
        );
        if !self.mv.dirty.contains(&idx) {
            self.mv.dirty.push(idx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn xf_local(
        &mut self,
        idx: u32,
        ent: &Ent,
        origin: Option<u32>,
        angles: Option<u32>,
        ang3: Option<[Option<u32>; 3]>,
        parent: Option<u32>,
        _st: i64,
    ) {
        let (lp, lq) = local_of(ent, origin, angles, ang3);
        let par = ent.h(parent);
        let old_parent = match self.mv.xf.get_mut(&idx) {
            Some(node) => {
                node.local_p = lp;
                node.local_q = lq;
                let old = node.parent;
                node.parent = par;
                old
            }
            None => return,
        };
        if old_parent != par {
            if let Some((oi, _)) = old_parent {
                if let Some(list) = self.mv.children.get_mut(&oi) {
                    list.retain(|c| *c != idx);
                }
            }
            if let Some((pi, _)) = par {
                self.mv.children.entry(pi).or_default().push(idx);
            }
        }
        if !self.mv.dirty.contains(&idx) {
            self.mv.dirty.push(idx);
        }
    }

    pub(crate) fn xf_unregister(&mut self, idx: u32) {
        if let Some(node) = self.mv.xf.remove(&idx) {
            if let Some((pi, _)) = node.parent {
                if let Some(list) = self.mv.children.get_mut(&pi) {
                    list.retain(|c| *c != idx);
                }
            }
        }
        self.mv.dirty.retain(|d| *d != idx);
    }

    pub(crate) fn xf_flush(&mut self, st: i64) {
        if self.mv.dirty.is_empty() {
            return;
        }
        let dirty = std::mem::take(&mut self.mv.dirty);
        let set: FnvHashSet<u32> = dirty.iter().copied().collect();
        for idx in &dirty {
            let parent_dirty = self
                .mv
                .xf
                .get(idx)
                .and_then(|n| n.parent)
                .map(|(pi, _)| set.contains(&pi) && self.mv.xf.contains_key(&pi))
                .unwrap_or(false);
            if parent_dirty {
                continue;
            }
            self.xf_recompute(*idx, st, 0);
        }
    }

    #[inline(never)]
    fn xf_recompute(&mut self, idx: u32, st: i64, depth: u32) {
        let Some(node) = self.mv.xf.get(&idx) else {
            return;
        };
        let (lp, lq, parent, class, target) = (
            node.local_p,
            node.local_q,
            node.parent,
            node.class,
            node.target,
        );
        let parent_abs = match parent {
            None => None,
            Some((pi, ps)) => {
                let live = self
                    .ents
                    .get(pi as usize)
                    .and_then(|e| e.as_ref())
                    .map(|e| e.serial == ps)
                    .unwrap_or(false);
                match self.mv.xf.get(&pi) {
                    Some(par) if live && par.has_abs => Some((par.abs_p, par.abs_q)),
                    _ => {
                        let name = self.class_name(class).to_string();
                        let key = format!("untracked parent for {name}");
                        if !self.warned.contains(&key) {
                            self.warned.insert(key);
                            self.warnings
                                .push(format!("untracked parent for {name} {idx}"));
                        }
                        None
                    }
                }
            }
        };
        let (ap, aq) = match parent_abs {
            Some((pp, pq)) => {
                let r = quat_rotate(pq, lp);
                ([pp[0] + r[0], pp[1] + r[1], pp[2] + r[2]], quat_mul(pq, lq))
            }
            None => (lp, lq),
        };
        if let Some(node) = self.mv.xf.get_mut(&idx) {
            node.abs_p = ap;
            node.abs_q = aq;
            node.has_abs = true;
        }
        let rec = XformRec {
            x: ap[0],
            y: ap[1],
            z: ap[2],
            qx: aq[0],
            qy: aq[1],
            qz: aq[2],
            qw: aq[3],
        };
        match target {
            XfTarget::Building(b) => {
                if self.incl.buildings {
                    self.bl.xform.push(b as usize, st, rec);
                }
            }
            XfTarget::Mover(m) => {
                if self.incl.movers {
                    self.mv.xform.push(m as usize, st, rec);
                }
            }
            XfTarget::Drop(d) => {
                if self.incl.drops {
                    self.mv.drop_xform.push(d as usize, st, rec);
                }
            }
        }
        if depth < 16 {
            let children: Vec<u32> = self.mv.children.get(&idx).cloned().unwrap_or_default();
            for c in children {
                self.xf_recompute(c, st, depth + 1);
            }
        }
    }

    /// Absolute position of a tracked transform node (sentry pivots).
    pub(crate) fn xf_abs(&self, idx: u32) -> Option<[f32; 3]> {
        self.mv.xf.get(&idx).filter(|n| n.has_abs).map(|n| n.abs_p)
    }

    pub(crate) fn mv_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let Slots::Mover(s) = &ci.slots else { return };
        let model_index = ent.i0(s.model);
        let model = self.model_name(model_index).to_string();
        if let Some((kind, size)) = super::pickups::pickup_kind(&model) {
            self.pk_create(idx, ent, kind, size, model_index, s.origin, st);
            return;
        }
        if ci.group == Group::Animating {
            return;
        }
        let cart = matches!(s.cls, 4 | 5) && is_cart_model(&model);
        let parent = self
            .slot_of_handle(ent.h(s.parent), Role::Mover)
            .map(|m| m as i32)
            .unwrap_or(-1);
        let row = MoverRow {
            cls: s.cls,
            bsp_model: bsp_model_of(&model),
            model: model_index.clamp(0, 65535) as u16,
            parent,
            parent_h: ent.h(s.parent),
            serial: ent.serial,
            cart,
            train: s.cls == 2,
        };
        let m = self.mv.inst.add(idx, st, row);
        ent.role = Role::Mover;
        ent.slot = m;
        self.xf_register(
            idx,
            ent,
            s.origin,
            s.angles,
            s.ang3,
            s.parent,
            XfTarget::Mover(m),
            st,
        );
        if self.incl.movers {
            self.mv.vis.push(m as usize, st, VisRec { v: 1 });
        }
    }

    pub(crate) fn mv_update(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        st: i64,
        revived: bool,
    ) {
        let Slots::Mover(s) = &ci.slots else { return };
        let mut bits = 0u32;
        for (i, _) in changed {
            bits |= ci.dep.get(*i as usize).copied().unwrap_or(0);
        }
        if bits & M_XF != 0 {
            self.xf_local(idx, ent, s.origin, s.angles, s.ang3, s.parent, st);
        }
        if revived && self.incl.movers {
            self.mv.vis.push(ent.slot as usize, st, VisRec { v: 1 });
        }
    }

    pub(crate) fn mv_leave(&mut self, ent: &Ent, st: i64) {
        if self.incl.movers {
            self.mv.vis.push(ent.slot as usize, st, VisRec { v: 0 });
        }
    }

    pub(crate) fn mv_end(&mut self, idx: u32, ent: &Ent, st: i64) {
        if let Some(r) = self.mv.inst.get_mut(ent.slot) {
            r.deleted = Some(st);
        }
        self.xf_unregister(idx);
    }

    /// Resolves `parent` of movers whose parent entered after them in the same message.
    fn mv_resolve_parents(&mut self) {
        let n = self.mv.inst.len();
        for i in 0..n {
            let Some(r) = self.mv.inst.get(i as u32) else {
                continue;
            };
            if r.data.parent >= 0 {
                continue;
            }
            let Some((pi, ps)) = r.data.parent_h else {
                continue;
            };
            let created = r.created;
            let found = self.mv.inst.rows.iter().enumerate().find(|(_, c)| {
                c.entity as u32 == pi
                    && c.data.serial == ps
                    && c.created <= created
                    && c.deleted.map(|d| d >= created).unwrap_or(true)
            });
            if let Some((j, _)) = found {
                if let Some(r) = self.mv.inst.get_mut(i as u32) {
                    r.data.parent = j as i32;
                }
            }
        }
    }

    /// Mover instances kept by the rule of section 2.5.7.
    #[inline(never)]
    pub(crate) fn mv_order(&mut self) -> InstOrder {
        self.mv_resolve_parents();
        let n = self.mv.inst.len();
        let mut keep = vec![false; n];
        for (i, k) in keep.iter_mut().enumerate() {
            let Some(r) = self.mv.inst.get(i as u32) else {
                continue;
            };
            *k = r.data.bsp_model >= 0
                || self.mv.xform.key_len(i) > 1
                || self.mv.vis.key_len(i) > 1
                || r.data.cart;
        }
        // parents of kept instances
        loop {
            let mut changed = false;
            for i in 0..n {
                if !keep.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let parent = self
                    .mv
                    .inst
                    .get(i as u32)
                    .map(|r| r.data.parent)
                    .unwrap_or(-1);
                if parent >= 0 {
                    if let Some(k) = keep.get_mut(parent as usize) {
                        if !*k {
                            *k = true;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        self.mv
            .inst
            .order_filtered(self.ticks, |i| keep.get(i).copied().unwrap_or(false))
    }

    #[inline(never)]
    pub(crate) fn movers_out(&mut self, ord: &Orders) -> XNode {
        let o = &ord.movers;
        let order = o.order.clone();
        let n = o.count();
        let t = &self.mv.inst;
        let base = t.base_fields(o, self.ticks);
        let cls = t.column(o, |r| r.data.cls);
        let bsp = t.column(o, |r| r.data.bsp_model);
        let model = t.column(o, |r| r.data.model);
        let parent = t.column(o, |r| o.map_i32(r.data.parent));
        let xform = std::mem::replace(&mut self.mv.xform, TrackBuilder::new(1))
            .into_fields(n, Some(&order));
        let vis =
            std::mem::replace(&mut self.mv.vis, TrackBuilder::new(1)).into_fields(n, Some(&order));
        ObjB::new()
            .extend(base)
            .put("cls", cls)
            .put("bspModel", bsp)
            .put("model", model)
            .put("parent", parent)
            .put("xform", XNode::Obj(xform))
            .put("vis", XNode::Obj(vis))
            .build()
    }

    /// `objectives.cart`: mover instance indices of cart props and of `CFuncTrackTrain`.
    pub(crate) fn cart_out(&self, ord: &Orders) -> XNode {
        let mut movers = Vec::new();
        let mut trains = Vec::new();
        if self.incl.movers {
            for (k, old) in ord.movers.order.iter().enumerate() {
                if let Some(r) = self.mv.inst.get(*old as u32) {
                    if r.data.cart {
                        movers.push(k as i32);
                    }
                    if r.data.train {
                        trains.push(k as i32);
                    }
                }
            }
        }
        ObjB::new()
            .put("movers", XNode::I32(movers))
            .put("trains", XNode::I32(trains))
            .build()
    }

    pub(crate) fn drop_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let Slots::Drop(s) = &ci.slots else { return };
        let model_index = ent.i0(s.model);
        let model = self.model_name(model_index).to_string();
        let kind = if s.weapon {
            2
        } else if model.contains("buildables/gibs/") {
            1
        } else {
            0
        };
        let row = DropRow {
            kind,
            model: model_index.clamp(0, 65535) as u16,
            def: if s.weapon {
                def_u16(ent.i(s.def))
            } else {
                65535
            },
        };
        let d = self.mv.drops.add(idx, st, row);
        ent.role = Role::Drop;
        ent.slot = d;
        let ang3 = if s.weapon { None } else { Some(s.ang3) };
        self.xf_register(
            idx,
            ent,
            s.origin,
            s.angles,
            ang3,
            s.parent,
            XfTarget::Drop(d),
            st,
        );
    }

    pub(crate) fn drop_update(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        st: i64,
    ) {
        let Slots::Drop(s) = &ci.slots else { return };
        let mut bits = 0u32;
        for (i, _) in changed {
            bits |= ci.dep.get(*i as usize).copied().unwrap_or(0);
        }
        if bits & M_XF != 0 {
            let ang3 = if s.weapon { None } else { Some(s.ang3) };
            self.xf_local(idx, ent, s.origin, s.angles, ang3, s.parent, st);
        }
    }

    pub(crate) fn drop_end(&mut self, idx: u32, ent: &Ent, st: i64) {
        if let Some(r) = self.mv.drops.get_mut(ent.slot) {
            r.deleted = Some(st);
        }
        self.xf_unregister(idx);
    }

    #[inline(never)]
    pub(crate) fn drops_out(&mut self) -> XNode {
        let o = self.mv.drops.order(self.ticks);
        let order = o.order.clone();
        let n = o.count();
        let t = &self.mv.drops;
        let base = t.base_fields(&o, self.ticks);
        let kind = t.column(&o, |r| r.data.kind);
        let model = t.column(&o, |r| r.data.model);
        let def = t.column(&o, |r| r.data.def);
        let xform = std::mem::replace(&mut self.mv.drop_xform, TrackBuilder::new(1))
            .into_fields(n, Some(&order));
        ObjB::new()
            .extend(base)
            .put("kind", kind)
            .put("model", model)
            .put("def", def)
            .put("xform", XNode::Obj(xform))
            .build()
    }
}

#[inline(never)]
pub fn movers_empty() -> XNode {
    ObjB::new()
        .extend(empty_instances())
        .put("cls", XNode::U8(Vec::new()))
        .put("bspModel", XNode::I16(Vec::new()))
        .put("model", XNode::U16(Vec::new()))
        .put("parent", XNode::I32(Vec::new()))
        .put("xform", XNode::Obj(empty_track::<XformRec>(0)))
        .put("vis", XNode::Obj(empty_track::<VisRec>(0)))
        .build()
}

#[inline(never)]
pub fn drops_empty() -> XNode {
    ObjB::new()
        .extend(empty_instances())
        .put("kind", XNode::U8(Vec::new()))
        .put("model", XNode::U16(Vec::new()))
        .put("def", XNode::U16(Vec::new()))
        .put("xform", XNode::Obj(empty_track::<XformRec>(0)))
        .build()
}
