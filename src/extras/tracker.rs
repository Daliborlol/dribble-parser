//! Generic tracked-entity layer: per-entity prop storage keyed by flattened index, Enter (own props,
//! then the instance-or-static baseline, then the static baseline, merged by `SendProp.index`),
//! Delta, Leave (dormant), Delete, revival of dormant entities and instance identity.
//! Port of `hl_probe.rs` `on_enter`/`on_delta`/`on_leave`/`on_delete`/`ensure_class` without the
//! statistics.

use std::borrow::Cow;

use fnv::FnvHashMap;

use crate::demo::message::packetentities::PacketEntity;
use crate::demo::sendprop::{SendProp, SendPropIdentifier, SendPropValue};
use crate::ParserState;

pub const MAX_EDICTS: usize = 2048;
pub const NULL_HANDLE: i64 = 0x1F_FFFF;

/// A decoded prop value. Strings are interned; arrays keep a change counter (`A`) and their
/// contents in `Ent::arrays` when the class asked for them.
#[derive(Clone, Copy, Debug, Default)]
pub enum Val {
    #[default]
    None,
    I(i64),
    F(f32),
    V([f32; 3]),
    S(u32),
    A(u32),
}

impl Val {
    pub fn same(&self, o: &Val) -> bool {
        match (self, o) {
            (Val::None, Val::None) => true,
            (Val::I(a), Val::I(b)) => a == b,
            (Val::F(a), Val::F(b)) => a.to_bits() == b.to_bits(),
            (Val::V(a), Val::V(b)) => {
                a[0].to_bits() == b[0].to_bits()
                    && a[1].to_bits() == b[1].to_bits()
                    && a[2].to_bits() == b[2].to_bits()
            }
            (Val::S(a), Val::S(b)) => a == b,
            // arrays always count as changed when sent
            _ => false,
        }
    }
    pub fn int(&self) -> Option<i64> {
        match self {
            Val::I(i) => Some(*i),
            Val::F(f) => Some(*f as i64),
            _ => None,
        }
    }
    pub fn float(&self) -> Option<f32> {
        match self {
            Val::F(f) => Some(*f),
            Val::I(i) => Some(*i as f32),
            _ => None,
        }
    }
    pub fn vec3(&self) -> Option<[f32; 3]> {
        match self {
            Val::V(v) => Some(*v),
            _ => None,
        }
    }
    pub fn is_set(&self) -> bool {
        !matches!(self, Val::None)
    }
}

/// Array contents kept for a few props (`healing_array`, `m_vCPPositions`).
#[derive(Clone, Debug)]
pub enum ArrVal {
    Ints(Vec<i64>),
    Vecs(Vec<[f32; 3]>),
    Other,
}

#[derive(Default)]
pub struct Interner {
    map: FnvHashMap<String, u32>,
    list: Vec<String>,
}

impl Interner {
    pub fn id(&mut self, s: &str) -> u32 {
        if let Some(i) = self.map.get(s) {
            return *i;
        }
        let i = self.list.len() as u32;
        self.list.push(s.to_string());
        self.map.insert(s.to_string(), i);
        i
    }
    pub fn get(&self, i: u32) -> &str {
        self.list.get(i as usize).map(|s| s.as_str()).unwrap_or("")
    }
}

/// EHANDLE helpers (rule 2.3.6): `index = v & 0x7FF`, `serial = v >> 11`, `0x1FFFFF` is null.
pub fn handle_parts(v: i64) -> Option<(u32, u32)> {
    if v < 0 || v == NULL_HANDLE || v == 0x7FF {
        return None;
    }
    let index = (v & 0x7FF) as u32;
    let serial = ((v >> 11) & 0x3FF) as u32;
    Some((index, serial))
}

/// Which output group a server class feeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Player,
    PlayerResource,
    Team,
    Rules,
    ObjRes,
    Timer,
    Watcher,
    Pd,
    ViewModel,
    Weapon,
    Wearable,
    Building,
    Projectile,
    Flame,
    Dot,
    Mover,
    Animating,
    Drop,
    Ragdoll,
}

/// Role of one tracked entity; `slot` in `Ent` is the group's provisional instance index (or the
/// player index for `Player`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    None,
    Player,
    PlayerResource,
    Team,
    Rules,
    ObjRes,
    Timer,
    Watcher,
    Pd,
    ViewModel,
    Weapon,
    Wearable,
    Building,
    Projectile,
    Flame,
    Dot,
    Mover,
    Pickup,
    Drop,
    Ragdoll,
}

