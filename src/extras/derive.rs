//! Native-only derivation of the base values (rule 2.3.11): reproduces the start tick, tick base,
//! tick count and player index mapping of `lib.rs` `parse_demo_cache_internal` with the same
//! analysers, so `hl_extras` can run without parser2. The browser never uses this.

use std::collections::BTreeMap;

use bitbuffer::BitRead;

use crate::demo::header::Header;
use crate::demo::message::EntityId;
use crate::demo::parser::analyser::UserInfo;
use crate::demo::parser::handler::DemoHandler;
use crate::demo::parser::RawPacketStream;
use crate::{CombinedAnalyser, Demo};

use super::ExtrasOptions;

/// Base values as `lib.rs` computes them.
#[derive(Clone, Debug)]
pub struct DerivedBase {
    pub options: ExtrasOptions,
    pub start_demo_tick: u32,
    pub next_mapped_player: usize,
    /// player names by player index (empty for indices without a `players` entry)
    pub names: Vec<String>,
    pub world_min: [f32; 3],
    pub world_max: [f32; 3],
    pub header_ticks: u32,
    /// whether lib.rs's start rule fired (100 packets with players and non-zero world bounds)
    pub started: bool,
    /// demo tick of the first packet with non-zero world bounds (diagnostics)
    pub first_world_tick: Option<u32>,
    pub warmup_frames: u32,
}

pub fn derive_options(buffer: &[u8]) -> Result<DerivedBase, String> {
    derive_options_with(buffer, true)
}

/// `require_world = false` drops the world-bounds condition of the start rule (native
/// diagnostics for tow_tetsudo demos whose world bounds stay zero, where lib.rs and parser2
/// never start).
pub fn derive_options_with(buffer: &[u8], require_world: bool) -> Result<DerivedBase, String> {
    let demo = Demo::new(buffer);
    let mut stream = demo.get_stream();
    let header =
        Header::read(&mut stream).map_err(|_| "Failed to parse demo header".to_string())?;
    let mut handler = DemoHandler::with_analyser(CombinedAnalyser::default());
    handler.handle_header(&header);
    let mut packets = RawPacketStream::new(stream);

    let tick_capacity = header.ticks.max(1) as usize;
    let mut start_server_tick: u32 = 0;
    let mut start_demo_tick: u32 = 0;
    let mut start_tick_base: i64 = 0;
    let mut started = false;
    let mut warmup_frames = 0u32;
    let mut player_ids: BTreeMap<EntityId, usize> = BTreeMap::new();
    let mut user_info_by_entity: BTreeMap<EntityId, UserInfo> = BTreeMap::new();
    let mut last_snapshot_tick: Option<u32> = None;
    let mut max_written_tick: u32 = 0;
    let mut server_tick_base: Option<i64> = None;
    let mut interval_per_tick = 0.0f32;
    let mut world_min = [0.0f32; 3];
    let mut world_max = [0.0f32; 3];
    let mut first_world_tick: Option<u32> = None;

    loop {
        let packet = match packets.next(handler.get_parser_state()) {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(_) => return Err("Failed to parse demo packet".to_string()),
        };
        let packet_tick: u32 = packet.tick().into();
        if handler.handle_packet(packet).is_err() {
            return Err("Failed to parse demo packet".to_string());
        }
        let server_tick: u32 = u32::from(handler.server_tick);
        let parser_state = handler.get_parser_state();
        let analyser = handler.analyser();
        let match_state = analyser.match_state(parser_state);
        let game_state = analyser.game_state(parser_state);

        if server_tick_base.is_none() && (match_state.start_tick != 0 || server_tick != 0) {
            server_tick_base = Some(server_tick as i64 - packet_tick as i64);
        }
        if let Some(w) = &game_state.world {
            world_min = [w.boundary_min.x, w.boundary_min.y, w.boundary_min.z];
            world_max = [w.boundary_max.x, w.boundary_max.y, w.boundary_max.z];
        }
        interval_per_tick = game_state
            .interval_per_tick
            .max(match_state.interval_per_tick);

        if !started {
            if !game_state.players.is_empty() {
                warmup_frames += 1;
            }
            let world_ready = !(world_min == [0.0; 3] && world_max == [0.0; 3]);
            if world_ready && first_world_tick.is_none() {
                first_world_tick = Some(packet_tick);
            }
            if warmup_frames >= 100 && (world_ready || !require_world) {
                started = true;
                start_server_tick = server_tick;
                start_demo_tick = packet_tick;
                start_tick_base = server_tick as i64 - packet_tick as i64;
            } else {
                continue;
            }
        }
        if server_tick < start_server_tick {
            continue;
        }
        let internal_tick = server_tick - start_server_tick;
        if internal_tick as usize >= tick_capacity {
            continue;
        }
        if let Some(prev) = last_snapshot_tick {
            if internal_tick <= prev {
                continue;
            }
        }
        for player in game_state.players.iter() {
            if !player_ids.contains_key(&player.entity) {
                let next = player_ids.len();
                player_ids.insert(player.entity, next);
            }
            if let Some(info) = player.info.as_ref() {
                user_info_by_entity.insert(player.entity, info.clone());
            }
        }
        max_written_tick = internal_tick;
        last_snapshot_tick = Some(internal_tick);
    }

    let tick_base = if started {
        start_tick_base
    } else {
        server_tick_base.unwrap_or(start_tick_base)
    };
    let output = handler.into_output();
    let users_by_entity: BTreeMap<EntityId, UserInfo> = output
        .match_state
        .users
        .values()
        .map(|user| (user.entity_id, user.clone()))
        .collect();
    let mut next_mapped_player = 0usize;
    let mut with_user: Vec<(usize, u32, String)> = Vec::new();
    for (entity_id, index) in player_ids.iter() {
        let user = users_by_entity
            .get(entity_id)
            .cloned()
            .or_else(|| user_info_by_entity.get(entity_id).cloned());
        if let Some(user) = user {
            with_user.push((*index, u32::from(*entity_id), user.name.clone()));
            next_mapped_player = next_mapped_player.max(index + 1);
        }
    }
    let mut player_entity_ids = vec![0u32; next_mapped_player];
    let mut names = vec![String::new(); next_mapped_player];
    for (index, entity, name) in with_user {
        if let Some(cell) = player_entity_ids.get_mut(index) {
            *cell = entity;
        }
        if let Some(cell) = names.get_mut(index) {
            *cell = name;
        }
    }
    Ok(DerivedBase {
        options: ExtrasOptions {
            start_tick: start_server_tick,
            tick_base,
            ticks: (max_written_tick + 1).max(1),
            interval_per_tick,
            player_entity_ids,
            groups: None,
            wearable_defs: None,
        },
        start_demo_tick,
        next_mapped_player,
        names,
        world_min,
        world_max,
        header_ticks: header.ticks,
        started,
        first_world_tick,
        warmup_frames,
    })
}
