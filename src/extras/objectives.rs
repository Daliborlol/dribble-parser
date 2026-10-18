//! `objectives` group (section 2.5.6): game rules, teams, round timers, the objective resource
//! with simulated capture progress (section 2.6.1), train watchers, carts and tug-of-war scores.

use super::out::{HlEvent, ObjB, XNode};
use super::track::{empty_instances, empty_track, record, InstanceTable, TrackBuilder};
use super::tracker::{ArrVal, ClassInfo, Ent, Role, Val};
use super::{clamp_u16, to_i8, to_u8, Ex, Orders, Slots};

record!(RulesRec {
    round_state: u8 => "roundState", winner: u8 => "winner", flags: u16 => "flags", sw_state: u8 => "swState",
    rounds: u8 => "rounds", red_koth: i16 => "redKoth", blu_koth: i16 => "bluKoth", hud_timer: i16 => "hudTimer",
    sw_timer: i16 => "swTimer", wave_red: f32 => "waveRed", wave_blu: f32 => "waveBlu", restart_tick: f32 => "restartTick"
});
record!(TeamRec { score: i32 => "score", rounds_won: u16 => "roundsWon", role: u8 => "role", flag_caps: u8 => "flagCaps" });
record!(TimerRec {
    paused: u8 => "paused", end_tick: f32 => "endTick", remaining: f32 => "remaining", length: u16 => "length",
    max_length: u16 => "maxLength", setup_length: u16 => "setupLength", mode: u8 => "mode", flags: u8 => "flags",
    total_time: f32 => "totalTime"
});
record!(WatcherRec { progress: u16 => "progress", speed: i8 => "speed", cappers: i8 => "cappers", recede_tick: f32 => "recedeTick" });
record!(CpStateRec {
    owner: u8 => "owner", capping: u8 => "capping", in_zone: u8 => "inZone", num_red: u8 => "numRed",
    num_blu: u8 => "numBlu", lazy: f32 => "lazy", flags: u8 => "flags", unlock_tick: f32 => "unlockTick"
});
record!(ProgressRec { p: u16 => "p" });
record!(PdRec { red: u16 => "red", blu: u16 => "blu", max: u16 => "max" });
record!(
    /// `m_iPreviousPoints` of one control point for RED and BLU, three slots each (-1 none).
    PrevRec {
    red0: i8 => "red0", red1: i8 => "red1", red2: i8 => "red2", blu0: i8 => "blu0", blu1: i8 => "blu1", blu2: i8 => "blu2"
    }
);
record!(
    /// `m_iBaseControlPoints` of RED and BLU (-1 none) and `m_bPlayingMiniRounds`.
    BaseRec { red: i8 => "baseRed", blu: i8 => "baseBlu", mini: u8 => "miniRounds" }
);

pub const RULES_IN_OVERTIME: u16 = 1 << 1;

#[derive(Default)]
pub struct RulesSlots {
    pub round_state: Option<u32>,
    pub winner: Option<u32>,
    pub rounds: Option<u32>,
    /// RULES bits 0..9 sources
    pub flags: [Option<u32>; 10],
    pub game_type: Option<u32>,
    pub koth: Option<u32>,
    pub sw_state: Option<u32>,
    pub red_koth: Option<u32>,
    pub blue_koth: Option<u32>,
    pub restart: Option<u32>,
    pub wave_red: Option<u32>,
    pub wave_blu: Option<u32>,
}

#[inline(never)]
pub fn setup_rules(ci: &mut ClassInfo) -> Vec<String> {
    let r = "DT_TeamplayRoundBasedRules";
    let g = "DT_TFGameRules";
    let s = RulesSlots {
        round_state: ci.find(r, "m_iRoundState"),
        winner: ci.find(r, "m_iWinningTeam"),
        rounds: ci.find(r, "m_nRoundsPlayed"),
        flags: [
            ci.find(r, "m_bInSetup"),
            ci.find(r, "m_bInOvertime"),
            ci.find(r, "m_bInWaitingForPlayers"),
            ci.find(r, "m_bStopWatch"),
            ci.find(g, "m_bStopWatchWinner"),
            ci.find(r, "m_bSwitchedTeamsThisRound"),
            ci.find(g, "m_bTeamsSwitched"),
            ci.find(r, "m_bAwaitingReadyRestart"),
            ci.find(g, "m_bMatchEnded"),
            ci.find(g, "m_bPlayingKoth"),
        ],
        game_type: ci.find(g, "m_nGameType"),
        koth: ci.find(g, "m_bPlayingKoth"),
        sw_state: ci.find(g, "m_nStopWatchState"),
        red_koth: ci.find(g, "m_hRedKothTimer"),
        blue_koth: ci.find(g, "m_hBlueKothTimer"),
        restart: ci.find(r, "m_flRestartRoundTime"),
        wave_red: ci.elem("m_flNextRespawnWave", 2),
        wave_blu: ci.elem("m_flNextRespawnWave", 3),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("m_iRoundState", s.round_state),
        ("m_bInOvertime", s.flags[1]),
        ("m_bStopWatch", s.flags[3]),
        ("m_nGameType", s.game_type),
        ("m_hRedKothTimer", s.red_koth),
        ("m_flNextRespawnWave.002", s.wave_red),
    ] {
        if x.is_none() {
            missing.push(n.to_string());
        }
    }
    let mut keep = vec![
        s.round_state,
        s.winner,
        s.rounds,
        s.game_type,
        s.koth,
        s.sw_state,
        s.red_koth,
        s.blue_koth,
        s.restart,
        s.wave_red,
        s.wave_blu,
    ];
    keep.extend(s.flags);
    for k in &keep {
        ci.add_dep(*k, 1);
    }
    ci.keep_only(&keep);
    ci.slots = Slots::Rules(Box::new(s));
    missing
}