pub fn classify(name: &str, has: &dyn Fn(&str, &str) -> bool) -> Option<Group> {
    Some(match name {
        "CTFPlayer" => Group::Player,
        "CTFPlayerResource" => Group::PlayerResource,
        "CTFTeam" => Group::Team,
        "CTFGameRulesProxy" => Group::Rules,
        "CTFObjectiveResource" => Group::ObjRes,
        "CTeamRoundTimer" => Group::Timer,
        "CTeamTrainWatcher" => Group::Watcher,
        "CTFPlayerDestructionLogic" => Group::Pd,
        "CTFViewModel" => Group::ViewModel,
        "CObjectSentrygun"
        | "CObjectDispenser"
        | "CObjectTeleporter"
        | "CObjectSapper"
        | "CObjectCartDispenser" => Group::Building,
        "CTFGrenadePipebombProjectile" | "CTFStunBall" | "CTFBall_Ornament" => Group::Projectile,
        "CTFFlameManager" => Group::Flame,
        "CSniperDot" | "CLaserDot" => Group::Dot,
        "CBaseDoor"
        | "CFuncRotating"
        | "CFuncTrackTrain"
        | "CFuncBrush"
        | "CBaseEntity"
        | "CDynamicProp"
        | "CPhysicsProp"
        | "CPhysicsPropMultiplayer" => Group::Mover,
        "CBaseAnimating" => Group::Animating,
        "CTFAmmoPack" | "CTFDroppedWeapon" => Group::Drop,
        "CTFRagdoll" => Group::Ragdoll,
        _ if name.starts_with("CTFProjectile_") => Group::Projectile,
        _ if name.starts_with("CTFWearable") => Group::Wearable,
        _ if has("DT_BaseCombatWeapon", "m_hOwner") => Group::Weapon,
        _ => return None,
    })
}

/// Per-class layout, built once per class.
pub struct ClassInfo {
    pub name: String,
    pub group: Group,
    pub n: usize,
    first: FnvHashMap<SendPropIdentifier, u32>,
    all: FnvHashMap<SendPropIdentifier, Vec<u32>>,
    /// props to store (None = all)
    pub keep: Option<Vec<bool>>,
    /// group-specific dependency bits per flattened index
    pub dep: Vec<u32>,
    /// flattened indices of array props whose contents are kept
    pub keep_arrays: Vec<u32>,
    /// flattened index -> prop type is an int (for zero values)
    pub slots: super::Slots,
}

impl ClassInfo {
    pub fn build(name: &str, idents: &[SendPropIdentifier]) -> Option<ClassInfo> {
        let mut first: FnvHashMap<SendPropIdentifier, u32> = FnvHashMap::default();
        let mut all: FnvHashMap<SendPropIdentifier, Vec<u32>> = FnvHashMap::default();
        for (i, id) in idents.iter().enumerate() {
            first.entry(*id).or_insert(i as u32);
            all.entry(*id).or_default().push(i as u32);
        }
        all.retain(|_, v| v.len() > 1);
        let has = |t: &str, p: &str| first.contains_key(&SendPropIdentifier::new(t, p));
        let group = classify(name, &has)?;
        Some(ClassInfo {
            name: name.to_string(),
            group,
            n: idents.len(),
            first,
            all,
            keep: None,
            dep: vec![0; idents.len()],
            keep_arrays: Vec::new(),
            slots: super::Slots::None,
        })
    }

    /// Flattened index of `table.prop` (first occurrence).
    pub fn find(&self, table: &str, prop: &str) -> Option<u32> {
        self.first
            .get(&SendPropIdentifier::new(table, prop))
            .copied()
    }

    /// Every flattened index of a repeated identifier (UtlVector elements, rule 2.3.3); a
    /// unique identifier yields one entry.
    pub fn find_all(&self, table: &str, prop: &str) -> Vec<u32> {
        let id = SendPropIdentifier::new(table, prop);
        match self.all.get(&id) {
            Some(v) => v.clone(),
            None => self.first.get(&id).map(|i| vec![*i]).unwrap_or_default(),
        }
    }

    /// Array element `table.NNN` (rule 2.3.4).
    pub fn elem(&self, table: &str, element: usize) -> Option<u32> {
        self.find(table, &format!("{element:03}"))
    }

