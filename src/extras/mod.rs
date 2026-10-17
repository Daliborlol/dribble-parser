//! Highlander extras parser (contract section 2). Pure Rust: `parse_extras` returns an
//! `ExtrasOutput` tree that `js.rs` converts to the JS object of `src/highlander/extras/types.ts`.
//!
//! Tick rules (section 1.2): entity state and temp entities belong to the packet's server tick
//! (`serverTick - startTick`), game events to `demoTick + tickBase - startTick`; everything in a
//! signon packet or before the first `NetTick` belongs to tick 0. State before tick 0 folds into
//! tick 0, events before tick 0 are dropped, anything at `tick >= ticks` is dropped.

pub mod derive;
pub mod js;
pub mod out;
pub mod track;
pub mod tracker;

mod buildings;
mod dots;
mod events;
mod flames;
mod movers;
mod objectives;
mod pickups;
mod players;
mod projectiles;
mod tempents;
mod viewmodels;
mod weapons;

use std::collections::BTreeMap;
use std::rc::Rc;

use bitbuffer::BitRead;
use fnv::{FnvHashMap, FnvHashSet};
use serde::{Deserialize, Serialize};

use crate::demo::data::DemoTick;
use crate::demo::header::Header;
use crate::demo::message::packetentities::{PacketEntitiesMessage, PacketEntity, UpdateType};
use crate::demo::message::{Message, MessageType};
use crate::demo::packet::datatable::{ParseSendTable, ServerClass};
use crate::demo::packet::message::MessagePacketMeta;
use crate::demo::packet::stringtable::StringTableEntry;
use crate::demo::packet::Packet;
use crate::demo::parser::gamestateanalyser::GameStateAnalyser;
use crate::demo::parser::handler::DemoHandler;
use crate::demo::parser::{MessageHandler, RawPacketStream};
use crate::{Demo, ParserState};

use out::{ObjB, XNode};
use tracker::{ClassInfo, Ent, Group, Interner, Role, Val, MAX_EDICTS};

/// Options of `parse_demo_extras` (section 2.2).
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExtrasOptions {
    pub start_tick: u32,
    pub tick_base: i64,
    pub ticks: u32,
    #[serde(default)]
    pub interval_per_tick: f32,
    pub player_entity_ids: Vec<u32>,
    #[serde(default)]
    pub groups: Option<Vec<String>>,
    #[serde(default)]
    pub wearable_defs: Option<Vec<u16>>,
}

/// Top-level groups of `ExtrasRaw`, in output order.
pub const GROUPS: [&str; 17] = [
    "meta",
    "strings",
    "alignment",
    "players",
    "weapons",
    "buildings",
    "projectiles",
    "flames",
    "dots",
    "objectives",
    "movers",
    "pickups",
    "ragdolls",
    "drops",
    "tempents",
    "hurts",
    "events",
];

/// GameStateAnalyser samples taken at the alignment ticks (native self-check, rule 2.3.11).
#[derive(Clone, Debug, Default)]
pub struct GsSamples {
    /// per k: per p `(position, alive)`; `None` when the player is unknown to the analyser
    pub samples: Vec<GsRow>,
}

/// One alignment sample of the GameStateAnalyser: per player `(position, alive)`.
pub type GsRow = Vec<Option<([f32; 3], bool)>>;

/// Result of `parse_extras`: the output groups in order plus native-only diagnostics.
pub struct ExtrasOutput {
    pub groups: Vec<(String, XNode)>,
    /// unresolved prop paths and other notes for the native runner (not sent to JS)
    pub diag: Vec<String>,
    /// player names by player index (native runner)
    pub player_names: Vec<String>,
    /// classes each player played (native runner)
    pub player_classes: Vec<Vec<u8>>,
    /// whether alignment sample k received a PacketEntities message within 64 ticks
    pub align_hit: [bool; 16],
    /// native-only diagnostic counters
    pub diag_counts: BTreeMap<String, f64>,
    pub gs: Option<GsSamples>,
}

