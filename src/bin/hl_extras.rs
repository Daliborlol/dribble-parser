//! hl_extras: native runner for the Highlander extras parser (contract section 2.7).
//!
//! Usage:
//!   hl_extras <demo.dem> [--json <file>] [--self-check] [--print-options <file>]
//!             [--wearable-defs <file>] [--counts-only] [--groups a,b,...]
//!             [--start-tick N --tick-base N --ticks N --players e0,e1,...]
//!
//! Without explicit base values the runner derives them the way `lib.rs` does (rule 2.3.11).
//! It prints per-group entry counts, typed-array bytes, parse time and peak RSS (VmHWM).

use std::collections::BTreeMap;
use std::time::Instant;
use std::{env, fs, process};

use serde_json::{json, Map, Value};
use tf_demo_parser::extras::derive::derive_options_with;
use tf_demo_parser::extras::out::{HlEvent, XNode};
use tf_demo_parser::extras::{parse_extras, parse_extras_checked, ExtrasOptions, ExtrasOutput};

mod dump {
    //! `--props <Class>`: prints the flattened send props of a server class.
    use std::collections::HashMap;

    use bitbuffer::BitRead;
    use tf_demo_parser::demo::header::Header;
    use tf_demo_parser::demo::message::{Message, MessageType};
    use tf_demo_parser::demo::packet::datatable::{ParseSendTable, ServerClass};
    use tf_demo_parser::demo::parser::handler::DemoHandler;
    use tf_demo_parser::demo::parser::{MessageHandler, RawPacketStream};
    use tf_demo_parser::demo::sendprop::SendPropIdentifier;
    use tf_demo_parser::{Demo, ParserState};

    #[derive(Default)]
    struct Names {
        names: HashMap<SendPropIdentifier, String>,
        classes: Vec<String>,
        done: bool,
        out: Vec<String>,
        want: String,
    }

    impl MessageHandler for Names {
        type Output = Vec<String>;
        fn does_handle(t: MessageType) -> bool {
            matches!(t, MessageType::PacketEntities)
        }
        fn handle_message(
            &mut self,
            _m: &Message,
            _t: tf_demo_parser::demo::data::DemoTick,
            ps: &ParserState,
        ) {
            if self.done {
                return;
            }
            self.done = true;
            if let Some(ci) = self.classes.iter().position(|c| *c == self.want) {
                if let Some(t) = ps.send_tables.get(ci) {
                    for (i, p) in t.flattened_props.iter().enumerate() {
                        let n = self
                            .names
                            .get(&p.identifier)
                            .cloned()
                            .unwrap_or_else(|| "?".into());
                        self.out.push(format!("{i:4} {n}"));
                    }
                }
            }
        }
        fn handle_data_tables(
            &mut self,
            tables: &[ParseSendTable],
            classes: &[ServerClass],
            _ps: &ParserState,
        ) {
            for t in tables {
                for p in &t.props {
                    self.names
                        .insert(p.identifier(), format!("{}.{}", t.name, p.name));
                }
            }
            self.classes = classes.iter().map(|c| c.name.to_string()).collect();
        }
        fn into_output(self, _s: &ParserState) -> Vec<String> {
            self.out
        }
    }

    pub fn props(buffer: &[u8], class: &str) -> Vec<String> {
        let demo = Demo::new(buffer);
        let mut stream = demo.get_stream();
        let Ok(header) = Header::read(&mut stream) else {
            return Vec::new();
        };
        let mut handler = DemoHandler::with_analyser(Names {
            want: class.to_string(),
            ..Names::default()
        });
        handler.handle_header(&header);
        let mut packets = RawPacketStream::new(stream);
        while let Ok(Some(p)) = packets.next(handler.get_parser_state()) {
            if handler.handle_packet(p).is_err() || handler.analyser().done {
                break;
            }
        }
        handler.into_output()
    }
}

// HL flag bits (contract Appendix A)
const HL_NAMES: [&str; 32] = [
    "ZOOMED",
    "AIMING",
    "DISGUISING",
    "DISGUISED",
    "CLOAKED",
    "CLOAK_BLINK",
    "FEIGN",
    "UBER",
    "UBER_FADING",
    "SPAWN_PROTECT",
    "CRIT",
    "CRIT_BONUS",
    "MINICRIT",
    "QUICKFIX_UBER",
    "VACC_UBER",
    "VACC_PASSIVE",
    "OVERHEALED",
    "HEALED",
    "BURNING",
    "BLEEDING",
    "JARATED",
    "MILKED",
    "MARKED",
    "TELEPORTED",
    "ON_TELEPORTER",
    "TAUNTING",
    "BLAST_JUMPING",
    "SPEED_BOOST",
    "BONK",
    "STUNNED",
    "SOLDIER_BUFF",
    "SHIELD_CHARGE",
];
const HL_SOURCES: [&[u32]; 32] = [
    &[1],
    &[0],
    &[2],
    &[3],
    &[4, 64, 66],
    &[9],
    &[13],
    &[5, 52, 57],
    &[8],
    &[51],
    &[11, 33, 34, 35, 37, 39, 40, 44, 56, 105],
    &[38],
    &[16, 19, 78],
    &[28],
    &[58, 59, 60],
    &[61, 62, 63],
    &[23],
    &[21],
    &[22, 124],
    &[25],
    &[24],
    &[27],
    &[30, 48],
    &[6],
    &[10],
    &[7],
    &[81],
    &[32],
    &[14],
    &[15],
    &[16, 26, 29, 45],
    &[17],
];