    pub fn add_dep(&mut self, slot: Option<u32>, bits: u32) {
        if let Some(s) = slot {
            if let Some(d) = self.dep.get_mut(s as usize) {
                *d |= bits;
            }
        }
    }

    pub fn keep_only(&mut self, slots: &[Option<u32>]) {
        let mut keep = vec![false; self.n];
        for s in slots.iter().flatten() {
            if let Some(k) = keep.get_mut(*s as usize) {
                *k = true;
            }
        }
        self.keep = Some(keep);
    }

    #[inline]
    pub fn kept(&self, i: u32) -> bool {
        match &self.keep {
            None => true,
            Some(k) => k.get(i as usize).copied().unwrap_or(false),
        }
    }
}

/// One tracked entity (one instance lifetime while it exists).
pub struct Ent {
    pub class: u16,
    pub serial: u32,
    pub dormant: bool,
    pub props: Vec<Val>,
    pub arrays: Vec<(u32, ArrVal)>,
    pub role: Role,
    pub slot: u32,
    pub created: i64,
    array_seq: u32,
}

impl Ent {
    pub fn new(class: u16, serial: u32, n: usize, created: i64) -> Ent {
        Ent {
            class,
            serial,
            dormant: false,
            props: vec![Val::None; n],
            arrays: Vec::new(),
            role: Role::None,
            slot: u32::MAX,
            created,
            array_seq: 0,
        }
    }

    #[inline]
    pub fn get(&self, slot: Option<u32>) -> Val {
        slot.and_then(|s| self.props.get(s as usize).copied())
            .unwrap_or(Val::None)
    }
    #[inline]
    pub fn i(&self, slot: Option<u32>) -> Option<i64> {
        self.get(slot).int()
    }
    #[inline]
    pub fn i0(&self, slot: Option<u32>) -> i64 {
        self.i(slot).unwrap_or(0)
    }
    #[inline]
    pub fn f(&self, slot: Option<u32>) -> Option<f32> {
        self.get(slot).float()
    }
    #[inline]
    pub fn f0(&self, slot: Option<u32>) -> f32 {
        self.f(slot).unwrap_or(0.0)
    }
    #[inline]
    pub fn v(&self, slot: Option<u32>) -> Option<[f32; 3]> {
        self.get(slot).vec3()
    }
    #[inline]
    pub fn b(&self, slot: Option<u32>) -> bool {
        self.i0(slot) != 0
    }
    /// EHANDLE parts of a handle prop, None when null or absent.
    #[inline]
    pub fn h(&self, slot: Option<u32>) -> Option<(u32, u32)> {
        self.i(slot).and_then(handle_parts)
    }
    pub fn array(&self, slot: Option<u32>) -> Option<&ArrVal> {
        let s = slot?;
        self.arrays.iter().find(|(i, _)| *i == s).map(|(_, a)| a)
    }

    fn store(&mut self, ci: &ClassInfo, p: &SendProp, strings: &mut Interner) -> Option<Val> {
        let idx = p.index;
        if !ci.kept(idx) {
            return None;
        }
        let v = match &p.value {
            SendPropValue::Integer(i) => Val::I(*i),
            SendPropValue::Float(f) => Val::F(*f),
            SendPropValue::Vector(v) => Val::V([v.x, v.y, v.z]),
            SendPropValue::VectorXY(v) => Val::V([v.x, v.y, 0.0]),
            SendPropValue::String(s) => Val::S(strings.id(s)),
            SendPropValue::Array(a) => {
                self.array_seq = self.array_seq.wrapping_add(1);
                if ci.keep_arrays.contains(&idx) {
                    let arr = convert_array(a);
                    match self.arrays.iter_mut().find(|(i, _)| *i == idx) {
                        Some(slot) => slot.1 = arr,
                        None => self.arrays.push((idx, arr)),
                    }
                }
                Val::A(self.array_seq)
            }
        };
        let cell = self.props.get_mut(idx as usize)?;
        let old = *cell;
        if old.same(&v) {
            return None;
        }
        *cell = v;
        Some(old)
    }