impl ExtrasOutput {
    pub fn group(&self, name: &str) -> Option<&XNode> {
        self.groups.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    pub fn path(&self, path: &str) -> Option<&XNode> {
        let (head, rest) = match path.split_once('.') {
            Some((h, r)) => (h, Some(r)),
            None => (path, None),
        };
        let g = self.group(head)?;
        match rest {
            Some(r) => g.path(r),
            None => Some(g),
        }
    }
}

/// Parses the extras. `progress` receives 0..100 at most once per percent and 100 at the end.
pub fn parse_extras(
    buffer: &[u8],
    opts: &ExtrasOptions,
    progress: &mut dyn FnMut(u32),
) -> Result<ExtrasOutput, String> {
    run(buffer, opts, progress, false)
}

/// `parse_extras` plus GameStateAnalyser positions at the alignment ticks (native self-check).
pub fn parse_extras_checked(
    buffer: &[u8],
    opts: &ExtrasOptions,
    progress: &mut dyn FnMut(u32),
) -> Result<ExtrasOutput, String> {
    run(buffer, opts, progress, true)
}

fn run(
    buffer: &[u8],
    opts: &ExtrasOptions,
    progress: &mut dyn FnMut(u32),
    with_gs: bool,
) -> Result<ExtrasOutput, String> {
    if opts.ticks == 0 {
        return Err("bad options: ticks is 0".to_string());
    }
    let demo = Demo::new(buffer);
    let mut stream = demo.get_stream();
    let header =
        Header::read(&mut stream).map_err(|_| "Failed to parse demo header".to_string())?;
    let mut handler = DemoHandler::with_analyser(Ex::new(opts, with_gs));
    handler.handle_header(&header);
    let mut packets = RawPacketStream::new(stream);
    let header_ticks = header.ticks.max(1) as f64;
    let mut last_pct: i64 = -1;
    let mut truncated: Option<u32> = None;
    loop {
        let packet = match packets.next(handler.get_parser_state()) {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(_) => {
                truncated = Some(handler.analyser().demo_tick);
                break;
            }
        };
        let is_signon = matches!(packet, Packet::Signon(_));
        let is_message = matches!(packet, Packet::Message(_));
        let demo_tick: u32 = packet.tick().into();
        handler.analyser_mut().in_signon = is_signon;
        if handler.handle_packet(packet).is_err() {
            truncated = Some(demo_tick);
            break;
        }
        // only message packets carry a meaningful demo tick; signon packets report 0
        let pct = if is_message {
            ((100.0 * demo_tick as f64 / header_ticks).round() as i64).clamp(0, 100)
        } else if is_signon {
            0
        } else {
            -1
        };
        if pct > last_pct {
            last_pct = pct;
            progress(pct as u32);
        }
    }
    if last_pct < 100 {
        progress(100);
    }
    let incomplete = packets.incomplete;
    let mut ex = handler.into_output();
    if let Some(t) = truncated {
        ex.warnings.push(format!("truncated at demo tick {t}"));
    }
    if incomplete {
        ex.diag
            .push("packet stream ended early (incomplete demo)".to_string());
    }
    if ex.map.is_empty() {
        ex.map = header.map.clone();
    }
    Ok(ex.finish())
}

// ---------------------------------------------------------------------------------------------
// analyser
// ---------------------------------------------------------------------------------------------

/// Per-class slots, one variant per group.
pub enum Slots {
    None,
    Player(Box<players::PlayerSlots>),
    PlayerResource(Box<players::ResourceSlots>),
    Weapon(Box<weapons::WeaponSlots>),
    Wearable(weapons::WearableSlots),
    ViewModel(viewmodels::VmSlots),
    Building(Box<buildings::BuildingSlots>),
    Projectile(Box<projectiles::ProjSlots>),
    Flame(flames::FlameSlots),
    Dot(dots::DotSlots),
    Rules(Box<objectives::RulesSlots>),
    ObjRes(Box<objectives::ObjResSlots>),
    Team(objectives::TeamSlots),
    Timer(objectives::TimerSlots),
    Watcher(objectives::WatcherSlots),
    Pd(objectives::PdSlots),
    Mover(movers::MoverSlots),
    Drop(movers::DropSlots),
    Ragdoll(Box<pickups::RagdollSlots>),
}

enum ClassSlot {
    Unknown,
    Untracked,
    Tracked(Rc<ClassInfo>),
}

/// Which groups are built (options.groups).
#[derive(Clone, Copy)]
pub struct Incl {
    pub players: bool,
    pub weapons: bool,
    pub buildings: bool,
    pub projectiles: bool,
    pub flames: bool,
    pub dots: bool,
    pub objectives: bool,
    pub movers: bool,
    pub pickups: bool,
    pub ragdolls: bool,
    pub drops: bool,
    pub tempents: bool,
    pub hurts: bool,
    pub events: bool,
}

pub(crate) struct AlignState {
    pub targets: [i64; 16],
    pub ticks: [u32; 16],
    pub done: [bool; 16],
    pub hit: [bool; 16],
    pub pos: Vec<f32>,
}

pub(crate) struct Ex {
    // options
    pub start: i64,
    pub tick_base: i64,
    pub ticks: u32,
    pub opt_ipt: f32,
    pub p_ent: Vec<u32>,
    pub ent_p: Vec<u8>,
    pub wearable_defs: Option<FnvHashSet<u16>>,
    pub incl: Incl,
    pub excluded: Vec<String>,
    // stream
    pub demo_tick: u32,
    pub server_tick: u32,
    pub in_signon: bool,
    pub seen_nettick: bool,
    pub ipt: f32,
    pub map: String,
    last_pe_server: Option<u32>,
    gaps: FnvHashMap<u32, u32>,
    // string tables
    pub models: Vec<String>,
    pub particles: Vec<String>,
    pub effects: Vec<String>,
    pub class_names: Vec<String>,
    pub user_ent: FnvHashMap<u32, u32>,
    pub ent_name: FnvHashMap<u32, String>,
    // tracker
    classes: Vec<ClassSlot>,
    pub ents: Vec<Option<Ent>>,
    pub strings: Interner,
    // groups
    pub pl: players::PlState,
    pub wp: weapons::WpState,
    pub bl: buildings::BlState,
    pub pj: projectiles::PjState,
    pub fl: flames::FlState,
    pub dt: dots::DtState,
    pub ob: objectives::ObState,
    pub mv: movers::MvState,
    pub pk: pickups::PkState,
    pub te: tempents::TeState,
    pub ev: events::EvState,
    // meta
    pub convars: Vec<(u32, String, String)>,
    pub cvar: FnvHashMap<String, String>,
    pub pauses: Vec<(u32, bool)>,
    pub warnings: Vec<String>,
    pub warned: FnvHashSet<String>,
    pub counts: BTreeMap<String, f64>,
    pub diag: Vec<String>,
    /// raw game event types received (native diagnostics)
    pub gev_counts: BTreeMap<String, u32>,
    pub al: AlignState,
    pub gs: Option<GameStateAnalyser>,
    pub gs_samples: Vec<GsRow>,
}

impl Ex {
    fn new(opts: &ExtrasOptions, with_gs: bool) -> Ex {
        let ticks = opts.ticks.max(1);
        let mut ent_p = vec![255u8; MAX_EDICTS];
        for (p, e) in opts.player_entity_ids.iter().enumerate() {
            if *e > 0 && (*e as usize) < MAX_EDICTS && p < 255 {
                if let Some(slot) = ent_p.get_mut(*e as usize) {
                    *slot = p as u8;
                }
            }
        }
        let want = |g: &str| -> bool {
            match &opts.groups {
                None => true,
                Some(list) => list.iter().any(|x| x == g),
            }
        };
        let incl = Incl {
            players: want("players"),
            weapons: want("weapons"),
            buildings: want("buildings"),
            projectiles: want("projectiles"),
            flames: want("flames"),
            dots: want("dots"),
            objectives: want("objectives"),
            movers: want("movers"),
            pickups: want("pickups"),
            ragdolls: want("ragdolls"),
            drops: want("drops"),
            tempents: want("tempents"),
            hurts: want("hurts"),
            events: want("events"),
        };
        let excluded: Vec<String> = GROUPS
            .iter()
            .filter(|g| !matches!(**g, "meta" | "strings" | "alignment"))
            .filter(|g| !want(g))
            .map(|g| g.to_string())
            .collect();
        let mut targets = [0i64; 16];
        for (k, t) in targets.iter_mut().enumerate() {
            *t = ((ticks as f64) * (k as f64 + 0.5) / 16.0).round() as i64;
        }
        let pc = opts.player_entity_ids.len();
        Ex {
            start: opts.start_tick as i64,
            tick_base: opts.tick_base,
            ticks,
            opt_ipt: opts.interval_per_tick,
            p_ent: opts.player_entity_ids.clone(),
            ent_p,
            wearable_defs: opts
                .wearable_defs
                .as_ref()
                .map(|v| v.iter().copied().collect()),
            incl,
            excluded,
            demo_tick: 0,
            server_tick: 0,
            in_signon: false,
            seen_nettick: false,
            ipt: 0.0,
            map: String::new(),
            last_pe_server: None,
            gaps: FnvHashMap::default(),
            models: Vec::new(),
            particles: Vec::new(),
            effects: Vec::new(),
            class_names: Vec::new(),
            user_ent: FnvHashMap::default(),
            ent_name: FnvHashMap::default(),
            classes: Vec::new(),
            ents: (0..MAX_EDICTS).map(|_| None).collect(),
            strings: Interner::default(),
            pl: players::PlState::new(pc, ticks),
            wp: weapons::WpState::new(ticks),
            bl: buildings::BlState::new(ticks),
            pj: projectiles::PjState::new(ticks),
            fl: flames::FlState::new(ticks),
            dt: dots::DtState::new(ticks),
            ob: objectives::ObState::new(ticks),
            mv: movers::MvState::new(ticks),
            pk: pickups::PkState::new(ticks),
            te: tempents::TeState::new(ticks),
            ev: events::EvState::new(ticks),
            convars: Vec::new(),
            cvar: FnvHashMap::default(),
            pauses: Vec::new(),
            warnings: Vec::new(),
            warned: FnvHashSet::default(),
            counts: BTreeMap::new(),
            diag: Vec::new(),
            gev_counts: BTreeMap::new(),
            al: AlignState {
                targets,
                ticks: [0; 16],
                done: [false; 16],
                hit: [false; 16],
                pos: vec![f32::NAN; pc * 16 * 3],
            },
            gs: if with_gs {
                Some(GameStateAnalyser::default())
            } else {
                None
            },
            gs_samples: if with_gs {
                vec![Vec::new(); 16]
            } else {
                Vec::new()
            },
        }
    }