fn cond_set(w: &[u32; 5], cond: u32) -> bool {
    let word = (cond / 32) as usize;
    w.get(word)
        .map(|x| (x >> (cond % 32)) & 1 == 1)
        .unwrap_or(false)
}

fn derive_flags(w: &[u32; 5]) -> u32 {
    let mut f = 0u32;
    for (bit, conds) in HL_SOURCES.iter().enumerate() {
        if conds.iter().any(|c| cond_set(w, *c)) {
            f |= 1 << bit;
        }
    }
    f
}

fn vm_hwm_kb() -> u64 {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            return rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        }
    }
    0
}

struct Args {
    demo: String,
    json: Option<String>,
    self_check: bool,
    print_options: Option<String>,
    wearable_defs: Option<String>,
    counts_only: bool,
    list: Option<String>,
    dump: Option<String>,
    start_without_world: bool,
    groups: Option<Vec<String>>,
    start_tick: Option<u32>,
    tick_base: Option<i64>,
    ticks: Option<u32>,
    players: Option<Vec<u32>>,
    ipt: Option<f32>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        demo: String::new(),
        json: None,
        self_check: false,
        print_options: None,
        wearable_defs: None,
        counts_only: false,
        list: None,
        dump: None,
        start_without_world: false,
        groups: None,
        start_tick: None,
        tick_base: None,
        ticks: None,
        players: None,
        ipt: None,
    };
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--json" => a.json = Some(val("--json")?),
            "--self-check" => a.self_check = true,
            "--print-options" => a.print_options = Some(val("--print-options")?),
            "--wearable-defs" => a.wearable_defs = Some(val("--wearable-defs")?),
            "--counts-only" => a.counts_only = true,
            "--list" => a.list = Some(val("--list")?),
            "--dump" => a.dump = Some(val("--dump")?),
            "--start-without-world" => a.start_without_world = true,
            "--props" => {
                let class = val("--props")?;
                let file = fs::read(&a.demo)
                    .map_err(|e| format!("cannot read demo (give it first): {e}"))?;
                for l in dump::props(&file, &class) {
                    println!("{l}");
                }
                process::exit(0);
            }
            "--groups" => {
                a.groups = Some(
                    val("--groups")?
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect(),
                )
            }
            "--start-tick" => {
                a.start_tick = Some(
                    val("--start-tick")?
                        .parse()
                        .map_err(|_| "bad --start-tick")?,
                )
            }
            "--tick-base" => {
                a.tick_base = Some(val("--tick-base")?.parse().map_err(|_| "bad --tick-base")?)
            }
            "--ticks" => a.ticks = Some(val("--ticks")?.parse().map_err(|_| "bad --ticks")?),
            "--interval-per-tick" => {
                a.ipt = Some(
                    val("--interval-per-tick")?
                        .parse()
                        .map_err(|_| "bad --interval-per-tick")?,
                )
            }
            "--players" => {
                let v = val("--players")?;
                let list: Result<Vec<u32>, _> = v
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim().parse::<u32>())
                    .collect();
                a.players = Some(list.map_err(|_| "bad --players")?);
            }
            "-h" | "--help" => {
                println!("usage: hl_extras <demo.dem> [--json file] [--self-check] [--print-options file] [--wearable-defs file] [--counts-only] [--groups a,b] [--start-tick N --tick-base N --ticks N --players e0,e1,...]");
                process::exit(0);
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => a.demo = other.to_string(),
        }
    }
    if a.demo.is_empty() {
        return Err("no demo given".into());
    }
    Ok(a)
}

/// Iterates the entries of a track node: (key, tick, entry index).
fn track_entries(t: &XNode, key_count: usize) -> Vec<(usize, u32, usize)> {
    let mut out = Vec::new();
    let (Some(off), Some(ticks)) = (t.get("offsets"), t.get("ticks")) else {
        return out;
    };
    for k in 0..key_count {
        let (Some(a), Some(b)) = (off.num_at(k), off.num_at(k + 1)) else {
            continue;
        };
        for i in a as usize..b as usize {
            out.push((k, ticks.num_at(i).unwrap_or(0.0) as u32, i));
        }
    }
    out
}

fn col(t: &XNode, name: &str, i: usize) -> f64 {
    t.get(name).and_then(|c| c.num_at(i)).unwrap_or(f64::NAN)
}