    /// Applies delta props; returns `(flat index, old value)` of every changed kept prop, in
    /// flattened order.
    pub fn apply(
        &mut self,
        ci: &ClassInfo,
        props: &[SendProp],
        strings: &mut Interner,
    ) -> Vec<(u32, Val)> {
        let mut changed = Vec::new();
        for p in props {
            if let Some(old) = self.store(ci, p, strings) {
                changed.push((p.index, old));
            }
        }
        changed
    }

    /// Applies an Enter: static baseline, then the instance-or-static baseline, then own props,
    /// later layers winning per flattened index.
    pub fn apply_enter(
        &mut self,
        ci: &ClassInfo,
        e: &PacketEntity,
        ps: &ParserState,
        strings: &mut Interner,
    ) -> Vec<(u32, Val)> {
        let n = ci.n;
        let baseline = e.get_baseline_props(ps);
        let static_owned: Vec<SendProp>;
        let static_bl: &[SendProp] = match &baseline {
            // get_baseline returned the static baseline itself
            Cow::Owned(_) => &[],
            Cow::Borrowed(_) => {
                static_owned = ps
                    .send_tables
                    .get(usize::from(e.server_class))
                    .and_then(|t| ps.get_static_baseline(e.server_class, t).ok())
                    .unwrap_or_default();
                &static_owned
            }
        };
        let mut layer: Vec<Option<&SendProp>> = vec![None; n];
        for p in static_bl
            .iter()
            .chain(baseline.iter())
            .chain(e.props.iter())
        {
            if let Some(cell) = layer.get_mut(p.index as usize) {
                *cell = Some(p);
            }
        }
        let mut changed = Vec::new();
        for p in layer.into_iter().flatten() {
            if let Some(old) = self.store(ci, p, strings) {
                changed.push((p.index, old));
            }
        }
        changed
    }
}

fn convert_array(a: &[SendPropValue]) -> ArrVal {
    if a.iter().all(|v| matches!(v, SendPropValue::Integer(_))) {
        ArrVal::Ints(
            a.iter()
                .map(|v| match v {
                    SendPropValue::Integer(i) => *i,
                    _ => 0,
                })
                .collect(),
        )
    } else if a
        .iter()
        .all(|v| matches!(v, SendPropValue::Vector(_) | SendPropValue::VectorXY(_)))
    {
        ArrVal::Vecs(
            a.iter()
                .map(|v| match v {
                    SendPropValue::Vector(v) => [v.x, v.y, v.z],
                    SendPropValue::VectorXY(v) => [v.x, v.y, 0.0],
                    _ => [0.0; 3],
                })
                .collect(),
        )
    } else {
        ArrVal::Other
    }
}

// --------------------------------------------------------------------------------------------
// math (Appendix B.1, section 2.6.2)
// --------------------------------------------------------------------------------------------

pub type Quat = [f32; 4];

/// Source `AngleQuaternion`, angles in degrees (pitch, yaw, roll).
pub fn angle_quat(a: [f32; 3]) -> Quat {
    let d = std::f64::consts::PI / 180.0;
    let (pitch, yaw, roll) = (a[0] as f64 * d, a[1] as f64 * d, a[2] as f64 * d);
    let (sy, cy) = ((yaw / 2.0).sin(), (yaw / 2.0).cos());
    let (sp, cp) = ((pitch / 2.0).sin(), (pitch / 2.0).cos());
    let (sr, cr) = ((roll / 2.0).sin(), (roll / 2.0).cos());
    [
        (sr * cp * cy - cr * sp * sy) as f32,
        (cr * sp * cy + sr * cp * sy) as f32,
        (cr * cp * sy - sr * sp * cy) as f32,
        (cr * cp * cy + sr * sp * sy) as f32,
    ]
}

/// Hamilton product `a * b` of (x, y, z, w) quaternions.
pub fn quat_mul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// Rotates `v` by unit quaternion `q`.
pub fn quat_rotate(q: Quat, v: [f32; 3]) -> [f32; 3] {
    let [qx, qy, qz, qw] = q;
    // t = 2 * cross(q.xyz, v)
    let tx = 2.0 * (qy * v[2] - qz * v[1]);
    let ty = 2.0 * (qz * v[0] - qx * v[2]);
    let tz = 2.0 * (qx * v[1] - qy * v[0]);
    [
        v[0] + qw * tx + (qy * tz - qz * ty),
        v[1] + qw * ty + (qz * tx - qx * tz),
        v[2] + qw * tz + (qx * ty - qy * tx),
    ]
}