    pub fn pc(&self) -> usize {
        self.p_ent.len()
    }

    /// Cache tick of entity state in the current packet.
    pub fn state_tick(&self) -> i64 {
        if self.in_signon || !self.seen_nettick {
            0
        } else {
            self.server_tick as i64 - self.start
        }
    }

    /// Cache tick of a game event in the current packet.
    pub fn event_tick(&self) -> i64 {
        if self.in_signon || !self.seen_nettick {
            0
        } else {
            self.demo_tick as i64 + self.tick_base - self.start
        }
    }

    pub fn interval(&self) -> f64 {
        let ipt = if self.ipt > 0.0 {
            self.ipt
        } else {
            self.opt_ipt
        };
        if ipt > 0.0 {
            ipt as f64
        } else {
            0.015
        }
    }

    /// Server time (`SendPropTime`) to fractional cache tick.
    pub fn tick_f(&self, time: f32) -> f32 {
        (time as f64 / self.interval() - self.start as f64) as f32
    }

    /// Like `tick_f` but a raw 0 becomes NaN.
    pub fn tick_f_nan(&self, time: f32) -> f32 {
        if time == 0.0 {
            f32::NAN
        } else {
            self.tick_f(time)
        }
    }

    /// Player index of an entity index, 255 when unknown.
    #[inline]
    pub fn p_of_ent(&self, e: u32) -> u8 {
        self.ent_p.get(e as usize).copied().unwrap_or(255)
    }

    /// Player index of an EHANDLE value (resolved by index, rule 2.3.6).
    #[inline]
    pub fn p_of_handle(&self, h: Option<(u32, u32)>) -> u8 {
        match h {
            Some((i, _)) => self.p_of_ent(i),
            None => 255,
        }
    }

    /// The live entity referenced by `(index, serial)`.
    pub fn ent_by_handle(&self, h: Option<(u32, u32)>) -> Option<&Ent> {
        let (i, s) = h?;
        let e = self.ents.get(i as usize)?.as_ref()?;
        if e.serial == s {
            Some(e)
        } else {
            None
        }
    }

    /// Provisional instance slot of the entity a handle references, when it has `role`.
    pub fn slot_of_handle(&self, h: Option<(u32, u32)>, role: Role) -> Option<u32> {
        self.ent_by_handle(h)
            .filter(|e| e.role == role)
            .map(|e| e.slot)
    }

    pub fn class_info(&self, class: u16) -> Option<Rc<ClassInfo>> {
        match self.classes.get(class as usize) {
            Some(ClassSlot::Tracked(ci)) => Some(ci.clone()),
            _ => None,
        }
    }