#[derive(Default)]
pub struct ObjResSlots {
    pub num_cp: Option<u32>,
    pub layout: Option<u32>,
    pub positions: Option<u32>,
    pub timer_hud: Option<u32>,
    pub sw_timer: Option<u32>,
    pub owner: [Option<u32>; 8],
    pub capping: [Option<u32>; 8],
    pub in_zone: [Option<u32>; 8],
    pub lazy: [Option<u32>; 8],
    pub blocked: [Option<u32>; 8],
    pub locked: [Option<u32>; 8],
    pub unlock: [Option<u32>; 8],
    pub visible: [Option<u32>; 8],
    pub path: [Option<u32>; 8],
    pub group: [Option<u32>; 8],
    pub scales: [Option<u32>; 8],
    pub members: Vec<Option<u32>>,
    pub req: Vec<Option<u32>>,
    pub cap_time: Vec<Option<u32>>,
    pub can_cap: Vec<Option<u32>>,
    /// `m_iPreviousPoints` elements 000..191: slot + 3 x point + 24 x team
    /// (`CBaseTeamObjectiveResource::GetPreviousPointForPoint`)
    pub prev: Vec<Option<u32>>,
    /// `m_iBaseControlPoints` elements 000..031 (one per team)
    pub base: Vec<Option<u32>>,
    pub mini_rounds: Option<u32>,
}

const O_RULES: u32 = 1;
const O_CP: u32 = 2;
const O_PREV: u32 = 4;

/// Element of `m_iPreviousPoints` for `point`, `team` and `slot` (team_objectiveresource.cpp:424-441).
pub fn prev_index(point: usize, team: usize, slot: usize) -> usize {
    slot + point * 3 + team * 8 * 3
}