fn group_entries(node: &XNode) -> usize {
    // number of track entries / events / instances below a group node
    match node {
        XNode::Obj(fields) => {
            let mut n = 0;
            for (k, v) in fields {
                match (k.as_str(), v) {
                    ("ticks", XNode::U32(t)) | ("tick", XNode::U32(t)) => n += t.len(),
                    (_, XNode::Obj(_)) => n += group_entries(v),
                    _ => {}
                }
            }
            n
        }
        XNode::Events(e) => e.len(),
        _ => 0,
    }
}

struct Check {
    pairs: usize,
    within: usize,
    alive_base: usize,
    need: usize,
    ok: bool,
    reason: String,
    max_dist: f64,
}

fn self_check(out: &ExtrasOutput) -> Option<Check> {
    let gs = out.gs.as_ref()?;
    let pos = out.path("alignment.pos")?;
    let pc = out
        .path("meta.playerCount")
        .and_then(|n| n.as_num())
        .unwrap_or(0.0) as usize;
    let mut c = Check {
        pairs: 0,
        within: 0,
        alive_base: 0,
        need: 0,
        ok: false,
        reason: String::new(),
        max_dist: 0.0,
    };
    for (k, row) in gs.samples.iter().enumerate() {
        for p in 0..pc {
            let Some(Some((bp, alive))) = row.get(p) else {
                continue;
            };
            if !*alive {
                continue;
            }
            c.alive_base += 1;
            let x = pos.num_at((p * 16 + k) * 3).unwrap_or(f64::NAN);
            let y = pos.num_at((p * 16 + k) * 3 + 1).unwrap_or(f64::NAN);
            let z = pos.num_at((p * 16 + k) * 3 + 2).unwrap_or(f64::NAN);
            if x.is_nan() || y.is_nan() || z.is_nan() {
                continue;
            }
            c.pairs += 1;
            let d = ((x - bp[0] as f64).powi(2)
                + (y - bp[1] as f64).powi(2)
                + (z - bp[2] as f64).powi(2))
            .sqrt();
            c.max_dist = c.max_dist.max(d);
            if d <= 4.0 {
                c.within += 1;
            }
        }
    }
    c.need = 20usize.min(c.alive_base.div_ceil(2));
    if c.alive_base == 0 {
        c.ok = true;
        c.reason = "no alive samples".into();
    } else {
        c.ok = c.pairs >= c.need
            && if c.pairs >= 20 {
                c.within as f64 / c.pairs as f64 >= 0.9
            } else {
                c.within == c.pairs
            };
        if !c.ok {
            c.reason = format!("pairs {} need {} within {}", c.pairs, c.need, c.within);
        }
    }
    Some(c)
}