    pub fn class_name(&self, class: u16) -> &str {
        self.class_names
            .get(class as usize)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub fn model_name(&self, index: i64) -> &str {
        if index < 0 {
            return "";
        }
        self.models
            .get(index as usize)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub fn count(&mut self, key: &str) {
        match self.counts.get_mut(key) {
            Some(v) => *v += 1.0,
            None => {
                self.counts.insert(key.to_string(), 1.0);
            }
        }
    }

    fn ensure_class(&mut self, class: u16, ps: &ParserState) -> Option<Rc<ClassInfo>> {
        let i = class as usize;
        if self.classes.len() <= i {
            self.classes.resize_with(i + 1, || ClassSlot::Unknown);
        }
        match self.classes.get(i) {
            Some(ClassSlot::Tracked(ci)) => return Some(ci.clone()),
            Some(ClassSlot::Untracked) => return None,
            _ => {}
        }
        let name = self.class_name(class).to_string();
        let idents: Vec<_> = ps
            .send_tables
            .get(i)
            .map(|t| t.flattened_props.iter().map(|p| p.identifier).collect())
            .unwrap_or_default();
        let slot = match ClassInfo::build(&name, &idents) {
            Some(mut ci) => {
                self.setup_class(&mut ci);
                ClassSlot::Tracked(Rc::new(ci))
            }
            None => ClassSlot::Untracked,
        };
        let out = match &slot {
            ClassSlot::Tracked(ci) => Some(ci.clone()),
            _ => None,
        };
        if let Some(c) = self.classes.get_mut(i) {
            *c = slot;
        }
        out
    }

    #[inline(never)]
    fn setup_class(&mut self, ci: &mut ClassInfo) {
        let missing = match ci.group {
            Group::Player => players::setup_player(ci),
            Group::PlayerResource => players::setup_resource(ci),
            Group::Weapon => weapons::setup_weapon(ci),
            Group::Wearable => weapons::setup_wearable(ci),
            Group::ViewModel => viewmodels::setup(ci),
            Group::Building => buildings::setup(ci),
            Group::Projectile => projectiles::setup(ci),
            Group::Flame => flames::setup(ci),
            Group::Dot => dots::setup(ci),
            Group::Rules => objectives::setup_rules(ci),
            Group::ObjRes => objectives::setup_objres(ci),
            Group::Team => objectives::setup_team(ci),
            Group::Timer => objectives::setup_timer(ci),
            Group::Watcher => objectives::setup_watcher(ci),
            Group::Pd => objectives::setup_pd(ci),
            Group::Mover | Group::Animating => movers::setup_mover(ci),
            Group::Drop => movers::setup_drop(ci),
            Group::Ragdoll => pickups::setup_ragdoll(ci),
        };
        for m in missing {
            self.diag.push(format!("{}: prop {} not found", ci.name, m));
        }
    }

    // ----------------------------------------------------------------------- entities

    #[inline(never)]
    fn on_packet_entities(&mut self, msg: &PacketEntitiesMessage, ps: &ParserState) {
        let st = self.state_tick();
        if !self.in_signon && self.seen_nettick {
            if let Some(prev) = self.last_pe_server {
                if self.server_tick > prev {
                    *self.gaps.entry(self.server_tick - prev).or_insert(0) += 1;
                }
            }
            self.last_pe_server = Some(self.server_tick);
        }
        for e in &msg.entities {
            let idx = u32::from(e.entity_index);
            match e.update_type {
                UpdateType::Enter => self.ent_enter(e, ps, st),
                UpdateType::Delta => self.ent_delta(e, st),
                UpdateType::Leave => self.ent_leave(idx, st),
                UpdateType::Delete => self.ent_delete(idx, st),
            }
        }
        for r in &msg.removed_entities {
            self.ent_delete(u32::from(*r), st);
        }
        self.flush(st);
        self.after_packet(st);
    }

    #[inline(never)]
    fn ent_enter(&mut self, e: &PacketEntity, ps: &ParserState, st: i64) {
        let idx = u32::from(e.entity_index);
        let class = u16::from(e.server_class);
        let old = self.ents.get_mut(idx as usize).and_then(|x| x.take());
        let revive = matches!(&old, Some(o) if o.serial == e.serial_number && o.class == class);
        let mut ent = match old {
            Some(o) if revive => Some(o),
            Some(o) => {
                self.ent_end(idx, o, st);
                None
            }
            None => None,
        };
        let Some(ci) = self.ensure_class(class, ps) else {
            return;
        };
        let is_new = ent.is_none();
        let mut ent = ent
            .take()
            .unwrap_or_else(|| Ent::new(class, e.serial_number, ci.n, st));
        let changed = ent.apply_enter(&ci, e, ps, &mut self.strings);
        if is_new {
            self.on_create(idx, &mut ent, &ci, &changed, st);
        } else {
            ent.dormant = false;
            self.on_update(idx, &mut ent, &ci, &changed, st, true);
        }
        if let Some(slot) = self.ents.get_mut(idx as usize) {
            *slot = Some(ent);
        }
    }

    #[inline(never)]
    fn ent_delta(&mut self, e: &PacketEntity, st: i64) {
        let idx = u32::from(e.entity_index);
        let Some(mut ent) = self.ents.get_mut(idx as usize).and_then(|x| x.take()) else {
            return;
        };
        if let Some(ci) = self.class_info(ent.class) {
            let changed = ent.apply(&ci, &e.props, &mut self.strings);
            if !changed.is_empty() {
                self.on_update(idx, &mut ent, &ci, &changed, st, false);
            }
        }
        if let Some(slot) = self.ents.get_mut(idx as usize) {
            *slot = Some(ent);
        }
    }

    fn ent_leave(&mut self, idx: u32, st: i64) {
        let Some(mut ent) = self.ents.get_mut(idx as usize).and_then(|x| x.take()) else {
            return;
        };
        if !ent.dormant {
            ent.dormant = true;
            self.on_leave(idx, &mut ent, st);
        }
        if let Some(slot) = self.ents.get_mut(idx as usize) {
            *slot = Some(ent);
        }
    }

    fn ent_delete(&mut self, idx: u32, st: i64) {
        if let Some(ent) = self.ents.get_mut(idx as usize).and_then(|x| x.take()) {
            self.ent_end(idx, ent, st);
        }
    }

    fn on_create(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        st: i64,
    ) {
        let key = format!("inst.{}", ci.name);
        self.count(&key);
        match ci.group {
            Group::Player => self.pl_create(idx, ent, ci, changed, st),
            Group::PlayerResource => self.pr_create(idx, ent, ci, st),
            Group::Weapon => self.wp_create(idx, ent, ci, st),
            Group::Wearable => self.wr_create(idx, ent, ci, st),
            Group::ViewModel => self.vm_create(idx, ent, ci, st),
            Group::Building => self.bl_create(idx, ent, ci, st),
            Group::Projectile => self.pj_create(idx, ent, ci, st),
            Group::Flame => self.fl_create(idx, ent, ci, st),
            Group::Dot => self.dt_create(idx, ent, ci, st),
            Group::Rules => self.rules_create(idx, ent, ci, st),
            Group::ObjRes => self.objres_create(idx, ent, ci, st),
            Group::Team => self.team_update(idx, ent, ci, st),
            Group::Timer => self.timer_create(idx, ent, ci, st),
            Group::Watcher => self.watcher_create(idx, ent, ci, st),
            Group::Pd => self.pd_create(idx, ent, ci, st),
            Group::Mover | Group::Animating => self.mv_create(idx, ent, ci, st),
            Group::Drop => self.drop_create(idx, ent, ci, st),
            Group::Ragdoll => self.rag_create(idx, ent, ci, st),
        }
    }

    fn on_update(
        &mut self,
        idx: u32,
        ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        st: i64,
        revived: bool,
    ) {
        match ent.role {
            Role::Player => self.pl_update(idx, ent, ci, changed, st),
            Role::PlayerResource => self.pr_update(idx, ent, ci, changed, st),
            Role::Weapon => self.wp_update(idx, ent, ci, changed, st),
            Role::Wearable => self.wr_update(idx, ent, ci, changed, st),
            Role::ViewModel => self.vm_update(idx, ent, ci, changed, st),
            Role::Building => self.bl_update(idx, ent, ci, changed, st, revived),
            Role::Projectile => self.pj_update(idx, ent, ci, changed, st),
            Role::Flame => self.fl_update(idx, ent, ci, st),
            Role::Dot => self.dt_update(idx, ent, ci, st),
            Role::Rules => self.rules_update(idx, ent, ci, changed, st),
            Role::ObjRes => self.objres_update(idx, ent, ci, changed, st),
            Role::Team => self.team_update(idx, ent, ci, st),
            Role::Timer => self.timer_update(idx, ent, ci, st),
            Role::Watcher => self.watcher_update(idx, ent, ci, st),
            Role::Pd => self.pd_update(idx, ent, ci, st),
            Role::Mover => self.mv_update(idx, ent, ci, changed, st, revived),
            Role::Pickup => self.pk_update(idx, ent, st, revived),
            Role::Drop => self.drop_update(idx, ent, ci, changed, st),
            Role::Ragdoll | Role::None => {}
        }
    }

    fn on_leave(&mut self, idx: u32, ent: &mut Ent, st: i64) {
        match ent.role {
            Role::ViewModel => self.vm_dirty(ent),
            Role::Building => self.bl_leave(idx, ent, st),
            Role::Projectile => self.pj_leave(ent, st),
            Role::Mover => self.mv_leave(ent, st),
            Role::Pickup => self.pk_leave(ent, st),
            _ => {}
        }
    }

    fn ent_end(&mut self, idx: u32, ent: Ent, st: i64) {
        match ent.role {
            Role::Player => self.pl_end(&ent),
            Role::Weapon => self.wp_end(&ent, st),
            Role::Wearable => self.wr_end(idx),
            Role::ViewModel => self.vm_end(idx, &ent),
            Role::Building => self.bl_end(idx, &ent, st),
            Role::Projectile => self.pj_end(&ent, st),
            Role::Flame => self
                .fl
                .inst
                .get_mut(ent.slot)
                .map(|r| r.deleted = Some(st))
                .unwrap_or(()),
            Role::Dot => self
                .dt
                .inst
                .get_mut(ent.slot)
                .map(|r| r.deleted = Some(st))
                .unwrap_or(()),
            Role::Timer => self.timer_end(&ent, st),
            Role::Watcher => self
                .ob
                .watchers
                .get_mut(ent.slot)
                .map(|r| r.deleted = Some(st))
                .unwrap_or(()),
            Role::Pd => self.pd_end(&ent, st),
            Role::Rules => self.ob.rules_ent = None,
            Role::ObjRes => self.objres_end(ent),
            Role::Mover => self.mv_end(idx, &ent, st),
            Role::Pickup => self
                .pk
                .inst
                .get_mut(ent.slot)
                .map(|r| r.deleted = Some(st))
                .unwrap_or(()),
            Role::Drop => self.drop_end(idx, &ent, st),
            Role::Ragdoll => self
                .pk
                .rag
                .get_mut(ent.slot)
                .map(|r| r.deleted = Some(st))
                .unwrap_or(()),
            Role::PlayerResource => self.pl.res_ent = None,
            Role::Team | Role::None => {}
        }
    }

    /// Recomputes records marked dirty during this message.
    fn flush(&mut self, st: i64) {
        self.pl_flush(st);
        self.bl_flush(st);
        self.xf_flush(st);
        self.ob_flush(st);
    }

    fn after_packet(&mut self, st: i64) {
        self.cp_simulate(st);
        self.pj_after_packet(st);
        self.align_sample(st);
    }

    #[inline(never)]
    fn align_sample(&mut self, st: i64) {
        if st < 0 || self.in_signon {
            return;
        }
        for k in 0..16usize {
            let (Some(done), Some(target)) = (
                self.al.done.get(k).copied(),
                self.al.targets.get(k).copied(),
            ) else {
                continue;
            };
            if done || target > st {
                continue;
            }
            if let Some(d) = self.al.done.get_mut(k) {
                *d = true;
            }
            if st - target > 64 {
                if let Some(t) = self.al.ticks.get_mut(k) {
                    *t = target.max(0) as u32;
                }
                continue;
            }
            if let Some(t) = self.al.ticks.get_mut(k) {
                *t = st as u32;
            }
            if let Some(h) = self.al.hit.get_mut(k) {
                *h = true;
            }
            for p in 0..self.pc() {
                let pos = self.pl_align_pos(p);
                for a in 0..3 {
                    if let Some(cell) = self.al.pos.get_mut((p * 16 + k) * 3 + a) {
                        *cell = pos
                            .map(|v| v.get(a).copied().unwrap_or(f32::NAN))
                            .unwrap_or(f32::NAN);
                    }
                }
            }
            if let Some(gs) = &self.gs {
                let mut row = Vec::with_capacity(self.p_ent.len());
                for e in &self.p_ent {
                    let pl = gs
                        .state
                        .players
                        .iter()
                        .find(|pl| u32::from(pl.entity) == *e && *e != 0);
                    row.push(pl.map(|pl| {
                        let alive = pl.connected
                            && pl.state == crate::demo::data::game_state::PlayerState::Alive
                            && pl.health > 0
                            && !(pl.position.x == 0.0
                                && pl.position.y == 0.0
                                && pl.position.z == 0.0);
                        ([pl.position.x, pl.position.y, pl.position.z], alive)
                    }));
                }
                if let Some(slot) = self.gs_samples.get_mut(k) {
                    *slot = row;
                }
            }
        }
    }

    // ----------------------------------------------------------------------- finish

    #[inline(never)]
    fn finish(mut self) -> ExtrasOutput {
        // alignment samples that never received a packet
        for k in 0..16usize {
            if !self.al.done.get(k).copied().unwrap_or(true) {
                if let (Some(t), Some(target)) = (self.al.ticks.get_mut(k), self.al.targets.get(k))
                {
                    *t = (*target).max(0) as u32;
                }
            }
        }
        self.pj_finish_positions();
        let pc = self.pc();
        let ticks = self.ticks;

        // instance orders (shared by cross references)
        let ord = Orders {
            weapons: self.wp.inst.order(ticks),
            buildings: self.bl.inst.order(ticks),
            projectiles: self.pj.inst.order(ticks),
            timers: self.ob.timers.order(ticks),
            movers: self.mv_order(),
        };
        let mut groups: Vec<(String, XNode)> = Vec::with_capacity(GROUPS.len());

        let players = if self.incl.players {
            self.players_out(&ord)
        } else {
            players::players_empty(pc, &self.p_ent)
        };
        let weapons = if self.incl.weapons {
            self.weapons_out(&ord)
        } else {
            weapons::weapons_empty()
        };
        let buildings = if self.incl.buildings {
            self.buildings_out(&ord)
        } else {
            buildings::buildings_empty()
        };
        let projectiles = if self.incl.projectiles {
            self.projectiles_out(&ord)
        } else {
            projectiles::projectiles_empty()
        };
        let flames = if self.incl.flames {
            self.flames_out()
        } else {
            flames::flames_empty()
        };
        let dots = if self.incl.dots {
            self.dots_out()
        } else {
            dots::dots_empty()
        };
        let objectives = if self.incl.objectives {
            self.objectives_out(&ord)
        } else {
            objectives::objectives_empty()
        };
        let movers = if self.incl.movers {
            self.movers_out(&ord)
        } else {
            movers::movers_empty()
        };
        let pickups = if self.incl.pickups {
            self.pickups_out()
        } else {
            pickups::pickups_empty()
        };
        let ragdolls = if self.incl.ragdolls {
            self.ragdolls_out()
        } else {
            pickups::ragdolls_empty()
        };
        let drops = if self.incl.drops {
            self.drops_out()
        } else {
            movers::drops_empty()
        };
        let tempents = if self.incl.tempents {
            self.tempents_out(&ord)
        } else {
            tempents::tempents_empty()
        };
        let hurts = if self.incl.hurts {
            self.hurts_out()
        } else {
            tempents::hurts_empty()
        };
        let events = if self.incl.events {
            self.events_out(&ord)
        } else {
            XNode::Events(Vec::new())
        };

        let available = self.available(
            &players,
            &[
                ("buildings", &buildings),
                ("projectiles", &projectiles),
                ("flames", &flames),
                ("dots", &dots),
                ("objectives", &objectives),
                ("movers", &movers),
                ("pickups", &pickups),
                ("ragdolls", &ragdolls),
                ("drops", &drops),
                ("tempents", &tempents),
                ("hurts", &hurts),
                ("events", &events),
            ],
        );
        if let XNode::Events(list) = &events {
            for e in list {
                *self.counts.entry(format!("ev.{}", e.name())).or_insert(0.0) += 1.0;
            }
        }

        let meta = self.meta_out(available);
        let strings = ObjB::new()
            .put("models", XNode::Strs(std::mem::take(&mut self.models)))
            .put(
                "classes",
                XNode::Strs(std::mem::take(&mut self.class_names)),
            )
            .put(
                "particles",
                XNode::Strs(std::mem::take(&mut self.particles)),
            )
            .put("effects", XNode::Strs(std::mem::take(&mut self.effects)))
            .build();
        let alignment = ObjB::new()
            .put("ticks", XNode::U32(self.al.ticks.to_vec()))
            .put("pos", XNode::F32(std::mem::take(&mut self.al.pos)))
            .build();

        groups.push(("meta".into(), meta));
        groups.push(("strings".into(), strings));
        groups.push(("alignment".into(), alignment));
        groups.push(("players".into(), players));
        groups.push(("weapons".into(), weapons));
        groups.push(("buildings".into(), buildings));
        groups.push(("projectiles".into(), projectiles));
        groups.push(("flames".into(), flames));
        groups.push(("dots".into(), dots));
        groups.push(("objectives".into(), objectives));
        groups.push(("movers".into(), movers));
        groups.push(("pickups".into(), pickups));
        groups.push(("ragdolls".into(), ragdolls));
        groups.push(("drops".into(), drops));
        groups.push(("tempents".into(), tempents));
        groups.push(("hurts".into(), hurts));
        groups.push(("events".into(), events));

        let player_names = self
            .p_ent
            .iter()
            .map(|e| self.ent_name.get(e).cloned().unwrap_or_default())
            .collect();
        let gs = self.gs.as_ref().map(|_| GsSamples {
            samples: std::mem::take(&mut self.gs_samples),
        });
        let mut diag_counts = BTreeMap::new();
        diag_counts.insert(
            "spyMedigunDefPushes".to_string(),
            self.pl.spy_medigun_defs as f64,
        );
        for (k, v) in &self.gev_counts {
            diag_counts.insert(format!("gameEvent.{k}"), *v as f64);
        }
        ExtrasOutput {
            groups,
            diag: std::mem::take(&mut self.diag),
            player_names,
            player_classes: std::mem::take(&mut self.pl.classes),
            align_hit: self.al.hit,
            diag_counts,
            gs,
        }
    }

    fn snapshot_interval(&self) -> u32 {
        self.gaps
            .iter()
            .max_by_key(|(gap, n)| (**n, std::cmp::Reverse(**gap)))
            .map(|(g, _)| *g)
            .unwrap_or(1)
    }

    #[inline(never)]
    fn available(&self, players: &XNode, groups: &[(&str, &XNode)]) -> XNode {
        let has_entries = |n: &XNode, path: &str| -> bool {
            n.path(path).map(|t| !t.is_empty()).unwrap_or(false)
        };
        let any = |n: &XNode, path: &str, pred: &dyn Fn(f64) -> bool| -> bool {
            match n.path(path) {
                Some(col) => (0..col.len()).any(|i| col.num_at(i).map(pred).unwrap_or(false)),
                None => false,
            }
        };
        let g = |name: &str| groups.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
        let mut a = ObjB::new();
        let pl = players;
        a.add("cond", XNode::Bool(has_entries(pl, "cond.ticks")));
        a.add("cloak", XNode::Bool(has_entries(pl, "cloak.ticks")));
        a.add("disguise", XNode::Bool(has_entries(pl, "disguise.ticks")));
        a.add(
            "weapon",
            XNode::Bool(any(pl, "weapon.def", &|v| v != 65535.0)),
        );
        a.add(
            "wstate",
            XNode::Bool(
                any(pl, "wstate.clip", &|v| v >= 0.0)
                    || any(pl, "wstate.reload", &|v| v != 0.0)
                    || any(pl, "wstate.state", &|v| v != 0.0),
            ),
        );
        a.add(
            "ammo",
            XNode::Bool(
                any(pl, "ammo.primary", &|v| v != 0.0) || any(pl, "ammo.metal", &|v| v != 0.0),
            ),
        );
        a.add(
            "medigun",
            XNode::Bool(any(pl, "medigun.def", &|v| v != 65535.0)),
        );
        a.add(
            "sniper",
            XNode::Bool(any(pl, "sniper.charge", &|v| v != 0.0)),
        );
        a.add("fov", XNode::Bool(has_entries(pl, "fov.ticks")));
        a.add("view", XNode::Bool(any(pl, "view.eyeZ", &|v| v != 0.0)));
        a.add("resource", XNode::Bool(has_entries(pl, "resource.ticks")));
        a.add("misc", XNode::Bool(has_entries(pl, "misc.ticks")));
        a.add(
            "builder",
            XNode::Bool(any(pl, "builder.type", &|v| v != 255.0)),
        );
        a.add(
            "loadout",
            XNode::Bool(any(pl, "loadout.d0", &|v| v != 65535.0)),
        );
        a.add(
            "viewmodels",
            XNode::Bool(has_entries(pl, "vm0.ticks") || has_entries(pl, "vm1.ticks")),
        );
        a.add("fire", XNode::Bool(has_entries(pl, "fire.tick")));
        a.add("anim", XNode::Bool(has_entries(pl, "anim.tick")));
        a.add(
            "scoring",
            XNode::Bool(
                any(pl, "scoring.points", &|v| v != 0.0)
                    || any(pl, "scoring.damage", &|v| v != 0.0)
                    || any(pl, "scoring.kills", &|v| v != 0.0),
            ),
        );
        a.add(
            "itemCharge",
            XNode::Bool(
                any(pl, "itemCharge.s0", &|v| v != 0.0)
                    || any(pl, "itemCharge.s1", &|v| v != 0.0)
                    || any(pl, "itemCharge.s2", &|v| v != 0.0),
            ),
        );
        let count_of = |n: Option<&XNode>| {
            n.and_then(|n| n.get("count"))
                .and_then(|c| c.as_num())
                .unwrap_or(0.0)
                > 0.0
        };
        a.add("buildings", XNode::Bool(count_of(g("buildings"))));
        a.add("projectiles", XNode::Bool(count_of(g("projectiles"))));
        a.add("flames", XNode::Bool(count_of(g("flames"))));
        a.add("dots", XNode::Bool(count_of(g("dots"))));
        let ob = g("objectives");
        let obh = |path: &str| ob.map(|o| has_entries(o, path)).unwrap_or(false);
        a.add("objectiveResource", XNode::Bool(obh("cp.state.ticks")));
        a.add("timers", XNode::Bool(obh("timers.state.ticks")));
        a.add("rules", XNode::Bool(obh("rules.ticks")));
        a.add("teams", XNode::Bool(obh("teams.ticks")));
        a.add("watchers", XNode::Bool(obh("watchers.state.ticks")));
        a.add(
            "carts",
            XNode::Bool(obh("cart.movers") || obh("cart.trains")),
        );
        a.add("pd", XNode::Bool(obh("pd.ticks")));
        a.add("movers", XNode::Bool(count_of(g("movers"))));
        a.add("pickups", XNode::Bool(count_of(g("pickups"))));
        a.add("ragdolls", XNode::Bool(count_of(g("ragdolls"))));
        a.add("drops", XNode::Bool(count_of(g("drops"))));
        let te = g("tempents");
        let teh = |path: &str| te.map(|o| has_entries(o, path)).unwrap_or(false);
        a.add("bullets", XNode::Bool(teh("bullets.tick")));
        a.add("explosions", XNode::Bool(teh("explosions.tick")));
        a.add("tracers", XNode::Bool(teh("tracers.tick")));
        a.add("particles", XNode::Bool(teh("particles.tick")));
        a.add(
            "clientProjectiles",
            XNode::Bool(teh("clientProjectiles.tick")),
        );
        a.add(
            "hurts",
            XNode::Bool(g("hurts").map(|h| has_entries(h, "tick")).unwrap_or(false)),
        );
        a.add(
            "events",
            XNode::Bool(g("events").map(|e| !e.is_empty()).unwrap_or(false)),
        );
        a.build()
    }

    #[inline(never)]
    fn meta_out(&mut self, available: XNode) -> XNode {
        let ipt = if self.ipt > 0.0 {
            self.ipt
        } else {
            self.opt_ipt
        };
        let mut counts = ObjB::new();
        for (k, v) in &self.counts {
            counts.add(k, XNode::Num(*v));
        }
        let convars: Vec<XNode> = self
            .convars
            .iter()
            .map(|(t, k, v)| {
                ObjB::new()
                    .put("tick", XNode::Num(*t as f64))
                    .put("key", XNode::Str(k.clone()))
                    .put("value", XNode::Str(v.clone()))
                    .build()
            })
            .collect();
        let pauses: Vec<XNode> = self
            .pauses
            .iter()
            .map(|(t, p)| {
                ObjB::new()
                    .put("tick", XNode::Num(*t as f64))
                    .put("paused", XNode::Bool(*p))
                    .build()
            })
            .collect();
        ObjB::new()
            .put("schema", XNode::Num(1.0))
            .put("startTick", XNode::Num(self.start as f64))
            .put("tickBase", XNode::Num(self.tick_base as f64))
            .put("ticks", XNode::Num(self.ticks as f64))
            .put("intervalPerTick", XNode::Num(ipt as f64))
            .put(
                "snapshotInterval",
                XNode::Num(self.snapshot_interval() as f64),
            )
            .put("playerCount", XNode::Num(self.pc() as f64))
            .put("map", XNode::Str(self.map.clone()))
            .put("parseMs", XNode::Num(0.0))
            .put("available", available)
            .put("counts", counts.build())
            .put("warnings", XNode::Strs(self.warnings.clone()))
            .put("excluded", XNode::Strs(self.excluded.clone()))
            .put("convars", XNode::Arr(convars))
            .put("pauses", XNode::Arr(pauses))
            .build()
    }
}

/// Final instance orders of the groups other groups refer to.
pub(crate) struct Orders {
    pub weapons: track::InstOrder,
    pub buildings: track::InstOrder,
    pub projectiles: track::InstOrder,
    pub timers: track::InstOrder,
    pub movers: track::InstOrder,
}

impl MessageHandler for Ex {
    type Output = Ex;

    fn does_handle(message_type: MessageType) -> bool {
        matches!(
            message_type,
            MessageType::PacketEntities
                | MessageType::TempEntities
                | MessageType::GameEvent
                | MessageType::NetTick
                | MessageType::SetConVar
                | MessageType::SetPause
                | MessageType::ServerInfo
        )
    }

    fn handle_message(&mut self, message: &Message, tick: DemoTick, ps: &ParserState) {
        if let Some(gs) = &mut self.gs {
            if GameStateAnalyser::does_handle(message.get_message_type()) {
                gs.handle_message(message, tick, ps);
            }
        }
        match message {
            Message::NetTick(m) => {
                self.server_tick = u32::from(m.tick);
                if !self.in_signon {
                    self.seen_nettick = true;
                }
            }
            Message::ServerInfo(m) => {
                self.ipt = m.interval_per_tick;
                self.map = m.map.clone();
            }
            // convars and pauses are state: values set before tick 0 fold into tick 0
            Message::SetConVar(m) => {
                let t = self.event_tick();
                for v in &m.vars {
                    self.cvar.insert(v.key.clone(), v.value.clone());
                    if t < self.ticks as i64 {
                        self.convars
                            .push((t.max(0) as u32, v.key.clone(), v.value.clone()));
                    }
                }
            }
            Message::SetPause(m) => {
                let t = self.event_tick();
                if t < self.ticks as i64 {
                    self.pauses.push((t.max(0) as u32, m.pause));
                }
            }
            Message::PacketEntities(m) => self.on_packet_entities(m, ps),
            Message::TempEntities(m) => self.on_temp_entities(m, ps),
            Message::GameEvent(m) => self.on_game_event(m),
            _ => {}
        }
    }

    fn handle_string_entry(
        &mut self,
        table: &str,
        index: usize,
        entry: &StringTableEntry,
        ps: &ParserState,
    ) {
        if let Some(gs) = &mut self.gs {
            gs.handle_string_entry(table, index, entry, ps);
        }
        let set = |list: &mut Vec<String>, entry: &StringTableEntry| {
            if list.len() <= index {
                list.resize(index + 1, String::new());
            }
            if let (Some(slot), Some(text)) = (list.get_mut(index), entry.text.as_ref()) {
                *slot = text.to_string();
            }
        };
        match table {
            "modelprecache" => set(&mut self.models, entry),
            "ParticleEffectNames" => set(&mut self.particles, entry),
            "EffectDispatch" => set(&mut self.effects, entry),
            "userinfo" => {
                let data = entry.extra_data.as_ref().map(|d| d.data.clone());
                if let Ok(Some(info)) = crate::demo::data::UserInfo::parse_from_string_table(
                    index as u16,
                    entry.text.as_ref().map(|s| s.as_ref()),
                    data,
                ) {
                    let uid = u32::from(info.player_info.user_id);
                    let eid = u32::from(info.entity_id);
                    self.user_ent.insert(uid, eid);
                    self.ent_name.insert(eid, info.player_info.name.clone());
                }
            }
            _ => {}
        }
    }

    fn handle_data_tables(
        &mut self,
        tables: &[ParseSendTable],
        server_classes: &[ServerClass],
        ps: &ParserState,
    ) {
        if let Some(gs) = &mut self.gs {
            gs.handle_data_tables(tables, server_classes, ps);
        }
        self.class_names = server_classes.iter().map(|c| c.name.to_string()).collect();
        self.classes = (0..server_classes.len())
            .map(|_| ClassSlot::Unknown)
            .collect();
        self.te.reset_classes(server_classes.len());
    }

    fn handle_packet_meta(&mut self, tick: DemoTick, meta: &MessagePacketMeta, ps: &ParserState) {
        if let Some(gs) = &mut self.gs {
            gs.handle_packet_meta(tick, meta, ps);
        }
        self.demo_tick = u32::from(tick);
    }

    fn into_output(self, _state: &ParserState) -> Self::Output {
        self
    }
}

/// Small helpers shared by the group modules.
pub(crate) fn clamp_u8(v: f64) -> u8 {
    if v.is_nan() {
        0
    } else {
        v.round().clamp(0.0, 255.0) as u8
    }
}

pub(crate) fn clamp_u16(v: i64) -> u16 {
    v.clamp(0, 65535) as u16
}

pub(crate) fn to_u8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

pub(crate) fn to_i8(v: i64) -> i8 {
    v.clamp(-128, 127) as i8
}

pub(crate) fn def_u16(v: Option<i64>) -> u16 {
    match v {
        Some(d) if (0..65535).contains(&d) => d as u16,
        _ => 65535,
    }
}
