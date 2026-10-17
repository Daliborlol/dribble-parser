//! Output tree of the extras parser.
//!
//! `XNode` mirrors the JS object of contract section 2.5 (`src/highlander/extras/types.ts`): objects,
//! typed arrays, numbers, strings and the JSON event list. `js.rs` converts it to JS group by group;
//! the native runner walks it for statistics.

use serde::Serialize;

/// One node of the output tree. Typed-array variants hold little-endian data exactly as the JS side
/// receives it.
#[derive(Debug, Clone)]
pub enum XNode {
    Obj(Vec<(String, XNode)>),
    Arr(Vec<XNode>),
    U8(Vec<u8>),
    I8(Vec<i8>),
    U16(Vec<u16>),
    I16(Vec<i16>),
    U32(Vec<u32>),
    I32(Vec<i32>),
    F32(Vec<f32>),
    Num(f64),
    Bool(bool),
    Str(String),
    Strs(Vec<String>),
    Events(Vec<HlEvent>),
}

impl XNode {
    pub fn get(&self, key: &str) -> Option<&XNode> {
        match self {
            XNode::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Dotted path lookup, for example `"cond.w0"`.
    pub fn path(&self, path: &str) -> Option<&XNode> {
        let mut node = self;
        for part in path.split('.') {
            node = node.get(part)?;
        }
        Some(node)
    }

    /// JS constructor name of a typed-array node, or the JS type of other nodes.
    pub fn type_name(&self) -> &'static str {
        match self {
            XNode::Obj(_) => "object",
            XNode::Arr(_) => "array",
            XNode::U8(_) => "Uint8Array",
            XNode::I8(_) => "Int8Array",
            XNode::U16(_) => "Uint16Array",
            XNode::I16(_) => "Int16Array",
            XNode::U32(_) => "Uint32Array",
            XNode::I32(_) => "Int32Array",
            XNode::F32(_) => "Float32Array",
            XNode::Num(_) => "number",
            XNode::Bool(_) => "boolean",
            XNode::Str(_) => "string",
            XNode::Strs(_) => "string[]",
            XNode::Events(_) => "events",
        }
    }

    /// Number of elements of a typed array (0 for other nodes).
    pub fn len(&self) -> usize {
        match self {
            XNode::U8(v) => v.len(),
            XNode::I8(v) => v.len(),
            XNode::U16(v) => v.len(),
            XNode::I16(v) => v.len(),
            XNode::U32(v) => v.len(),
            XNode::I32(v) => v.len(),
            XNode::F32(v) => v.len(),
            XNode::Arr(v) => v.len(),
            XNode::Strs(v) => v.len(),
            XNode::Events(v) => v.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes held in typed arrays below this node.
    pub fn bytes(&self) -> usize {
        match self {
            XNode::Obj(fields) => fields.iter().map(|(_, v)| v.bytes()).sum(),
            XNode::Arr(items) => items.iter().map(|v| v.bytes()).sum(),
            XNode::U8(v) => v.len(),
            XNode::I8(v) => v.len(),
            XNode::U16(v) => v.len() * 2,
            XNode::I16(v) => v.len() * 2,
            XNode::U32(v) => v.len() * 4,
            XNode::I32(v) => v.len() * 4,
            XNode::F32(v) => v.len() * 4,
            _ => 0,
        }
    }

    /// Element `i` of a numeric typed array as f64.
    pub fn num_at(&self, i: usize) -> Option<f64> {
        match self {
            XNode::U8(v) => v.get(i).map(|x| *x as f64),
            XNode::I8(v) => v.get(i).map(|x| *x as f64),
            XNode::U16(v) => v.get(i).map(|x| *x as f64),
            XNode::I16(v) => v.get(i).map(|x| *x as f64),
            XNode::U32(v) => v.get(i).map(|x| *x as f64),
            XNode::I32(v) => v.get(i).map(|x| *x as f64),
            XNode::F32(v) => v.get(i).map(|x| *x as f64),
            _ => None,
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            XNode::Num(n) => Some(*n),
            XNode::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn as_events(&self) -> Option<&[HlEvent]> {
        match self {
            XNode::Events(v) => Some(v.as_slice()),
            _ => None,
        }
    }
}

/// Builds an `XNode::Obj` with string keys.
#[derive(Default)]
pub struct ObjB(pub Vec<(String, XNode)>);

impl ObjB {
    pub fn new() -> Self {
        ObjB(Vec::new())
    }
    pub fn put(mut self, key: &str, value: XNode) -> Self {
        self.0.push((key.to_string(), value));
        self
    }
    pub fn add(&mut self, key: &str, value: XNode) {
        self.0.push((key.to_string(), value));
    }
    pub fn extend(mut self, fields: Vec<(String, XNode)>) -> Self {
        self.0.extend(fields);
        self
    }
    pub fn build(self) -> XNode {
        XNode::Obj(self.0)
    }
}

/// Element types that become a typed array.
pub trait ColType: Copy {
    fn col(v: Vec<Self>) -> XNode;
}

impl ColType for u8 {
    fn col(v: Vec<Self>) -> XNode {
        XNode::U8(v)
    }
}
impl ColType for i8 {
    fn col(v: Vec<Self>) -> XNode {
        XNode::I8(v)
    }
}
impl ColType for u16 {
    fn col(v: Vec<Self>) -> XNode {
        XNode::U16(v)
    }
}
impl ColType for i16 {
    fn col(v: Vec<Self>) -> XNode {
        XNode::I16(v)
    }
}
impl ColType for u32 {
    fn col(v: Vec<Self>) -> XNode {
        XNode::U32(v)
    }
}
impl ColType for i32 {
    fn col(v: Vec<Self>) -> XNode {
        XNode::I32(v)
    }
}
impl ColType for f32 {
    fn col(v: Vec<Self>) -> XNode {
        XNode::F32(v)
    }
}

/// JSON events (contract section 2.5 `HlEvent`). Serialized with `serde_wasm_bindgen` as plain
/// objects tagged by `t`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t", rename_all_fields = "camelCase")]
pub enum HlEvent {
    #[serde(rename = "kill")]
    Kill {
        tick: u32,
        victim: u32,
        attacker: u32,
        assister: u32,
        inflictor: u32,
        weapon: String,
        weapon_log: String,
        weapon_def: u32,
        weapon_id: u32,
        death_flags: u32,
        custom_kill: u32,
        crit_type: u32,
        feign: bool,
    },
    #[serde(rename = "uberDeployed")]
    UberDeployed { tick: u32, medic: u32, target: u32 },
    #[serde(rename = "medicDeath")]
    MedicDeath {
        tick: u32,
        medic: u32,
        attacker: u32,
        healing: u32,
        charged: bool,
    },
    #[serde(rename = "deflect")]
    Deflect {
        tick: u32,
        pyro: u32,
        owner: u32,
        weapon_id: u32,
        entity: u32,
        projectile: i32,
    },
    #[serde(rename = "buildStart")]
    BuildStart {
        tick: u32,
        building: i32,
        builder: u32,
        #[serde(rename = "type")]
        kind: u32,
        mode: u32,
    },
    #[serde(rename = "buildDone")]
    BuildDone {
        tick: u32,
        building: i32,
        builder: u32,
        #[serde(rename = "type")]
        kind: u32,
    },
    #[serde(rename = "upgrade")]
    Upgrade {
        tick: u32,
        building: i32,
        builder: u32,
        level: u32,
    },
    #[serde(rename = "carry")]
    Carry {
        tick: u32,
        building: i32,
        builder: u32,
    },
    #[serde(rename = "redeploy")]
    Redeploy {
        tick: u32,
        building: i32,
        builder: u32,
    },
    #[serde(rename = "destroyed")]
    Destroyed {
        tick: u32,
        building: i32,
        owner: u32,
        attacker: u32,
        assister: u32,
        weapon: String,
        #[serde(rename = "type")]
        kind: u32,
        was_building: bool,
    },
    #[serde(rename = "detonated")]
    Detonated {
        tick: u32,
        building: i32,
        owner: u32,
        #[serde(rename = "type")]
        kind: u32,
    },
    #[serde(rename = "sapped")]
    Sapped {
        tick: u32,
        building: i32,
        spy: u32,
        owner: u32,
        #[serde(rename = "type")]
        kind: u32,
    },
    #[serde(rename = "teleported")]
    Teleported {
        tick: u32,
        player: u32,
        builder: u32,
        dist: f32,
    },
    #[serde(rename = "capStart")]
    CapStart {
        tick: u32,
        cp: u32,
        cp_name: String,
        team: u32,
        cap_team: u32,
        cappers: Vec<u32>,
        cap_time: f32,
    },
    #[serde(rename = "captured")]
    Captured {
        tick: u32,
        cp: u32,
        cp_name: String,
        team: u32,
        cappers: Vec<u32>,
    },
    #[serde(rename = "capBlocked")]
    CapBlocked {
        tick: u32,
        cp: u32,
        cp_name: String,
        blocker: u32,
        victim: u32,
    },
    #[serde(rename = "capBroken")]
    CapBroken {
        tick: u32,
        cp: u32,
        cp_name: String,
        time_remaining: f32,
    },
    #[serde(rename = "cpLocked")]
    CpLocked { tick: u32, cp: u32 },
    #[serde(rename = "cpUnlocked")]
    CpUnlocked { tick: u32, cp: u32 },
    #[serde(rename = "roundStart")]
    RoundStart { tick: u32, full_reset: bool },
    #[serde(rename = "roundActive")]
    RoundActive { tick: u32 },
    #[serde(rename = "setupFinished")]
    SetupFinished { tick: u32 },
    #[serde(rename = "roundWin")]
    RoundWin {
        tick: u32,
        team: u32,
        win_reason: u32,
        full_round: bool,
        round_time: f32,
        sudden_death: bool,
    },
    #[serde(rename = "overtimeStart")]
    OvertimeStart { tick: u32 },
    #[serde(rename = "overtimeEnd")]
    OvertimeEnd { tick: u32 },
    #[serde(rename = "timeAdded")]
    TimeAdded { tick: u32, timer: i32, seconds: u32 },
    #[serde(rename = "gameOver")]
    GameOver { tick: u32, reason: String },
    #[serde(rename = "banner")]
    Banner {
        tick: u32,
        owner: u32,
        buff_type: u32,
    },
    #[serde(rename = "pickup")]
    Pickup {
        tick: u32,
        player: u32,
        item: String,
    },
    #[serde(rename = "spawn")]
    Spawn {
        tick: u32,
        player: u32,
        cls: u32,
        team: u32,
    },
    #[serde(rename = "classChange")]
    ClassChange { tick: u32, player: u32, cls: u32 },
    #[serde(rename = "teamChange")]
    TeamChange {
        tick: u32,
        player: u32,
        team: u32,
        old_team: u32,
        disconnect: bool,
    },
    #[serde(rename = "disconnect")]
    Disconnect {
        tick: u32,
        player: u32,
        reason: String,
    },
}

impl HlEvent {
    /// The `t` tag.
    pub fn name(&self) -> &'static str {
        match self {
            HlEvent::Kill { .. } => "kill",
            HlEvent::UberDeployed { .. } => "uberDeployed",
            HlEvent::MedicDeath { .. } => "medicDeath",
            HlEvent::Deflect { .. } => "deflect",
            HlEvent::BuildStart { .. } => "buildStart",
            HlEvent::BuildDone { .. } => "buildDone",
            HlEvent::Upgrade { .. } => "upgrade",
            HlEvent::Carry { .. } => "carry",
            HlEvent::Redeploy { .. } => "redeploy",
            HlEvent::Destroyed { .. } => "destroyed",
            HlEvent::Detonated { .. } => "detonated",
            HlEvent::Sapped { .. } => "sapped",
            HlEvent::Teleported { .. } => "teleported",
            HlEvent::CapStart { .. } => "capStart",
            HlEvent::Captured { .. } => "captured",
            HlEvent::CapBlocked { .. } => "capBlocked",
            HlEvent::CapBroken { .. } => "capBroken",
            HlEvent::CpLocked { .. } => "cpLocked",
            HlEvent::CpUnlocked { .. } => "cpUnlocked",
            HlEvent::RoundStart { .. } => "roundStart",
            HlEvent::RoundActive { .. } => "roundActive",
            HlEvent::SetupFinished { .. } => "setupFinished",
            HlEvent::RoundWin { .. } => "roundWin",
            HlEvent::OvertimeStart { .. } => "overtimeStart",
            HlEvent::OvertimeEnd { .. } => "overtimeEnd",
            HlEvent::TimeAdded { .. } => "timeAdded",
            HlEvent::GameOver { .. } => "gameOver",
            HlEvent::Banner { .. } => "banner",
            HlEvent::Pickup { .. } => "pickup",
            HlEvent::Spawn { .. } => "spawn",
            HlEvent::ClassChange { .. } => "classChange",
            HlEvent::TeamChange { .. } => "teamChange",
            HlEvent::Disconnect { .. } => "disconnect",
        }
    }

    pub fn tick(&self) -> u32 {
        match self {
            HlEvent::Kill { tick, .. }
            | HlEvent::UberDeployed { tick, .. }
            | HlEvent::MedicDeath { tick, .. }
            | HlEvent::Deflect { tick, .. }
            | HlEvent::BuildStart { tick, .. }
            | HlEvent::BuildDone { tick, .. }
            | HlEvent::Upgrade { tick, .. }
            | HlEvent::Carry { tick, .. }
            | HlEvent::Redeploy { tick, .. }
            | HlEvent::Destroyed { tick, .. }
            | HlEvent::Detonated { tick, .. }
            | HlEvent::Sapped { tick, .. }
            | HlEvent::Teleported { tick, .. }
            | HlEvent::CapStart { tick, .. }
            | HlEvent::Captured { tick, .. }
            | HlEvent::CapBlocked { tick, .. }
            | HlEvent::CapBroken { tick, .. }
            | HlEvent::CpLocked { tick, .. }
            | HlEvent::CpUnlocked { tick, .. }
            | HlEvent::RoundStart { tick, .. }
            | HlEvent::RoundActive { tick }
            | HlEvent::SetupFinished { tick }
            | HlEvent::RoundWin { tick, .. }
            | HlEvent::OvertimeStart { tick }
            | HlEvent::OvertimeEnd { tick }
            | HlEvent::TimeAdded { tick, .. }
            | HlEvent::GameOver { tick, .. }
            | HlEvent::Banner { tick, .. }
            | HlEvent::Pickup { tick, .. }
            | HlEvent::Spawn { tick, .. }
            | HlEvent::ClassChange { tick, .. }
            | HlEvent::TeamChange { tick, .. }
            | HlEvent::Disconnect { tick, .. } => *tick,
        }
    }

    pub fn set_tick(&mut self, t: u32) {
        match self {
            HlEvent::Kill { tick, .. }
            | HlEvent::UberDeployed { tick, .. }
            | HlEvent::MedicDeath { tick, .. }
            | HlEvent::Deflect { tick, .. }
            | HlEvent::BuildStart { tick, .. }
            | HlEvent::BuildDone { tick, .. }
            | HlEvent::Upgrade { tick, .. }
            | HlEvent::Carry { tick, .. }
            | HlEvent::Redeploy { tick, .. }
            | HlEvent::Destroyed { tick, .. }
            | HlEvent::Detonated { tick, .. }
            | HlEvent::Sapped { tick, .. }
            | HlEvent::Teleported { tick, .. }
            | HlEvent::CapStart { tick, .. }
            | HlEvent::Captured { tick, .. }
            | HlEvent::CapBlocked { tick, .. }
            | HlEvent::CapBroken { tick, .. }
            | HlEvent::CpLocked { tick, .. }
            | HlEvent::CpUnlocked { tick, .. }
            | HlEvent::RoundStart { tick, .. }
            | HlEvent::RoundActive { tick }
            | HlEvent::SetupFinished { tick }
            | HlEvent::RoundWin { tick, .. }
            | HlEvent::OvertimeStart { tick }
            | HlEvent::OvertimeEnd { tick }
            | HlEvent::TimeAdded { tick, .. }
            | HlEvent::GameOver { tick, .. }
            | HlEvent::Banner { tick, .. }
            | HlEvent::Pickup { tick, .. }
            | HlEvent::Spawn { tick, .. }
            | HlEvent::ClassChange { tick, .. }
            | HlEvent::TeamChange { tick, .. }
            | HlEvent::Disconnect { tick, .. } => *tick = t,
        }
    }
}