#[inline(never)]
pub fn setup_objres(ci: &mut ClassInfo) -> Vec<String> {
    let b = "DT_BaseTeamObjectiveResource";
    let a8 = |ci: &ClassInfo, t: &str| -> [Option<u32>; 8] {
        let mut out = [None; 8];
        for (i, cell) in out.iter_mut().enumerate() {
            *cell = ci.elem(t, i);
        }
        out
    };
    let a64 =
        |ci: &ClassInfo, t: &str| -> Vec<Option<u32>> { (0..64).map(|i| ci.elem(t, i)).collect() };
    let s = ObjResSlots {
        num_cp: ci.find(b, "m_iNumControlPoints"),
        layout: ci.find(b, "m_pszCapLayoutInHUD"),
        positions: ci.find(b, "m_vCPPositions"),
        timer_hud: ci.find(b, "m_iTimerToShowInHUD"),
        sw_timer: ci.find(b, "m_iStopWatchTimer"),
        owner: a8(ci, "m_iOwner"),
        capping: a8(ci, "m_iCappingTeam"),
        in_zone: a8(ci, "m_iTeamInZone"),
        lazy: a8(ci, "m_flLazyCapPerc"),
        blocked: a8(ci, "m_bBlocked"),
        locked: a8(ci, "m_bCPLocked"),
        unlock: a8(ci, "m_flUnlockTimes"),
        visible: a8(ci, "m_bCPIsVisible"),
        path: a8(ci, "m_flPathDistance"),
        group: a8(ci, "m_iCPGroup"),
        scales: a8(ci, "m_bCPCapRateScalesWithPlayers"),
        members: a64(ci, "m_iNumTeamMembers"),
        req: a64(ci, "m_iTeamReqCappers"),
        cap_time: a64(ci, "m_flTeamCapTime"),
        can_cap: a64(ci, "m_bTeamCanCap"),
        prev: (0..192).map(|i| ci.elem("m_iPreviousPoints", i)).collect(),
        base: (0..32).map(|i| ci.elem("m_iBaseControlPoints", i)).collect(),
        mini_rounds: ci.find(b, "m_bPlayingMiniRounds"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("m_iNumControlPoints", s.num_cp),
        ("m_vCPPositions", s.positions),
        ("m_iTimerToShowInHUD", s.timer_hud),
        ("m_iStopWatchTimer", s.sw_timer),
        ("m_pszCapLayoutInHUD", s.layout),
    ] {
        if x.is_none() {
            missing.push(format!("{b}.{n}"));
        }
    }
    for (n, arr) in [
        ("m_iOwner", &s.owner),
        ("m_iCappingTeam", &s.capping),
        ("m_iTeamInZone", &s.in_zone),
        ("m_flLazyCapPerc", &s.lazy),
        ("m_bBlocked", &s.blocked),
        ("m_bCPLocked", &s.locked),
        ("m_flUnlockTimes", &s.unlock),
        ("m_bCPIsVisible", &s.visible),
        ("m_flPathDistance", &s.path),
        ("m_iCPGroup", &s.group),
        ("m_bCPCapRateScalesWithPlayers", &s.scales),
    ] {
        if arr.iter().any(|x| x.is_none()) {
            missing.push(format!("{n}.000..007"));
        }
    }
    for (n, arr) in [
        ("m_iNumTeamMembers", &s.members),
        ("m_iTeamReqCappers", &s.req),
        ("m_flTeamCapTime", &s.cap_time),
        ("m_bTeamCanCap", &s.can_cap),
    ] {
        if arr.iter().take(32).any(|x| x.is_none()) {
            missing.push(format!("{n}.000..031"));
        }
    }
    ci.add_dep(s.timer_hud, O_RULES);
    ci.add_dep(s.sw_timer, O_RULES);
    let mut keep: Vec<Option<u32>> = vec![s.num_cp, s.layout, s.positions, s.timer_hud, s.sw_timer];
    for arr in [
        &s.owner, &s.capping, &s.in_zone, &s.lazy, &s.blocked, &s.locked, &s.unlock, &s.visible,
        &s.path, &s.group, &s.scales,
    ] {
        keep.extend(arr.iter().copied());
    }
    for arr in [&s.members, &s.req, &s.cap_time, &s.can_cap] {
        keep.extend(arr.iter().copied());
    }
    for k in &keep {
        ci.add_dep(*k, O_CP);
    }
    if s.prev.iter().take(96).any(|x| x.is_none()) {
        missing.push("m_iPreviousPoints.000..095".to_string());
    }
    let mut prev_keep: Vec<Option<u32>> = s.prev.clone();
    prev_keep.extend(s.base.iter().take(4).copied());
    prev_keep.push(s.mini_rounds);
    for k in &prev_keep {
        ci.add_dep(*k, O_PREV);
    }
    keep.extend(prev_keep);
    if let Some(p) = s.positions {
        ci.keep_arrays.push(p);
    }
    ci.keep_only(&keep);
    ci.slots = Slots::ObjRes(Box::new(s));
    missing
}

#[derive(Default, Clone, Copy)]
pub struct TeamSlots {
    pub num: Option<u32>,
    pub score: Option<u32>,
    pub rounds_won: Option<u32>,
    pub role: Option<u32>,
    pub flag_caps: Option<u32>,
}

pub fn setup_team(ci: &mut ClassInfo) -> Vec<String> {
    let s = TeamSlots {
        num: ci.find("DT_Team", "m_iTeamNum"),
        score: ci.find("DT_Team", "m_iScore"),
        rounds_won: ci.find("DT_Team", "m_iRoundsWon"),
        role: ci.find("DT_TFTeam", "m_iRole"),
        flag_caps: ci.find("DT_TFTeam", "m_nFlagCaptures"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("DT_Team.m_iTeamNum", s.num),
        ("DT_Team.m_iScore", s.score),
        ("DT_Team.m_iRoundsWon", s.rounds_won),
        ("DT_TFTeam.m_iRole", s.role),
        ("DT_TFTeam.m_nFlagCaptures", s.flag_caps),
    ] {
        if x.is_none() {
            missing.push(n.to_string());
        }
    }
    ci.keep_only(&[s.num, s.score, s.rounds_won, s.role, s.flag_caps]);
    ci.slots = Slots::Team(s);
    missing
}

#[derive(Default, Clone, Copy)]
pub struct TimerSlots {
    pub paused: Option<u32>,
    pub remaining: Option<u32>,
    pub end_time: Option<u32>,
    pub length: Option<u32>,
    pub max_length: Option<u32>,
    pub setup_length: Option<u32>,
    pub state: Option<u32>,
    pub total_time: Option<u32>,
    /// TIMER bits 0..6 sources
    pub flags: [Option<u32>; 7],
}

#[inline(never)]
pub fn setup_timer(ci: &mut ClassInfo) -> Vec<String> {
    let t = "DT_TeamRoundTimer";
    let s = TimerSlots {
        paused: ci.find(t, "m_bTimerPaused"),
        remaining: ci.find(t, "m_flTimeRemaining"),
        end_time: ci.find(t, "m_flTimerEndTime"),
        length: ci.find(t, "m_nTimerLength"),
        max_length: ci.find(t, "m_nTimerMaxLength"),
        setup_length: ci.find(t, "m_nSetupTimeLength"),
        state: ci.find(t, "m_nState"),
        total_time: ci.find(t, "m_flTotalTime"),
        flags: [
            ci.find(t, "m_bIsDisabled"),
            ci.find(t, "m_bShowInHUD"),
            ci.find(t, "m_bStopWatchTimer"),
            ci.find(t, "m_bInCaptureWatchState"),
            ci.find(t, "m_bShowTimeRemaining"),
            ci.find(t, "m_bAutoCountdown"),
            ci.find(t, "m_bStartPaused"),
        ],
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("m_bTimerPaused", s.paused),
        ("m_flTimerEndTime", s.end_time),
        ("m_flTimeRemaining", s.remaining),
        ("m_nState", s.state),
        ("m_flTotalTime", s.total_time),
    ] {
        if x.is_none() {
            missing.push(format!("{t}.{n}"));
        }
    }
    ci.slots = Slots::Timer(s);
    missing
}

#[derive(Default, Clone, Copy)]
pub struct WatcherSlots {
    pub progress: Option<u32>,
    pub speed: Option<u32>,
    pub cappers: Option<u32>,
    pub recede: Option<u32>,
}

pub fn setup_watcher(ci: &mut ClassInfo) -> Vec<String> {
    let t = "DT_TeamTrainWatcher";
    let s = WatcherSlots {
        progress: ci.find(t, "m_flTotalProgress"),
        speed: ci.find(t, "m_iTrainSpeedLevel"),
        cappers: ci.find(t, "m_nNumCappers"),
        recede: ci.find(t, "m_flRecedeTime"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("m_flTotalProgress", s.progress),
        ("m_iTrainSpeedLevel", s.speed),
    ] {
        if x.is_none() {
            missing.push(format!("{t}.{n}"));
        }
    }
    ci.keep_only(&[s.progress, s.speed, s.cappers, s.recede]);
    ci.slots = Slots::Watcher(s);
    missing
}

#[derive(Default, Clone, Copy)]
pub struct PdSlots {
    pub red: Option<u32>,
    pub blu: Option<u32>,
    pub max: Option<u32>,
}

pub fn setup_pd(ci: &mut ClassInfo) -> Vec<String> {
    let t = "DT_TFRobotDestructionLogic";
    let s = PdSlots {
        red: ci.find(t, "m_nRedScore"),
        blu: ci.find(t, "m_nBlueScore"),
        max: ci.find(t, "m_nMaxPoints"),
    };
    let mut missing = Vec::new();
    for (n, x) in [
        ("m_nRedScore", s.red),
        ("m_nBlueScore", s.blu),
        ("m_nMaxPoints", s.max),
    ] {
        if x.is_none() {
            missing.push(format!("{t}.{n}"));
        }
    }
    ci.slots = Slots::Pd(s);
    missing
}

#[derive(Clone, Copy, Default)]
struct CpSim {
    init: bool,
    capping: i64,
    owner: i64,
    lazy_bits: u32,
    sim_capping: i64,
    left: f64,
}

/// Static control point arrays captured from the objective resource.
#[derive(Clone, Default)]
struct CpStatic {
    layout: String,
    pos: Vec<f32>,
    visible: Vec<u8>,
    group: Vec<u8>,
    cap_red: Vec<f32>,
    cap_blu: Vec<f32>,
    req_red: Vec<u8>,
    req_blu: Vec<u8>,
    path: Vec<f32>,
    scales: Vec<u8>,
}

pub struct ObState {
    pub rules_ent: Option<u32>,
    pub objres_ent: Option<u32>,
    pub rules: TrackBuilder<RulesRec>,
    pub teams: TrackBuilder<TeamRec>,
    pub timers: InstanceTable<u32>,
    pub timer_state: TrackBuilder<TimerRec>,
    pub watchers: InstanceTable<u32>,
    pub watcher_state: TrackBuilder<WatcherRec>,
    pub cp_state: TrackBuilder<CpStateRec>,
    pub cp_progress: TrackBuilder<ProgressRec>,
    /// previous points per control point (key = point 0..7)
    pub cp_prev: TrackBuilder<PrevRec>,
    /// base control points and mini rounds (key 0)
    pub cp_base: TrackBuilder<BaseRec>,
    pub pd: TrackBuilder<PdRec>,
    pub pd_inst: Vec<(u32, u32, bool)>,
    pub game_type: i64,
    pub koth: bool,
    pub cp_count: i64,
    rules_dirty: bool,
    cp_dirty: bool,
    prev_dirty: bool,
    last_flags: Option<u16>,
    in_overtime: bool,
    sim: [CpSim; 8],
    last_sim_tick: Option<i64>,
    cp_static: Option<CpStatic>,
}

impl ObState {
    pub fn new(ticks: u32) -> ObState {
        ObState {
            rules_ent: None,
            objres_ent: None,
            rules: TrackBuilder::new(ticks),
            teams: TrackBuilder::new(ticks),
            timers: InstanceTable::new(),
            timer_state: TrackBuilder::new(ticks),
            watchers: InstanceTable::new(),
            watcher_state: TrackBuilder::new(ticks),
            cp_state: TrackBuilder::new(ticks),
            cp_progress: TrackBuilder::new(ticks),
            cp_prev: TrackBuilder::new(ticks),
            cp_base: TrackBuilder::new(ticks),
            pd: TrackBuilder::new(ticks),
            pd_inst: Vec::new(),
            game_type: 0,
            koth: false,
            cp_count: 0,
            rules_dirty: false,
            cp_dirty: false,
            prev_dirty: false,
            last_flags: None,
            in_overtime: false,
            sim: [CpSim::default(); 8],
            last_sim_tick: None,
            cp_static: None,
        }
    }
}

impl Ex {
    fn rules_slots_ent(&self) -> Option<(&Ent, std::rc::Rc<ClassInfo>)> {
        let ent = self.ents.get(self.ob.rules_ent? as usize)?.as_ref()?;
        let ci = self.class_info(ent.class)?;
        Some((ent, ci))
    }

    fn objres_ent(&self) -> Option<(&Ent, std::rc::Rc<ClassInfo>)> {
        let ent = self.ents.get(self.ob.objres_ent? as usize)?.as_ref()?;
        let ci = self.class_info(ent.class)?;
        Some((ent, ci))
    }

    /// Timer instance (provisional) at entity index `e`, -1 when 0 or unresolved.
    fn timer_at(&self, e: i64) -> i16 {
        if e <= 0 {
            return -1;
        }
        match self.ents.get(e as usize).and_then(|x| x.as_ref()) {
            Some(t) if t.role == Role::Timer => t.slot.min(i16::MAX as u32) as i16,
            _ => -1,
        }
    }

    pub(crate) fn rules_create(&mut self, idx: u32, ent: &mut Ent, _ci: &ClassInfo, _st: i64) {
        ent.role = Role::Rules;
        self.ob.rules_ent = Some(idx);
        self.ob.rules_dirty = true;
    }

    pub(crate) fn rules_update(
        &mut self,
        _idx: u32,
        _ent: &mut Ent,
        _ci: &ClassInfo,
        _changed: &[(u32, Val)],
        _st: i64,
    ) {
        self.ob.rules_dirty = true;
    }

    pub(crate) fn objres_create(&mut self, idx: u32, ent: &mut Ent, _ci: &ClassInfo, _st: i64) {
        ent.role = Role::ObjRes;
        self.ob.objres_ent = Some(idx);
        self.ob.rules_dirty = true;
        self.ob.cp_dirty = true;
        self.ob.prev_dirty = true;
    }

    pub(crate) fn objres_update(
        &mut self,
        _idx: u32,
        _ent: &mut Ent,
        ci: &ClassInfo,
        changed: &[(u32, Val)],
        _st: i64,
    ) {
        let mut bits = 0;
        for (i, _) in changed {
            bits |= ci.dep.get(*i as usize).copied().unwrap_or(0);
        }
        if bits & O_RULES != 0 {
            self.ob.rules_dirty = true;
        }
        if bits & O_CP != 0 {
            self.ob.cp_dirty = true;
        }
        if bits & O_PREV != 0 {
            self.ob.prev_dirty = true;
        }
    }

    pub(crate) fn objres_end(&mut self, ent: Ent) {
        if let Some(st) = self.cp_static_of(&ent) {
            self.ob.cp_static = Some(st);
        }
        self.ob.objres_ent = None;
    }

    pub(crate) fn team_update(&mut self, _idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        ent.role = Role::Team;
        let Slots::Team(s) = &ci.slots else { return };
        let team = ent.i0(s.num);
        if !(0..4).contains(&team) || !self.incl.objectives {
            return;
        }
        let rec = TeamRec {
            score: ent.i0(s.score).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            rounds_won: clamp_u16(ent.i0(s.rounds_won)),
            role: to_u8(ent.i0(s.role)),
            flag_caps: to_u8(ent.i0(s.flag_caps)),
        };
        self.ob.teams.push(team as usize, st, rec);
    }

    pub(crate) fn timer_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let t = self.ob.timers.add(idx, st, idx);
        ent.role = Role::Timer;
        ent.slot = t;
        self.ob.rules_dirty = true;
        self.timer_update(idx, ent, ci, st);
    }

    pub(crate) fn timer_update(&mut self, _idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        if !self.incl.objectives {
            return;
        }
        let Slots::Timer(s) = &ci.slots else { return };
        let mut flags = 0u8;
        for (bit, slot) in s.flags.iter().enumerate() {
            if ent.b(*slot) {
                flags |= 1 << bit;
            }
        }
        let rec = TimerRec {
            paused: ent.b(s.paused) as u8,
            end_tick: self.tick_f(ent.f0(s.end_time)),
            remaining: ent.f0(s.remaining),
            length: clamp_u16(ent.i0(s.length)),
            max_length: clamp_u16(ent.i0(s.max_length)),
            setup_length: clamp_u16(ent.i0(s.setup_length)),
            mode: to_u8(ent.i0(s.state)),
            flags,
            total_time: ent.f0(s.total_time),
        };
        self.ob.timer_state.push(ent.slot as usize, st, rec);
    }

    pub(crate) fn timer_end(&mut self, ent: &Ent, st: i64) {
        if let Some(r) = self.ob.timers.get_mut(ent.slot) {
            r.deleted = Some(st);
        }
        self.ob.rules_dirty = true;
    }

    pub(crate) fn watcher_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        let w = self.ob.watchers.add(idx, st, idx);
        ent.role = Role::Watcher;
        ent.slot = w;
        self.watcher_update(idx, ent, ci, st);
    }

    pub(crate) fn watcher_update(&mut self, _idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        if !self.incl.objectives {
            return;
        }
        let Slots::Watcher(s) = &ci.slots else { return };
        let rec = WatcherRec {
            progress: ((ent.f0(s.progress) as f64).clamp(0.0, 1.0) * 65535.0).round() as u16,
            speed: to_i8(ent.i0(s.speed)),
            cappers: to_i8(ent.i0(s.cappers)),
            recede_tick: self.tick_f_nan(ent.f0(s.recede)),
        };
        self.ob.watcher_state.push(ent.slot as usize, st, rec);
    }

    pub(crate) fn pd_create(&mut self, idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        ent.role = Role::Pd;
        let n = self.ob.pd_inst.len() as u32;
        ent.slot = n;
        self.ob.pd_inst.push((idx, ent.serial, true));
        self.pd_update(idx, ent, ci, st);
    }

    pub(crate) fn pd_update(&mut self, _idx: u32, ent: &mut Ent, ci: &ClassInfo, st: i64) {
        // key 0 follows the newest alive instance
        let newest = self
            .ob
            .pd_inst
            .iter()
            .enumerate()
            .filter(|(_, x)| x.2)
            .map(|(i, _)| i as u32)
            .max();
        if newest != Some(ent.slot) || !self.incl.objectives {
            return;
        }
        let Slots::Pd(s) = &ci.slots else { return };
        let rec = PdRec {
            red: clamp_u16(ent.i0(s.red)),
            blu: clamp_u16(ent.i0(s.blu)),
            max: clamp_u16(ent.i0(s.max)),
        };
        self.ob.pd.push(0, st, rec);
    }

    pub(crate) fn pd_end(&mut self, ent: &Ent, st: i64) {
        if let Some(x) = self.ob.pd_inst.get_mut(ent.slot as usize) {
            x.2 = false;
        }
        // the previous alive instance takes over
        let newest = self
            .ob
            .pd_inst
            .iter()
            .filter(|x| x.2)
            .map(|x| (x.0, x.1))
            .next_back();
        if let Some((i, serial)) = newest {
            if let Some(e) = self.ents.get(i as usize).and_then(|e| e.as_ref()) {
                if e.serial == serial {
                    if let Some(ci) = self.class_info(e.class) {
                        if let Slots::Pd(s) = &ci.slots {
                            let rec = PdRec {
                                red: clamp_u16(e.i0(s.red)),
                                blu: clamp_u16(e.i0(s.blu)),
                                max: clamp_u16(e.i0(s.max)),
                            };
                            if self.incl.objectives {
                                self.ob.pd.push(0, st, rec);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn ob_flush(&mut self, st: i64) {
        if self.ob.rules_dirty {
            self.ob.rules_dirty = false;
            self.rules_push(st);
        }
        if self.ob.cp_dirty {
            self.ob.cp_dirty = false;
            self.cp_push(st);
        }
        if self.ob.prev_dirty {
            self.ob.prev_dirty = false;
            self.prev_push(st);
        }
    }

    /// Survey of the previous-point bookkeeping (native diagnostics): how often `m_iPreviousPoints`, the base
    /// points and the mini-round flag change after their first value, and the final values.
    pub(crate) fn prev_survey(
        &self,
    ) -> (
        std::collections::BTreeMap<String, f64>,
        std::collections::BTreeMap<String, String>,
    ) {
        let mut counts = std::collections::BTreeMap::new();
        let mut notes = std::collections::BTreeMap::new();
        let mut changes = 0usize;
        let mut entries = 0usize;
        let mut change_ticks: Vec<u32> = Vec::new();
        let mut finals: Vec<String> = Vec::new();
        for key in 0..8usize {
            let list = self.ob.cp_prev.entries(key);
            entries += list.len();
            changes += list.len().saturating_sub(1);
            for (t, _) in list.iter().skip(1) {
                change_ticks.push(*t);
            }
            if let Some((_, r)) = list.last() {
                finals.push(format!(
                    "{}:r[{},{},{}]b[{},{},{}]",
                    key, r.red0, r.red1, r.red2, r.blu0, r.blu1, r.blu2
                ));
            }
        }
        change_ticks.sort_unstable();
        change_ticks.dedup();
        let base = self.ob.cp_base.entries(0);
        counts.insert("survey.prev.entries".to_string(), entries as f64);
        counts.insert("survey.prev.changes".to_string(), changes as f64);
        counts.insert(
            "survey.prev.changeTicks".to_string(),
            change_ticks.len() as f64,
        );
        if let Some(t) = change_ticks.first() {
            counts.insert("survey.prev.firstChangeTick".to_string(), *t as f64);
        }
        counts.insert(
            "survey.base.changes".to_string(),
            base.len().saturating_sub(1) as f64,
        );
        notes.insert("prev.final".to_string(), finals.join(" "));
        notes.insert(
            "prev.changeTicks".to_string(),
            change_ticks
                .iter()
                .take(12)
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .join(","),
        );
        notes.insert(
            "base.entries".to_string(),
            base.iter()
                .take(12)
                .map(|(t, r)| format!("{t}:{}/{}/{}", r.red, r.blu, r.mini))
                .collect::<Vec<_>>()
                .join(" "),
        );
        (counts, notes)
    }

    /// Previous points of every control point for RED and BLU, plus the base points and mini rounds.
    /// A prop the demo never sent reads -1 (the server's `ResetControlPoints` value).
    #[inline(never)]
    fn prev_push(&mut self, st: i64) {
        if !self.incl.objectives {
            return;
        }
        let Some((ent, ci)) = self.objres_ent() else {
            return;
        };
        let Slots::ObjRes(s) = &ci.slots else { return };
        let val = |slot: Option<u32>| -> i8 { to_i8(ent.i(slot).unwrap_or(-1)) };
        let at = |point: usize, team: usize, slot: usize| -> i8 {
            val(s.prev.get(prev_index(point, team, slot)).copied().flatten())
        };
        let mut recs = Vec::with_capacity(8);
        for i in 0..8usize {
            recs.push(PrevRec {
                red0: at(i, 2, 0),
                red1: at(i, 2, 1),
                red2: at(i, 2, 2),
                blu0: at(i, 3, 0),
                blu1: at(i, 3, 1),
                blu2: at(i, 3, 2),
            });
        }
        let base = BaseRec {
            red: val(s.base.get(2).copied().flatten()),
            blu: val(s.base.get(3).copied().flatten()),
            mini: u8::from(ent.b(s.mini_rounds)),
        };
        for (i, r) in recs.into_iter().enumerate() {
            self.ob.cp_prev.push(i, st, r);
        }
        self.ob.cp_base.push(0, st, base);
    }

    #[inline(never)]
    fn rules_push(&mut self, st: i64) {
        let Some((ent, ci)) = self.rules_slots_ent() else {
            return;
        };
        let Slots::Rules(s) = &ci.slots else { return };
        let mut flags = 0u16;
        for (bit, slot) in s.flags.iter().enumerate() {
            if ent.b(*slot) {
                flags |= 1 << bit;
            }
        }
        let (hud, sw) = match self.objres_ent() {
            Some((oe, oci)) => match &oci.slots {
                Slots::ObjRes(os) => (
                    self.timer_at(oe.i0(os.timer_hud)),
                    self.timer_at(oe.i0(os.sw_timer)),
                ),
                _ => (-1, -1),
            },
            None => (-1, -1),
        };
        let timer_of = |slot: Option<u32>| -> i16 {
            self.slot_of_handle(ent.h(slot), Role::Timer)
                .map(|t| t.min(i16::MAX as u32) as i16)
                .unwrap_or(-1)
        };
        let restart = ent.f0(s.restart);
        let rec = RulesRec {
            round_state: to_u8(ent.i0(s.round_state)),
            winner: to_u8(ent.i0(s.winner)),
            flags,
            sw_state: to_u8(ent.i0(s.sw_state)),
            rounds: to_u8(ent.i0(s.rounds)),
            red_koth: timer_of(s.red_koth),
            blu_koth: timer_of(s.blue_koth),
            hud_timer: hud,
            sw_timer: sw,
            wave_red: self.tick_f_nan(ent.f0(s.wave_red)),
            wave_blu: self.tick_f_nan(ent.f0(s.wave_blu)),
            restart_tick: if restart > 0.0 {
                self.tick_f(restart)
            } else {
                f32::NAN
            },
        };
        let game_type = ent.i0(s.game_type);
        let koth = ent.b(s.koth);
        self.ob.game_type = game_type;
        self.ob.koth = koth;
        let ot = flags & RULES_IN_OVERTIME != 0;
        let prev_ot = self
            .ob
            .last_flags
            .map(|f| f & RULES_IN_OVERTIME != 0)
            .unwrap_or(false);
        self.ob.last_flags = Some(flags);
        self.ob.in_overtime = ot;
        if self.incl.objectives {
            self.ob.rules.push(0, st, rec);
        }
        if ot && !prev_ot {
            self.ev_push(st, HlEvent::OvertimeStart { tick: 0 });
        } else if !ot && prev_ot {
            self.ev_push(st, HlEvent::OvertimeEnd { tick: 0 });
        }
    }

    #[inline(never)]
    fn cp_push(&mut self, st: i64) {
        let Some((ent, ci)) = self.objres_ent() else {
            return;
        };
        let Slots::ObjRes(s) = &ci.slots else { return };
        let n = ent.i0(s.num_cp).clamp(0, 8);
        let mut recs = Vec::with_capacity(n as usize);
        for i in 0..n as usize {
            let g = |arr: &[Option<u32>; 8]| arr.get(i).copied().flatten();
            let t = |arr: &Vec<Option<u32>>, team: usize| arr.get(team * 8 + i).copied().flatten();
            let mut flags = 0u8;
            if ent.b(g(&s.blocked)) {
                flags |= 1;
            }
            if ent.b(g(&s.locked)) {
                flags |= 2;
            }
            if ent.b(t(&s.can_cap, 2)) {
                flags |= 4;
            }
            if ent.b(t(&s.can_cap, 3)) {
                flags |= 8;
            }
            if ent.b(g(&s.visible)) {
                flags |= 16;
            }
            recs.push(CpStateRec {
                owner: to_u8(ent.i0(g(&s.owner))),
                capping: to_u8(ent.i0(g(&s.capping))),
                in_zone: to_u8(ent.i0(g(&s.in_zone))),
                num_red: to_u8(ent.i0(t(&s.members, 2))),
                num_blu: to_u8(ent.i0(t(&s.members, 3))),
                lazy: ent.f0(g(&s.lazy)),
                flags,
                unlock_tick: self.tick_f_nan(ent.f0(g(&s.unlock))),
            });
        }
        if n > self.ob.cp_count {
            self.ob.cp_count = n;
        }
        if self.incl.objectives {
            for (i, r) in recs.into_iter().enumerate() {
                self.ob.cp_state.push(i, st, r);
            }
        }
    }

    /// Capture progress simulation (port of `C_BaseTeamObjectiveResource::ClientThink`).
    #[inline(never)]
    pub(crate) fn cp_simulate(&mut self, st: i64) {
        if self.ob.objres_ent.is_none() {
            return;
        }
        let ipt = self.interval();
        let dt = match self.ob.last_sim_tick {
            Some(prev) if st > prev => ipt * (st - prev) as f64,
            _ => 0.0,
        };
        self.ob.last_sim_tick = Some(st);
        let Some((ent, ci)) = self.objres_ent() else {
            return;
        };
        let Slots::ObjRes(s) = &ci.slots else { return };
        let capstyle = self
            .cvar
            .get("mp_capstyle")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(1);
        let deteriorate_time = self
            .cvar
            .get("mp_capdeteriorate_time")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(90.0);
        let overtime = self.ob.in_overtime;
        let n = ent.i0(s.num_cp).clamp(0, 8) as usize;
        let mut pushes: Vec<(usize, u16)> = Vec::with_capacity(n);
        let mut sims = self.ob.sim;
        for (i, sim) in sims.iter_mut().enumerate().take(n) {
            let g = |arr: &[Option<u32>; 8]| arr.get(i).copied().flatten();
            let tt = |arr: &Vec<Option<u32>>, team: i64| -> Option<u32> {
                if team == 2 || team == 3 {
                    arr.get(team as usize * 8 + i).copied().flatten()
                } else {
                    None
                }
            };
            let cap_time = |team: i64| -> f64 {
                match tt(&s.cap_time, team) {
                    Some(sl) => ent.f0(Some(sl)) as f64,
                    None => 0.0,
                }
            };
            let members = |team: i64| -> i64 {
                match tt(&s.members, team) {
                    Some(sl) => ent.i0(Some(sl)),
                    None => 0,
                }
            };
            let can_cap = |team: i64| -> bool {
                match tt(&s.can_cap, team) {
                    Some(sl) => ent.b(Some(sl)),
                    None => false,
                }
            };
            let capping = ent.i0(g(&s.capping));
            let owner = ent.i0(g(&s.owner));
            let lazy = ent.f0(g(&s.lazy));
            let blocked = ent.b(g(&s.blocked));
            let locked = ent.b(g(&s.locked));
            let scales = ent.b(g(&s.scales));
            let in_zone = ent.i0(g(&s.in_zone));
            if !sim.init {
                sim.init = true;
                sim.capping = capping;
                sim.owner = owner;
                sim.lazy_bits = lazy.to_bits();
                sim.sim_capping = capping;
                sim.left = lazy as f64 * cap_time(capping);
                if sim.left < 0.0 || capping <= 1 {
                    sim.left = sim.left.max(0.0);
                }
            } else {
                if capping != sim.capping {
                    sim.capping = capping;
                    sim.sim_capping = capping;
                    sim.left = if capping != owner && capping > 1 {
                        cap_time(capping)
                    } else {
                        0.0
                    };
                }
                if owner != sim.owner {
                    sim.owner = owner;
                    if owner == sim.sim_capping {
                        sim.sim_capping = 0;
                        sim.left = 0.0;
                    }
                }
                if lazy.to_bits() != sim.lazy_bits {
                    sim.lazy_bits = lazy.to_bits();
                    sim.left = lazy as f64 * cap_time(sim.sim_capping);
                }
            }
            let cap = sim.sim_capping;
            if dt > 0.0 && sim.left != 0.0 && !blocked {
                let team = in_zone;
                let num = members(team);
                let mut deteriorate = true;
                if num > 0 {
                    let mut reduction = dt;
                    if capstyle == 1 && scales {
                        for k in 1..num {
                            reduction += dt / (k + 1) as f64;
                        }
                    }
                    if team == cap {
                        sim.left -= reduction;
                        deteriorate = false;
                    } else if owner == 0 && team != 0 {
                        sim.left += reduction;
                        deteriorate = false;
                    }
                }
                if deteriorate {
                    if !locked && can_cap(cap) {
                        let len = cap_time(cap);
                        let scale = if scales { deteriorate_time } else { len };
                        let mut dec = if scale > 0.0 { (len / scale) * dt } else { 0.0 };
                        if overtime {
                            dec *= 6.0;
                        }
                        sim.left += dec;
                    } else {
                        sim.left = 0.0;
                    }
                }
                // The client lets capTimeLeft go negative while the capping team stays on a
                // finished point, so its bar shows 100 % until the owner change arrives; hold a
                // tiny positive remainder instead of 0, which would read as "not capping".
                let finished = sim.left <= 0.0 && !deteriorate;
                sim.left = sim.left.clamp(0.0, cap_time(cap).max(0.0));
                if finished && cap > 1 {
                    sim.left = 1e-9;
                }
            }
            let len = cap_time(cap);
            let progress = if len > 0.0 && sim.left != 0.0 {
                (len - sim.left) / len
            } else {
                0.0
            };
            pushes.push((i, (progress.clamp(0.0, 1.0) * 65535.0).round() as u16));
        }
        self.ob.sim = sims;
        if self.incl.objectives {
            for (i, p) in pushes {
                self.ob.cp_progress.push(i, st, ProgressRec { p });
            }
        }
    }

    #[inline(never)]
    fn cp_static_of(&self, ent: &Ent) -> Option<CpStatic> {
        let ci = self.class_info(ent.class)?;
        let Slots::ObjRes(s) = &ci.slots else {
            return None;
        };
        let mut pos = vec![0.0f32; 24];
        if let Some(ArrVal::Vecs(list)) = ent.array(s.positions) {
            for (i, v) in list.iter().take(8).enumerate() {
                for a in 0..3 {
                    if let (Some(cell), Some(x)) = (pos.get_mut(i * 3 + a), v.get(a)) {
                        *cell = *x;
                    }
                }
            }
        }
        let u8s =
            |arr: &[Option<u32>; 8]| -> Vec<u8> { arr.iter().map(|x| to_u8(ent.i0(*x))).collect() };
        let f32s =
            |arr: &[Option<u32>; 8]| -> Vec<f32> { arr.iter().map(|x| ent.f0(*x)).collect() };
        let team_f = |arr: &Vec<Option<u32>>, team: usize| -> Vec<f32> {
            (0..8)
                .map(|i| ent.f0(arr.get(team * 8 + i).copied().flatten()))
                .collect()
        };
        let team_u8 = |arr: &Vec<Option<u32>>, team: usize| -> Vec<u8> {
            (0..8)
                .map(|i| to_u8(ent.i0(arr.get(team * 8 + i).copied().flatten())))
                .collect()
        };
        let layout = match ent.get(s.layout) {
            Val::S(id) => self.strings.get(id).to_string(),
            _ => String::new(),
        };
        Some(CpStatic {
            layout,
            pos,
            visible: u8s(&s.visible),
            group: u8s(&s.group),
            cap_red: team_f(&s.cap_time, 2),
            cap_blu: team_f(&s.cap_time, 3),
            req_red: team_u8(&s.req, 2),
            req_blu: team_u8(&s.req, 3),
            path: f32s(&s.path),
            scales: u8s(&s.scales),
        })
    }

    #[inline(never)]
    pub(crate) fn objectives_out(&mut self, ord: &Orders) -> XNode {
        let to = &ord.timers;
        let ticks = self.ticks;
        self.ob.rules.map_records(|r| {
            r.red_koth = to.map_i16(r.red_koth);
            r.blu_koth = to.map_i16(r.blu_koth);
            r.hud_timer = to.map_i16(r.hud_timer);
            r.sw_timer = to.map_i16(r.sw_timer);
        });
        let cp_static = match self.objres_ent() {
            Some((ent, _)) => self.cp_static_of(ent),
            None => None,
        }
        .or_else(|| self.ob.cp_static.clone())
        .unwrap_or_default();
        let pad = |v: Vec<u8>| -> Vec<u8> {
            let mut v = v;
            v.resize(8, 0);
            v
        };
        let padf = |v: Vec<f32>| -> Vec<f32> {
            let mut v = v;
            v.resize(8, 0.0);
            v
        };
        let mut posv = cp_static.pos.clone();
        posv.resize(24, 0.0);
        // Previous points, base points and mini rounds (P2): static, because no local demo changes them within
        // the demo (133-demo survey); the last value wins, -1 / false without an objective resource.
        let mut prev_red = vec![-1i8; 24];
        let mut prev_blu = vec![-1i8; 24];
        for i in 0..8usize {
            if let Some(r) = self.ob.cp_prev.last(i) {
                for (slot, (red, blu)) in [(r.red0, r.blu0), (r.red1, r.blu1), (r.red2, r.blu2)]
                    .into_iter()
                    .enumerate()
                {
                    if let Some(c) = prev_red.get_mut(i * 3 + slot) {
                        *c = red;
                    }
                    if let Some(c) = prev_blu.get_mut(i * 3 + slot) {
                        *c = blu;
                    }
                }
            }
        }
        let (base_red, base_blu, mini) = match self.ob.cp_base.last(0) {
            Some(b) => (b.red, b.blu, b.mini != 0),
            None => (-1, -1, false),
        };
        let ob = &mut self.ob;
        let cp_state =
            std::mem::replace(&mut ob.cp_state, TrackBuilder::new(1)).into_fields(8, None);
        let cp_progress =
            std::mem::replace(&mut ob.cp_progress, TrackBuilder::new(1)).into_fields(8, None);
        let cp = ObjB::new()
            .put("count", XNode::Num(ob.cp_count as f64))
            .put("layout", XNode::Str(cp_static.layout.clone()))
            .put("pos", XNode::F32(posv))
            .put("visible", XNode::U8(pad(cp_static.visible.clone())))
            .put("group", XNode::U8(pad(cp_static.group.clone())))
            .put("capTimeRed", XNode::F32(padf(cp_static.cap_red.clone())))
            .put("capTimeBlu", XNode::F32(padf(cp_static.cap_blu.clone())))
            .put("reqRed", XNode::U8(pad(cp_static.req_red.clone())))
            .put("reqBlu", XNode::U8(pad(cp_static.req_blu.clone())))
            .put("pathDistance", XNode::F32(padf(cp_static.path.clone())))
            .put("scales", XNode::U8(pad(cp_static.scales.clone())))
            .put("state", XNode::Obj(cp_state))
            .put("progress", XNode::Obj(cp_progress))
            .put("prevRed", XNode::I8(prev_red))
            .put("prevBlu", XNode::I8(prev_blu))
            .put("baseRed", XNode::Num(base_red as f64))
            .put("baseBlu", XNode::Num(base_blu as f64))
            .put("miniRounds", XNode::Bool(mini))
            .build();
        let timers_n = to.count();
        let timer_order = to.order.clone();
        let timers = ObjB::new()
            .extend(ob.timers.base_fields(to, ticks))
            .put(
                "state",
                XNode::Obj(
                    std::mem::replace(&mut ob.timer_state, TrackBuilder::new(1))
                        .into_fields(timers_n, Some(&timer_order)),
                ),
            )
            .build();
        let wo = ob.watchers.order(ticks);
        let watchers = ObjB::new()
            .extend(ob.watchers.base_fields(&wo, ticks))
            .put(
                "state",
                XNode::Obj(
                    std::mem::replace(&mut ob.watcher_state, TrackBuilder::new(1))
                        .into_fields(wo.count(), Some(&wo.order)),
                ),
            )
            .build();
        let rules = std::mem::replace(&mut ob.rules, TrackBuilder::new(1)).into_fields(1, None);
        let teams = std::mem::replace(&mut ob.teams, TrackBuilder::new(1)).into_fields(4, None);
        let pd = std::mem::replace(&mut ob.pd, TrackBuilder::new(1)).into_fields(1, None);
        let game_type = ob.game_type;
        let koth = ob.koth;
        let cart = self.cart_out(ord);
        ObjB::new()
            .put("gameType", XNode::Num(game_type as f64))
            .put("koth", XNode::Bool(koth))
            .put("rules", XNode::Obj(rules))
            .put("teams", XNode::Obj(teams))
            .put("timers", timers)
            .put("cp", cp)
            .put("watchers", watchers)
            .put("cart", cart)
            .put("pd", XNode::Obj(pd))
            .build()
    }
}

#[inline(never)]
pub fn objectives_empty() -> XNode {
    let cp = ObjB::new()
        .put("count", XNode::Num(0.0))
        .put("layout", XNode::Str(String::new()))
        .put("pos", XNode::F32(vec![0.0; 24]))
        .put("visible", XNode::U8(vec![0; 8]))
        .put("group", XNode::U8(vec![0; 8]))
        .put("capTimeRed", XNode::F32(vec![0.0; 8]))
        .put("capTimeBlu", XNode::F32(vec![0.0; 8]))
        .put("reqRed", XNode::U8(vec![0; 8]))
        .put("reqBlu", XNode::U8(vec![0; 8]))
        .put("pathDistance", XNode::F32(vec![0.0; 8]))
        .put("scales", XNode::U8(vec![0; 8]))
        .put("state", XNode::Obj(empty_track::<CpStateRec>(8)))
        .put("progress", XNode::Obj(empty_track::<ProgressRec>(8)))
        .put("prevRed", XNode::I8(vec![-1; 24]))
        .put("prevBlu", XNode::I8(vec![-1; 24]))
        .put("baseRed", XNode::Num(-1.0))
        .put("baseBlu", XNode::Num(-1.0))
        .put("miniRounds", XNode::Bool(false))
        .build();
    ObjB::new()
        .put("gameType", XNode::Num(0.0))
        .put("koth", XNode::Bool(false))
        .put("rules", XNode::Obj(empty_track::<RulesRec>(1)))
        .put("teams", XNode::Obj(empty_track::<TeamRec>(4)))
        .put(
            "timers",
            ObjB::new()
                .extend(empty_instances())
                .put("state", XNode::Obj(empty_track::<TimerRec>(0)))
                .build(),
        )
        .put("cp", cp)
        .put(
            "watchers",
            ObjB::new()
                .extend(empty_instances())
                .put("state", XNode::Obj(empty_track::<WatcherRec>(0)))
                .build(),
        )
        .put(
            "cart",
            ObjB::new()
                .put("movers", XNode::I32(Vec::new()))
                .put("trains", XNode::I32(Vec::new()))
                .build(),
        )
        .put("pd", XNode::Obj(empty_track::<PdRec>(1)))
        .build()
}