fn stats(out: &ExtrasOutput, player_classes: &[Vec<u8>]) -> Value {
    let pc = out
        .path("meta.playerCount")
        .and_then(|n| n.as_num())
        .unwrap_or(0.0) as usize;
    let mut s = Map::new();
    // HL and raw cond rising edges
    let mut hl_rising = [0u64; 32];
    let mut cond_rising: BTreeMap<u32, u64> = BTreeMap::new();
    let mut hl_by_player: Vec<[u64; 32]> = vec![[0; 32]; pc];
    if let Some(cond) = out.path("players.cond") {
        let mut prev: Vec<[u32; 5]> = vec![[0; 5]; pc];
        for (k, _t, i) in track_entries(cond, pc) {
            let w = [
                col(cond, "w0", i) as u32,
                col(cond, "w1", i) as u32,
                col(cond, "w2", i) as u32,
                col(cond, "w3", i) as u32,
                col(cond, "w4", i) as u32,
            ];
            let Some(pw) = prev.get(k).copied() else {
                continue;
            };
            let (fa, fb) = (derive_flags(&pw), derive_flags(&w));
            for bit in 0..32 {
                if fb & (1 << bit) != 0 && fa & (1 << bit) == 0 {
                    if let Some(c) = hl_rising.get_mut(bit) {
                        *c += 1;
                    }
                    if let Some(c) = hl_by_player.get_mut(k).and_then(|r| r.get_mut(bit)) {
                        *c += 1;
                    }
                }
            }
            for c in 0..160u32 {
                if cond_set(&w, c) && !cond_set(&pw, c) {
                    *cond_rising.entry(c).or_insert(0) += 1;
                }
            }
            if let Some(slot) = prev.get_mut(k) {
                *slot = w;
            }
        }
    }
    let mut hl = Map::new();
    for (bit, name) in HL_NAMES.iter().enumerate() {
        hl.insert(
            name.to_string(),
            json!(hl_rising.get(bit).copied().unwrap_or(0)),
        );
    }
    s.insert("hlRising".into(), Value::Object(hl));
    s.insert(
        "condRising".into(),
        Value::Object(
            cond_rising
                .iter()
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect(),
        ),
    );
    // per player
    let mut fire_by_p = vec![0u64; pc];
    let mut fire_by_def: BTreeMap<u32, u64> = BTreeMap::new();
    if let Some(fire) = out.path("players.fire") {
        let n = fire.get("tick").map(|t| t.len()).unwrap_or(0);
        for i in 0..n {
            let p = col(fire, "player", i) as usize;
            if let Some(c) = fire_by_p.get_mut(p) {
                *c += 1;
            }
            *fire_by_def
                .entry(col(fire, "weaponDef", i) as u32)
                .or_insert(0) += 1;
        }
    }
    let mut anim_by_p = vec![0u64; pc];
    if let Some(anim) = out.path("players.anim") {
        let n = anim.get("tick").map(|t| t.len()).unwrap_or(0);
        for i in 0..n {
            if let Some(c) = anim_by_p.get_mut(col(anim, "player", i) as usize) {
                *c += 1;
            }
        }
    }
    let mut bullets_by_p = vec![0u64; pc];
    if let Some(b) = out.path("tempents.bullets") {
        let n = b.get("tick").map(|t| t.len()).unwrap_or(0);
        for i in 0..n {
            if let Some(c) = bullets_by_p.get_mut(col(b, "player", i) as usize) {
                *c += 1;
            }
        }
    }
    // medigun entries with a def while the player is a spy (class at entry from player_classes)
    let mut players = Vec::new();
    let entity = out.path("players.entityId");
    let scoring = out.path("players.scoring");
    let mut scoring_players = 0;
    for p in 0..pc {
        let mut classes: Vec<u8> = player_classes.get(p).cloned().unwrap_or_default();
        classes.sort();
        classes.dedup();
        let sc_entries = scoring
            .map(|t| {
                let off = t.get("offsets");
                let a = off.and_then(|o| o.num_at(p)).unwrap_or(0.0);
                let b = off.and_then(|o| o.num_at(p + 1)).unwrap_or(0.0);
                (b - a) as u64
            })
            .unwrap_or(0);
        if sc_entries > 0 {
            scoring_players += 1;
        }
        let mut hlp = Map::new();
        if let Some(row) = hl_by_player.get(p) {
            for (bit, name) in HL_NAMES.iter().enumerate() {
                let v = row.get(bit).copied().unwrap_or(0);
                if v > 0 {
                    hlp.insert(name.to_string(), json!(v));
                }
            }
        }
        players.push(json!({
            "p": p,
            "entityId": entity.and_then(|e| e.num_at(p)).unwrap_or(0.0),
            "name": out.player_names.get(p).cloned().unwrap_or_default(),
            "classes": classes,
            "fire": fire_by_p.get(p).copied().unwrap_or(0),
            "anim": anim_by_p.get(p).copied().unwrap_or(0),
            "bullets": bullets_by_p.get(p).copied().unwrap_or(0),
            "scoringEntries": sc_entries,
            "hlRising": hlp,
        }));
    }
    s.insert("players".into(), Value::Array(players));
    s.insert("scoringPlayers".into(), json!(scoring_players));
    s.insert(
        "fireByDef".into(),
        Value::Object(
            fire_by_def
                .iter()
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect(),
        ),
    );
    // projectiles
    let mut kinds: BTreeMap<u32, u64> = BTreeMap::new();
    let mut proj_crit = 0u64;
    if let Some(pj) = out.group("projectiles") {
        let n = pj.get("count").and_then(|c| c.as_num()).unwrap_or(0.0) as usize;
        for i in 0..n {
            *kinds.entry(col(pj, "kind", i) as u32).or_insert(0) += 1;
            if col(pj, "crit", i) > 0.0 {
                proj_crit += 1;
            }
        }
        s.insert(
            "projectilePositions".into(),
            json!(pj.get("pos").map(|p| p.len() / 3).unwrap_or(0)),
        );
        s.insert(
            "deflectEntries".into(),
            json!(pj.path("deflects.ticks").map(|t| t.len()).unwrap_or(0)),
        );
    }
    s.insert(
        "projectileKinds".into(),
        Value::Object(
            kinds
                .iter()
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect(),
        ),
    );
    s.insert("projectileCrits".into(), json!(proj_crit));
    // dots
    let (mut l0, mut l1) = (0u64, 0u64);
    if let Some(d) = out.group("dots") {
        let n = d.get("count").and_then(|c| c.as_num()).unwrap_or(0.0) as usize;
        for i in 0..n {
            if col(d, "laser", i) > 0.0 {
                l1 += 1;
            } else {
                l0 += 1;
            }
        }
    }
    s.insert("dots".into(), json!({"laser0": l0, "laser1": l1}));
    // buildings
    let mut bkinds: BTreeMap<String, u64> = BTreeMap::new();
    if let Some(b) = out.group("buildings") {
        let n = b.get("count").and_then(|c| c.as_num()).unwrap_or(0.0) as usize;
        for i in 0..n {
            let key = format!(
                "type{}_mapPlaced{}",
                col(b, "type", i),
                col(b, "mapPlaced", i)
            );
            *bkinds.entry(key).or_insert(0) += 1;
        }
    }
    s.insert("buildings".into(), json!(bkinds));
    // weapons: disguise instances per class
    let mut wdis: BTreeMap<String, u64> = BTreeMap::new();
    let classes = out.path("strings.classes");
    if let (Some(w), Some(XNode::Strs(names))) = (out.group("weapons"), classes) {
        let n = w.get("count").and_then(|c| c.as_num()).unwrap_or(0.0) as usize;
        for i in 0..n {
            if col(w, "disguise", i) > 0.0 {
                let cls = names
                    .get(col(w, "cls", i) as usize)
                    .cloned()
                    .unwrap_or_default();
                *wdis.entry(cls).or_insert(0) += 1;
            }
        }
    }
    s.insert("weaponDisguiseInstances".into(), json!(wdis));
    // events
    let mut ev_counts: BTreeMap<&str, u64> = BTreeMap::new();
    let mut feign = 0u64;
    if let Some(events) = out.group("events").and_then(|e| e.as_events()) {
        for e in events {
            *ev_counts.entry(e.name()).or_insert(0) += 1;
            if let HlEvent::Kill { feign: true, .. } = e {
                feign += 1;
            }
        }
    }
    s.insert("eventCounts".into(), json!(ev_counts));
    s.insert("killFeign".into(), json!(feign));
    // tempents
    let te_len = |path: &str| out.path(path).map(|t| t.len()).unwrap_or(0);
    let mut cp_kinds: BTreeMap<u32, u64> = BTreeMap::new();
    if let Some(cp) = out.path("tempents.clientProjectiles") {
        for i in 0..cp.get("tick").map(|t| t.len()).unwrap_or(0) {
            *cp_kinds.entry(col(cp, "kind", i) as u32).or_insert(0) += 1;
        }
    }
    s.insert(
        "tempents".into(),
        json!({
            "bullets": te_len("tempents.bullets.tick"),
            "explosions": te_len("tempents.explosions.tick"),
            "tracers": te_len("tempents.tracers.tick"),
            "particles": te_len("tempents.particles.tick"),
            "clientProjectiles": te_len("tempents.clientProjectiles.tick"),
            "clientProjectileKinds": cp_kinds.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<String, Value>>(),
            "anim": te_len("players.anim.tick"),
            "fire": te_len("players.fire.tick"),
            "hurts": te_len("hurts.tick"),
        }),
    );
    // objectives
    if let Some(ob) = out.group("objectives") {
        let mut path = Vec::new();
        if let Some(pd) = ob.path("cp.pathDistance") {
            for i in 0..pd.len() {
                path.push(pd.num_at(i).unwrap_or(0.0));
            }
        }
        let rules = ob.get("rules");
        let mut stopwatch = false;
        let mut sw_state_max = 0.0f64;
        let mut red_koth = 0usize;
        let mut blu_koth = 0usize;
        if let Some(r) = rules {
            for (_, _, i) in track_entries(r, 1) {
                if (col(r, "flags", i) as u32) & (1 << 3) != 0 {
                    stopwatch = true;
                }
                sw_state_max = sw_state_max.max(col(r, "swState", i));
                if col(r, "redKoth", i) >= 0.0 {
                    red_koth += 1;
                }
                if col(r, "bluKoth", i) >= 0.0 {
                    blu_koth += 1;
                }
            }
        }
        let mut pd_max = 0.0f64;
        let mut pd_red = 0.0f64;
        let mut pd_blu = 0.0f64;
        let pd_entries = ob.path("pd.ticks").map(|t| t.len()).unwrap_or(0);
        if let Some(pd) = ob.get("pd") {
            for (_, _, i) in track_entries(pd, 1) {
                pd_max = pd_max.max(col(pd, "max", i));
                pd_red = pd_red.max(col(pd, "red", i));
                pd_blu = pd_blu.max(col(pd, "blu", i));
            }
        }
        s.insert(
            "objectives".into(),
            json!({
                "gameType": ob.get("gameType").and_then(|n| n.as_num()),
                "koth": ob.get("koth").and_then(|n| n.as_num()),
                "cpCount": ob.path("cp.count").and_then(|n| n.as_num()),
                "cpLayout": match ob.path("cp.layout") { Some(XNode::Str(s)) => s.clone(), _ => String::new() },
                "pathDistance": path,
                "rulesEntries": rules.and_then(|r| r.get("ticks")).map(|t| t.len()).unwrap_or(0),
                "stopwatch": stopwatch,
                "swStateMax": sw_state_max,
                "redKothResolved": red_koth,
                "bluKothResolved": blu_koth,
                "timers": ob.path("timers.count").and_then(|n| n.as_num()),
                "watchers": ob.path("watchers.count").and_then(|n| n.as_num()),
                "cpStateEntries": ob.path("cp.state.ticks").map(|t| t.len()).unwrap_or(0),
                "cpProgressEntries": ob.path("cp.progress.ticks").map(|t| t.len()).unwrap_or(0),
                "cartMovers": ob.path("cart.movers").map(|t| t.len()).unwrap_or(0),
                "cartTrains": ob.path("cart.trains").map(|t| t.len()).unwrap_or(0),
                "pdEntries": pd_entries,
                "pdMax": pd_max,
                "pdRedMax": pd_red,
                "pdBluMax": pd_blu,
                "teamsEntries": ob.path("teams.ticks").map(|t| t.len()).unwrap_or(0),
            }),
        );
    }
    // movers etc.
    let count = |g: &str| {
        out.path(&format!("{g}.count"))
            .and_then(|n| n.as_num())
            .unwrap_or(0.0)
    };
    s.insert(
        "instances".into(),
        json!({
            "weapons": count("weapons"),
            "buildings": count("buildings"),
            "projectiles": count("projectiles"),
            "flames": count("flames"),
            "dots": count("dots"),
            "movers": count("movers"),
            "pickups": count("pickups"),
            "ragdolls": count("ragdolls"),
            "drops": count("drops"),
        }),
    );
    // convars at tick 0
    let mut cv0 = Map::new();
    if let Some(XNode::Arr(list)) = out.path("meta.convars") {
        for c in list {
            let tick = c.get("tick").and_then(|n| n.as_num()).unwrap_or(-1.0);
            if tick == 0.0 {
                if let (Some(XNode::Str(k)), Some(XNode::Str(v))) = (c.get("key"), c.get("value")) {
                    cv0.insert(k.clone(), json!(v));
                }
            }
        }
    }
    s.insert("convarsTick0".into(), Value::Object(cv0));
    Value::Object(s)
}

fn survey(out: &ExtrasOutput, st: &Value) -> Value {
    let cond = |c: u32| {
        st.pointer(&format!("/condRising/{c}"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    let pk = |k: u32| {
        st.pointer(&format!("/projectileKinds/{k}"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    let ck = |k: u32| {
        st.pointer(&format!("/tempents/clientProjectileKinds/{k}"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    let ev = |n: &str| {
        st.pointer(&format!("/eventCounts/{n}"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    let mut conds = Map::new();
    for c in [15, 24, 27, 28, 58, 59, 60, 123] {
        conds.insert(c.to_string(), json!(cond(c)));
    }
    let mut kinds = Map::new();
    for k in [5u32, 9, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20] {
        kinds.insert(k.to_string(), json!(pk(k)));
    }
    json!({
        "map": match out.path("meta.map") { Some(XNode::Str(s)) => s.clone(), _ => String::new() },
        "ticks": out.path("meta.ticks").and_then(|n| n.as_num()),
        "condRising": conds,
        "clientProjectiles": { "17": ck(17), "21": ck(21) },
        "projectileKinds": kinds,
        "dotsLaser1": st.pointer("/dots/laser1").and_then(|v| v.as_u64()).unwrap_or(0),
        "events": { "banner": ev("banner"), "teleported": ev("teleported"), "sapped": ev("sapped"), "deflect": ev("deflect") },
        "uberDeployed": ev("uberDeployed"),
    })
}

fn num_json(v: f64) -> Value {
    if v.is_finite() {
        json!(v)
    } else {
        Value::Null
    }
}

/// Full output tree as JSON (typed arrays as arrays, NaN as null), for inspection.
fn node_json(n: &XNode) -> Value {
    match n {
        XNode::Obj(f) => Value::Object(f.iter().map(|(k, v)| (k.clone(), node_json(v))).collect()),
        XNode::Arr(a) => Value::Array(a.iter().map(node_json).collect()),
        XNode::U8(_)
        | XNode::I8(_)
        | XNode::U16(_)
        | XNode::I16(_)
        | XNode::U32(_)
        | XNode::I32(_)
        | XNode::F32(_) => Value::Array(
            (0..n.len())
                .map(|i| num_json(n.num_at(i).unwrap_or(f64::NAN)))
                .collect(),
        ),
        XNode::Num(x) => num_json(*x),
        XNode::Bool(b) => json!(b),
        XNode::Str(s) => json!(s),
        XNode::Strs(s) => json!(s),
        XNode::Events(e) => serde_json::to_value(e).unwrap_or(Value::Null),
    }
}

/// Debug listing of an instance group with per-instance track entry counts.
fn list_instances(out: &ExtrasOutput, group: &str) {
    let Some(g) = out.group(group) else { return };
    let n = g.get("count").and_then(|c| c.as_num()).unwrap_or(0.0) as usize;
    let models = match out.path("strings.models") {
        Some(XNode::Strs(m)) => m.clone(),
        _ => Vec::new(),
    };
    let tracks: Vec<(String, &XNode)> = match g {
        XNode::Obj(fields) => fields
            .iter()
            .filter(|(_, v)| v.get("offsets").is_some())
            .map(|(k, v)| (k.clone(), v))
            .collect(),
        _ => Vec::new(),
    };
    let mut rows: Vec<(usize, String)> = Vec::new();
    for i in 0..n {
        let mut line = format!(
            "{i:5} ent {:4} created {:6} deleted {:10}",
            col(g, "entityId", i),
            col(g, "created", i),
            col(g, "deleted", i)
        );
        for f in [
            "cls", "kind", "type", "bspModel", "model", "parent", "owner", "builder", "laser",
            "def", "team",
        ] {
            let v = col(g, f, i);
            if !v.is_nan() {
                line.push_str(&format!(" {f} {v}"));
                if f == "model" {
                    line.push_str(&format!(
                        " ({})",
                        models.get(v as usize).cloned().unwrap_or_default()
                    ));
                }
            }
        }
        let mut total = 0usize;
        for (name, t) in &tracks {
            let off = t.get("offsets");
            let a = off.and_then(|o| o.num_at(i)).unwrap_or(0.0);
            let b = off.and_then(|o| o.num_at(i + 1)).unwrap_or(0.0);
            let e = (b - a) as usize;
            total += e;
            line.push_str(&format!(" {name}:{e}"));
        }
        rows.push((total, line));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, l) in rows.iter().take(60) {
        println!("{l}");
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hl_extras: {e}");
            process::exit(2);
        }
    };
    let file = match fs::read(&args.demo) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("hl_extras: cannot read {}: {e}", args.demo);
            process::exit(2);
        }
    };
    let t0 = Instant::now();
    let explicit = args.start_tick.is_some()
        || args.tick_base.is_some()
        || args.ticks.is_some()
        || args.players.is_some();
    let mut base_names: Vec<String> = Vec::new();
    let mut base_json = Value::Null;
    let mut opts = if explicit {
        match (args.start_tick, args.tick_base, args.ticks, &args.players) {
            (Some(s), Some(b), Some(t), Some(p)) => ExtrasOptions {
                start_tick: s,
                tick_base: b,
                ticks: t,
                interval_per_tick: args.ipt.unwrap_or(0.015),
                player_entity_ids: p.clone(),
                groups: None,
                wearable_defs: None,
            },
            _ => {
                eprintln!("hl_extras: explicit base values need --start-tick, --tick-base, --ticks and --players");
                process::exit(2);
            }
        }
    } else {
        match derive_options_with(&file, !args.start_without_world) {
            Ok(d) => {
                base_names = d.names.clone();
                base_json = json!({
                    "startTick": d.options.start_tick,
                    "startDemoTick": d.start_demo_tick,
                    "tickBase": d.options.tick_base,
                    "ticks": d.options.ticks,
                    "intervalPerTick": d.options.interval_per_tick,
                    "nextMappedPlayer": d.next_mapped_player,
                    "playerEntityIds": d.options.player_entity_ids,
                    "worldMin": d.world_min,
                    "worldMax": d.world_max,
                    "headerTicks": d.header_ticks,
                    "started": d.started,
                    "firstWorldTick": d.first_world_tick,
                    "warmupFrames": d.warmup_frames,
                });
                if !d.started {
                    eprintln!(
                        "hl_extras: lib.rs start rule never fired (warmup frames {}, first non-zero world bounds at demo tick {:?}); the base parser would produce an empty demo",
                        d.warmup_frames, d.first_world_tick
                    );
                }
                d.options
            }
            Err(e) => {
                eprintln!("hl_extras: {e}");
                process::exit(1);
            }
        }
    };
    let derive_ms = t0.elapsed().as_secs_f64() * 1000.0;
    if let Some(path) = &args.print_options {
        let v = json!({
            "startTick": opts.start_tick,
            "tickBase": opts.tick_base,
            "ticks": opts.ticks,
            "intervalPerTick": opts.interval_per_tick,
            "playerEntityIds": opts.player_entity_ids,
            "derived": base_json.clone(),
        });
        if let Err(e) = fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default()) {
            eprintln!("hl_extras: cannot write {path}: {e}");
        }
    }
    if let Some(path) = &args.wearable_defs {
        match fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<u16>>(&s).ok())
        {
            Some(v) => opts.wearable_defs = Some(v),
            None => {
                eprintln!("hl_extras: cannot read wearable defs from {path}");
                process::exit(2);
            }
        }
    }
    opts.groups = args.groups.clone();

    let t1 = Instant::now();
    let mut progress = |_p: u32| {};
    let result = if args.self_check {
        parse_extras_checked(&file, &opts, &mut progress)
    } else {
        parse_extras(&file, &opts, &mut progress)
    };
    let parse_ms = t1.elapsed().as_secs_f64() * 1000.0;
    let out = match result {
        Ok(o) => o,
        Err(e) => {
            eprintln!("hl_extras: {e}");
            process::exit(1);
        }
    };
    let hwm = vm_hwm_kb();
    let player_classes: Vec<Vec<u8>> = out.player_classes.clone();
    let st = stats(&out, &player_classes);
    let check = self_check(&out);
    let warnings: Vec<String> = match out.path("meta.warnings") {
        Some(XNode::Strs(w)) => w.clone(),
        _ => Vec::new(),
    };
    let truncated = warnings.iter().any(|w| w.contains("truncated"));

    if args.counts_only {
        let mut sv = survey(&out, &st);
        if let Value::Object(m) = &mut sv {
            m.insert("demo".into(), json!(args.demo));
            m.insert("fileBytes".into(), json!(file.len()));
            m.insert("parseMs".into(), json!(parse_ms));
            m.insert("vmHwmKB".into(), json!(hwm));
            m.insert("truncated".into(), json!(truncated));
        }
        println!("{}", serde_json::to_string(&sv).unwrap_or_default());
        if let Some(path) = &args.json {
            let _ = fs::write(path, serde_json::to_string_pretty(&sv).unwrap_or_default());
        }
        return;
    }

    if let Some(what) = &args.list {
        list_instances(&out, what);
    }
    if let Some(path) = &args.dump {
        let mut root = Map::new();
        for (name, node) in &out.groups {
            root.insert(name.clone(), node_json(node));
        }
        if let Err(e) = fs::write(
            path,
            serde_json::to_string(&Value::Object(root)).unwrap_or_default(),
        ) {
            eprintln!("hl_extras: cannot write {path}: {e}");
        }
    }
    // per-group bytes and entries
    let mut groups = Map::new();
    let mut total_bytes = 0usize;
    println!("{}", args.demo);
    println!("{:<12} {:>10} {:>12}", "group", "entries", "bytes");
    for (name, node) in &out.groups {
        let b = node.bytes();
        let e = group_entries(node);
        total_bytes += b;
        println!("{:<12} {:>10} {:>12}", name, e, b);
        groups.insert(name.clone(), json!({"bytes": b, "entries": e}));
    }
    println!("total typed-array bytes {total_bytes}");
    println!(
        "derive {:.0} ms, parse {:.0} ms, VmHWM {} kB, file {} bytes",
        derive_ms,
        parse_ms,
        hwm,
        file.len()
    );
    if !out.diag.is_empty() {
        println!("diagnostics:");
        for d in &out.diag {
            println!("  {d}");
        }
    }
    if !warnings.is_empty() {
        println!("warnings:");
        for w in &warnings {
            println!("  {w}");
        }
    }
    let mut check_json = Value::Null;
    if let Some(c) = &check {
        println!(
            "self-check: {} (pairs {}, within {}, aliveBase {}, need {}, max distance {:.3}) {}",
            if c.ok { "PASS" } else { "FAIL" },
            c.pairs,
            c.within,
            c.alive_base,
            c.need,
            c.max_dist,
            c.reason
        );
        check_json = json!({
            "ok": c.ok, "pairs": c.pairs, "within": c.within, "aliveBase": c.alive_base, "need": c.need,
            "maxDistance": c.max_dist, "reason": c.reason,
        });
    }
    let meta = out.group("meta");
    let available: Map<String, Value> = match meta.and_then(|m| m.get("available")) {
        Some(XNode::Obj(fields)) => fields
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    json!(v.as_num().map(|n| n > 0.0).unwrap_or(false)),
                )
            })
            .collect(),
        _ => Map::new(),
    };
    let counts: Map<String, Value> = match meta.and_then(|m| m.get("counts")) {
        Some(XNode::Obj(fields)) => fields
            .iter()
            .map(|(k, v)| (k.clone(), json!(v.as_num())))
            .collect(),
        _ => Map::new(),
    };
    let align_ticks: Vec<f64> = out
        .path("alignment.ticks")
        .map(|t| (0..t.len()).filter_map(|i| t.num_at(i)).collect())
        .unwrap_or_default();
    let summary = json!({
        "demo": args.demo,
        "fileBytes": file.len(),
        "deriveMs": derive_ms,
        "parseMs": parse_ms,
        "vmHwmKB": hwm,
        "base": base_json,
        "baseNames": base_names,
        "options": {
            "startTick": opts.start_tick, "tickBase": opts.tick_base, "ticks": opts.ticks,
            "intervalPerTick": opts.interval_per_tick, "playerEntityIds": opts.player_entity_ids,
            "groups": opts.groups, "wearableDefs": opts.wearable_defs.as_ref().map(|v| v.len()),
        },
        "meta": {
            "snapshotInterval": meta.and_then(|m| m.get("snapshotInterval")).and_then(|n| n.as_num()),
            "intervalPerTick": meta.and_then(|m| m.get("intervalPerTick")).and_then(|n| n.as_num()),
            "map": match meta.and_then(|m| m.get("map")) { Some(XNode::Str(s)) => s.clone(), _ => String::new() },
            "available": available,
            "counts": counts,
            "excluded": match meta.and_then(|m| m.get("excluded")) { Some(XNode::Strs(s)) => s.clone(), _ => Vec::new() },
            "warnings": warnings,
        },
        "alignmentTicks": align_ticks,
        "alignmentFromPacket": out.align_hit.to_vec(),
        "selfCheck": check_json,
        "groups": groups,
        "totalBytes": total_bytes,
        "diag": out.diag,
        "diagCounts": out.diag_counts,
        "stats": st,
        "truncated": truncated,
    });
    if let Some(path) = &args.json {
        if let Err(e) = fs::write(
            path,
            serde_json::to_string_pretty(&summary).unwrap_or_default(),
        ) {
            eprintln!("hl_extras: cannot write {path}: {e}");
        }
    }
    if truncated {
        eprintln!("hl_extras: parse truncated");
        process::exit(1);
    }
    if let Some(c) = check {
        if !c.ok {
            process::exit(1);
        }
    }
}
